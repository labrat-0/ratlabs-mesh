//! A job is one folder: source art in, finished GLBs and review renders out.
//!
//! jobs/<id>/
//!   job.json            options + live status
//!   job.log
//!   source.png          what you uploaded; views/ holds optional back/left/right art
//!   prep.png            the cut-out sent to the engine
//!   v1/raw.glb          straight from the engine
//!   v1/<name>.glb       finished asset
//!   v1/renders/*.png    front / three_quarter / side / back, plus sheet.jpg
//!
//! The JSON layout is the same one the Python version wrote, so old jobs open unchanged.
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::config;
use crate::events::{Event, emit};
use crate::presets::preset;
use crate::{now, slug};

static SAVE_LOCK: Mutex<()> = Mutex::new(());

fn yes() -> bool {
    true
}
fn bottom() -> String {
    "bottom".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResult {
    pub variant: u32,
    pub glb: String,
    pub raw: String,
    pub renders: String,
    #[serde(default)]
    pub meta: Value,
    #[serde(default)]
    pub stats: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rigged: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rig: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub engine: String,
    pub variants: u32,
    #[serde(default = "standard")]
    pub quality: String,
    pub height: f64,
    pub polys: u32,
    pub texture: u32,
    #[serde(default = "bottom")]
    pub origin: String,
    #[serde(default)]
    pub crop: Option<[f64; 4]>,
    #[serde(default)]
    pub rotate: f64,
    #[serde(default = "yes")]
    pub pbr: bool,
    #[serde(default)]
    pub pose: Option<String>,
    #[serde(default)]
    pub seed: i64,
    #[serde(default)]
    pub keep_background: bool,
    #[serde(default = "yes")]
    pub delight: bool,
    #[serde(default)]
    pub keep_parts: bool,
    pub status: String,
    pub stage: String,
    pub created: f64,
    #[serde(default)]
    pub results: Vec<JobResult>,
    #[serde(default)]
    pub prep: Option<Value>,
    #[serde(default)]
    pub error: Option<String>,
    /// Who started it: "studio", "cli", or the name of the AI client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// Anything a newer or older version wrote that this one doesn't know about.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn standard() -> String {
    "standard".into()
}

/// Everything a caller can choose when starting a job. Unset fields come from the kind's preset.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NewJob {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub engine: Option<String>,
    pub variants: Option<u32>,
    pub quality: Option<String>,
    pub height: Option<f64>,
    pub polys: Option<u32>,
    pub texture: Option<u32>,
    pub origin: Option<String>,
    pub crop: Option<[f64; 4]>,
    pub rotate: Option<f64>,
    pub pbr: Option<bool>,
    pub pose: Option<String>,
    pub seed: Option<i64>,
    pub keep_background: Option<bool>,
    pub delight: Option<bool>,
    pub keep_parts: Option<bool>,
    #[serde(skip)]
    pub by: Option<String>,
}

pub fn job_dir(id: &str) -> Result<PathBuf> {
    let ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') && !id.starts_with('-');
    if !ok {
        bail!("bad job id");
    }
    Ok(config().jobs.join(id))
}

pub fn load(id: &str) -> Result<Job> {
    let p = job_dir(id)?.join("job.json");
    let text = std::fs::read_to_string(&p).with_context(|| format!("no job {id}"))?;
    Ok(serde_json::from_str(&text)?)
}

/// Write job.json atomically and tell every listener.
pub fn save(job: &Job) -> Result<()> {
    let d = job_dir(&job.id)?;
    {
        let _g = SAVE_LOCK.lock().unwrap();
        let tmp = d.join("job.json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(job)?)?;
        std::fs::rename(tmp, d.join("job.json"))?;
    }
    emit(Event::Job { job: Box::new(job.clone()) });
    Ok(())
}

/// All jobs, newest first (by creation time, so starter-pack items sit below your own work).
pub fn list() -> Vec<Job> {
    let ids: Vec<String> =
        std::fs::read_dir(&config().jobs).map(|rd| rd.filter_map(|e| e.ok()).filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default();
    let mut jobs: Vec<Job> = ids.iter().filter_map(|id| load(id).ok()).collect();
    jobs.sort_by(|a, b| b.created.total_cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
    jobs
}

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default()
}

/// Make a job folder. `glb` imports an existing model instead of generating one.
pub fn create(source: Option<&Path>, glb: Option<&Path>, views: &BTreeMap<String, PathBuf>, opt: NewJob) -> Result<Job> {
    let kind = opt.kind.unwrap_or_else(|| "character".into());
    let p = preset(&kind).ok_or_else(|| anyhow!("unknown kind {kind}"))?;
    let from = source.or(glb).ok_or_else(|| anyhow!("send an image or a glb"))?;
    let name = opt.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| from.file_stem().unwrap_or_default().to_string_lossy().into_owned());
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-").to_string();
    let mut id = stamp.clone() + &slug(&name);
    // Two jobs in the same second with the same name (an AI batching variants) get a suffix.
    let mut n = 2;
    while config().jobs.join(&id).exists() {
        id = format!("{stamp}{}-{n}", slug(&name));
        n += 1;
    }
    let d = config().jobs.join(&id);
    std::fs::create_dir_all(d.join("views"))?;
    if let Some(s) = source {
        std::fs::copy(s, d.join(format!("source{}", ext_of(s))))?;
    }
    for (side, path) in views {
        std::fs::copy(path, d.join("views").join(format!("{side}{}", ext_of(path))))?;
    }
    if let Some(g) = glb {
        std::fs::copy(g, d.join("import.glb"))?;
    }
    let job = Job {
        id,
        name,
        kind: kind.clone(),
        engine: if glb.is_some() { "import".into() } else { opt.engine.unwrap_or_else(|| "trellis2".into()) },
        variants: if glb.is_some() { 1 } else { opt.variants.unwrap_or(1).clamp(1, 4) },
        quality: opt.quality.unwrap_or_else(standard),
        height: opt.height.unwrap_or(p.height),
        polys: opt.polys.filter(|&v| v > 0).unwrap_or(p.polys),
        texture: opt.texture.filter(|&v| v > 0).unwrap_or(p.texture),
        origin: opt.origin.unwrap_or_else(|| p.origin.into()),
        crop: opt.crop,
        rotate: opt.rotate.unwrap_or(0.0),
        pbr: opt.pbr.unwrap_or(true),
        pose: opt.pose.filter(|s| !s.is_empty()),
        seed: opt.seed.unwrap_or((now() as i64) % 100000),
        keep_background: opt.keep_background.unwrap_or(false),
        delight: opt.delight.unwrap_or(true),
        keep_parts: opt.keep_parts.unwrap_or(false),
        status: "queued".into(),
        stage: "queued".into(),
        created: now(),
        results: vec![],
        prep: None,
        error: None,
        by: opt.by,
        extra: BTreeMap::new(),
    };
    save(&job)?;
    Ok(job)
}

impl Job {
    pub fn dir(&self) -> PathBuf {
        config().jobs.join(&self.id)
    }

    /// Append a timestamped line to job.log and make it the job's current stage.
    pub fn log(&mut self, msg: &str) {
        let line = format!("{} {msg}", chrono::Local::now().format("%H:%M:%S"));
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.dir().join("job.log")) {
            let _ = writeln!(f, "{line}");
        }
        self.stage = msg.to_string();
        let _ = self.save();
    }

    /// Extra detail for the log only (tracebacks), without touching the stage.
    pub fn log_raw(&self, text: &str) {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.dir().join("job.log")) {
            let _ = writeln!(f, "{text}");
        }
    }

    pub fn save(&self) -> Result<()> {
        save(self)
    }

    /// The last `n` lines of job.log.
    pub fn log_tail(&self, n: usize) -> Vec<String> {
        let text = std::fs::read_to_string(self.dir().join("job.log")).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        lines[lines.len().saturating_sub(n)..].iter().map(|s| s.to_string()).collect()
    }

    pub fn busy(&self) -> bool {
        self.status == "queued" || self.status == "running"
    }

    pub fn slug(&self) -> String {
        slug(&self.name)
    }
}
