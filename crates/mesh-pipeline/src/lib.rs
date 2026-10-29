//! The pipeline: prep the art, generate with an engine, finish in Blender, and the
//! optional rig. Plus one work queue so the studio, the CLI and AI clients share a
//! single worker and never fight over the CPU.
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

use anyhow::{Result, anyhow};
use mesh_core::{Job, JobResult, jobs};
use mesh_engines::GenOptions;
use serde::Deserialize;
use serde_json::json;

/// Re-finish settings; unset fields keep the job's current value.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Refinish {
    pub height: Option<f64>,
    pub polys: Option<u32>,
    pub texture: Option<u32>,
    pub rotate: Option<f64>,
    pub origin: Option<String>,
}

fn fail(job: &Arc<Mutex<Job>>, e: anyhow::Error) {
    let mut j = job.lock().unwrap();
    j.status = "failed".into();
    j.error = Some(format!("{e:#}"));
    let msg = format!("failed: {e:#}");
    j.log(&msg);
    j.log_raw(&format!("{e:?}"));
}

fn finish_variant(job: &Arc<Mutex<Job>>, i: u32, raw: PathBuf, meta: serde_json::Value) -> Result<()> {
    let (snapshot, d, name) = {
        let mut j = job.lock().unwrap();
        let size = if j.height > 0.0 { format!("{} m", j.height) } else { "own scale".into() };
        let msg = format!("v{i} finish: clean, retopo to {} tris, {size}, renders", j.polys);
        j.log(&msg);
        (j.clone(), j.dir(), j.slug())
    };
    let vd = d.join(format!("v{i}"));
    std::fs::create_dir_all(&vd)?;
    let stats = mesh_blender::finish(&raw, &vd.join(format!("{name}.glb")), &snapshot, Some(&vd.join("renders")))?;
    let mut j = job.lock().unwrap();
    j.results.push(JobResult {
        variant: i,
        glb: format!("v{i}/{name}.glb"),
        raw: raw.strip_prefix(&d).unwrap_or(&raw).to_string_lossy().into_owned(),
        renders: format!("v{i}/renders"),
        meta,
        stats,
        rigged: None,
        rig: None,
    });
    j.results.sort_by_key(|r| r.variant);
    j.save()
}

/// Generate (or import) and finish every variant of a job.
pub fn run(id: &str) -> Result<Job> {
    let mut job = jobs::load(id)?;
    job.status = "running".into();
    job.error = None;
    job.results.clear();
    job.save()?;
    let job = Arc::new(Mutex::new(job));
    if let Err(e) = run_inner(&job) {
        fail(&job, e);
    } else {
        let mut j = job.lock().unwrap();
        j.status = "done".into();
        j.log("done");
    }
    let j = job.lock().unwrap().clone();
    j.save()?;
    Ok(j)
}

fn run_inner(job: &Arc<Mutex<Job>>) -> Result<()> {
    let snap = job.lock().unwrap().clone();
    let d = snap.dir();
    if snap.extra.contains_key("pack") {
        return Err(anyhow!("starter pack models can't be generated again (they have no source art); refinish and rig work"));
    }
    if snap.engine == "import" {
        return finish_variant(job, 1, d.join("import.glb"), json!({"engine": "import"}));
    }
    let engine = mesh_engines::get(&snap.engine).ok_or_else(|| anyhow!("unknown engine {}", snap.engine))?;
    let info = engine.info();
    if !engine.ready(&mesh_core::keys::load()) {
        let how = info.setup.clone().unwrap_or_else(|| format!("{} is not set up", info.label));
        return Err(mesh_engines::EngineError(match &info.key {
            Some(k) if mesh_core::keys::get(k).is_none() => {
                format!("{k} is not set. Add it with Keys in the studio or in {}", mesh_core::config().keys_file.display())
            }
            _ => how,
        })
        .into());
    }
    job.lock().unwrap().log("prep: cutting out the subject");
    let src = std::fs::read_dir(&d)?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.file_stem().is_some_and(|s| s == "source"))
        .ok_or_else(|| anyhow!("source image missing"))?;
    let prep = mesh_prep::prepare(
        &src,
        &d.join("prep.png"),
        mesh_prep::PrepOptions { crop: snap.crop, keep_background: snap.keep_background },
        Some(&d.join("prep_hi.png")),
    )?;
    {
        let mut j = job.lock().unwrap();
        j.prep = Some(serde_json::to_value(&prep)?);
        for note in &prep.notes {
            j.log(&format!("prep note: {note}"));
        }
    }
    let mut views = BTreeMap::new();
    let mut view_files: Vec<PathBuf> = std::fs::read_dir(d.join("views")).into_iter().flatten().flatten().map(|e| e.path()).collect();
    view_files.sort();
    for v in view_files {
        let name = v.file_name().unwrap().to_string_lossy().into_owned();
        if name.ends_with(".prep.png") {
            continue;
        }
        let side = v.file_stem().unwrap().to_string_lossy().into_owned();
        let out = d.join("views").join(format!("{side}.prep.png"));
        mesh_prep::prepare(&v, &out, Default::default(), None)?;
        views.insert(side, out);
    }
    let opt = GenOptions {
        polys: snap.polys,
        texture: snap.texture,
        quality: snap.quality.clone(),
        pbr: snap.pbr,
        pose: snap.pose.clone(),
        seed: snap.seed,
        delight: snap.delight,
        views,
        detail_source: Some(d.join("prep_hi.png")),
    };

    job.lock().unwrap().log(&format!("generate: {} x{}", snap.engine, snap.variants));
    // Local engines saturate the CPU and need GBs of RAM each: one variant at a time.
    let parallel = !info.local;
    let gen_one = |i: u32| -> Result<(u32, PathBuf, serde_json::Value)> {
        let vd = d.join(format!("v{i}"));
        std::fs::create_dir_all(&vd)?;
        let mut o = opt.clone();
        o.seed = snap.seed + i as i64 - 1;
        let log = |m: &str| job.lock().unwrap().log(&format!("v{i} {m}"));
        let meta = engine.run(&d.join("prep.png"), &vd.join("raw.glb"), &o, &log)?;
        Ok((i, vd.join("raw.glb"), meta))
    };
    let raws: Vec<Result<(u32, PathBuf, serde_json::Value)>> = if parallel {
        std::thread::scope(|s| {
            let hs: Vec<_> = (1..=snap.variants).map(|i| s.spawn(move || gen_one(i))).collect();
            hs.into_iter().map(|h| h.join().unwrap_or_else(|_| Err(anyhow!("engine thread panicked")))).collect()
        })
    } else {
        (1..=snap.variants).map(gen_one).collect()
    };
    for r in raws {
        let (i, raw, meta) = r?;
        finish_variant(job, i, raw, meta)?;
    }
    Ok(())
}

/// Auto-rig each finished variant and retarget the CC0 animation libraries onto it.
pub fn rig(id: &str) -> Result<Job> {
    let mut job = jobs::load(id)?;
    job.status = "running".into();
    job.error = None;
    job.save()?;
    if let Err(e) = ensure_animations(&mut |m| job.log(m)) {
        // The finished model is fine; only the rig couldn't start.
        job.status = "done".into();
        job.log(&format!("rig skipped: {e:#}"));
        job.save()?;
        return Err(e);
    }
    let d = job.dir();
    let n_libs = mesh_core::config().anim_libs().len();
    let mut result = Ok(());
    for k in 0..job.results.len() {
        let (v, glb, renders) = (job.results[k].variant, d.join(&job.results[k].glb), d.join(&job.results[k].renders));
        job.log(&format!("v{v} rig: skeleton, skin, {n_libs} animation libraries"));
        match mesh_blender::rig(&glb, &renders) {
            Ok((out, st)) => {
                let r = &mut job.results[k];
                r.rigged = Some(out.strip_prefix(&d).unwrap_or(&out).to_string_lossy().into_owned());
                r.rig = Some(json!({"clips": st["clips"], "bones": st["bones"], "warnings": st["warnings"]}));
                for w in st["warnings"].as_array().into_iter().flatten() {
                    job.log(&format!("v{v} rig warning: {}", w.as_str().unwrap_or("")));
                }
            }
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    match result {
        Ok(()) => {
            job.status = "done".into();
            job.log("done");
        }
        Err(e) => {
            job.status = "failed".into();
            job.error = Some(format!("{e:#}"));
            job.log(&format!("failed: {e:#}"));
        }
    }
    job.save()?;
    Ok(job)
}

const ANIM_URL: &str = "https://opengameart.org/sites/default/files";
const ANIM_ZIPS: &[&str] = &["universal_animation_librarystandard", "universal_animation_library_2standard"];

/// The Quaternius Universal Animation Libraries (CC0, ~25 MB): downloaded on the first rig,
/// so rigging works without a setup step.
pub fn ensure_animations(log: &mut dyn FnMut(&str)) -> Result<()> {
    let c = mesh_core::config();
    if c.anim_libs().iter().all(|p| p.exists()) {
        return Ok(());
    }
    log("downloading the CC0 animation library (25 MB, once)");
    std::fs::create_dir_all(&c.anim)?;
    let http = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(600)).build()?;
    for z in ANIM_ZIPS {
        let dest = c.anim.join(z);
        if dest.exists() {
            continue;
        }
        let bytes = http.get(format!("{ANIM_URL}/{z}.zip")).send()?.error_for_status()?.bytes()?;
        let tmp = c.anim.join(format!("{z}.part"));
        std::fs::remove_dir_all(&tmp).ok();
        zip::ZipArchive::new(std::io::Cursor::new(bytes))?.extract(&tmp)?;
        std::fs::rename(&tmp, &dest)?;
    }
    match c.anim_libs().into_iter().find(|p| !p.exists()) {
        Some(m) => Err(anyhow!("the animation download didn't contain {}", m.display())),
        None => Ok(()),
    }
}

/// Re-run only the finishing stage with new size/budget/orientation. No GPU cost.
pub fn refinish(id: &str, ch: &Refinish) -> Result<Job> {
    let mut job = jobs::load(id)?;
    if let Some(v) = ch.height {
        job.height = v;
    }
    if let Some(v) = ch.polys {
        job.polys = v;
    }
    if let Some(v) = ch.texture {
        job.texture = v;
    }
    if let Some(v) = ch.rotate {
        job.rotate = v;
    }
    if let Some(v) = &ch.origin {
        job.origin = v.clone();
    }
    job.status = "running".into();
    job.error = None;
    job.save()?;
    let d = job.dir();
    let mut result = Ok(());
    for k in 0..job.results.len() {
        let v = job.results[k].variant;
        job.log(&format!("v{v} refinish"));
        let (raw, glb, renders) = (d.join(&job.results[k].raw), d.join(&job.results[k].glb), d.join(&job.results[k].renders));
        match mesh_blender::finish(&raw, &glb, &job, Some(&renders)) {
            Ok(stats) => job.results[k].stats = stats,
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    match result {
        Ok(()) => {
            job.status = "done".into();
            job.log("done");
        }
        Err(e) => {
            job.status = "failed".into();
            job.error = Some(format!("{e:#}"));
            job.log(&format!("failed: {e:#}"));
        }
    }
    job.save()?;
    Ok(job)
}

// ---------- the queue ----------

#[derive(Debug, Clone)]
pub enum Work {
    Run(String),
    Rig(String),
    Refinish(String, Refinish),
}

struct Queue {
    items: Mutex<VecDeque<Work>>,
    ready: Condvar,
}

fn queue() -> &'static Queue {
    static Q: std::sync::OnceLock<Queue> = std::sync::OnceLock::new();
    Q.get_or_init(|| {
        std::thread::Builder::new().name("ratmesh-worker".into()).spawn(worker).expect("start worker");
        Queue { items: Mutex::new(VecDeque::new()), ready: Condvar::new() }
    })
}

fn worker() {
    loop {
        let w = {
            let q = queue();
            let mut items = q.items.lock().unwrap();
            loop {
                if let Some(w) = items.pop_front() {
                    break w;
                }
                items = q.ready.wait(items).unwrap();
            }
        };
        let r = match &w {
            Work::Run(id) => run(id),
            Work::Rig(id) => rig(id),
            Work::Refinish(id, ch) => refinish(id, ch),
        };
        if let Err(e) = r {
            // Errors before the job could record them (missing animation libs, bad id).
            let id = match &w {
                Work::Run(id) | Work::Rig(id) | Work::Refinish(id, _) => id,
            };
            if let Ok(mut j) = jobs::load(id).map_err(|_| ()).and_then(|j| if j.busy() { Ok(j) } else { Err(()) }) {
                j.status = "failed".into();
                j.error = Some(format!("{e:#}"));
                j.log(&format!("failed: {e:#}"));
            }
        }
    }
}

/// Queue work for the background worker; marks the job queued so every view shows it.
pub fn submit(w: Work) -> Result<()> {
    let id = match &w {
        Work::Run(id) | Work::Rig(id) | Work::Refinish(id, _) => id.clone(),
    };
    let mut j = jobs::load(&id)?;
    j.status = "queued".into();
    j.stage = "queued".into();
    j.save()?;
    let q = queue();
    q.items.lock().unwrap().push_back(w);
    q.ready.notify_one();
    Ok(())
}

pub fn queued() -> usize {
    queue().items.lock().unwrap().len()
}
