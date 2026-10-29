//! Engine plugins: drop a TOML manifest in ~/.config/ratlabs-mesh/engines/ and the engine
//! shows up in the studio, the CLI and every AI client. Two kinds:
//!
//! **command**: run any program that writes a GLB.
//!
//! ```toml
//! id = "my-engine"
//! label = "My engine (local GPU)"
//! cost = "free"
//! multiview = false
//! exclusive = true                       # one at a time per machine (heavy local models)
//! key = "MY_API_KEY"                     # optional; passed in the environment, required to be ready
//! ready = "{plugin}/venv/bin/python"     # optional; a file that must exist to be ready
//! command = ["{plugin}/venv/bin/python", "{plugin}/run.py", "{image}", "{out}", "--seed", "{seed}"]
//! view_args = ["--{side}", "{path}"]     # repeated for each extra view given
//! keep_lighting_args = ["--keep-lighting"]
//! detail_args = ["--detail-source", "{path}"]
//! progress = "my-engine {stage}"         # log line for each JSON line {"stage": ...} printed on stdout
//! [quality.draft]
//! args = ["--steps", "10"]
//! [meta]                                 # extra fields kept with the result
//! host = "local-gpu"
//! ```
//!
//! **fal**: call any image-to-3D model hosted on fal.ai, no code.
//!
//! ```toml
//! id = "sam3d"
//! type = "fal"
//! label = "SAM 3D Objects (fal.ai)"
//! endpoint = "fal-ai/sam-3/3d-objects"
//! result = "model_glb.url"               # dotted path to the GLB URL; alternatives with "|"
//! [payload]
//! image_url = "{image_url}"              # uploaded cut-out; also {back_image_url} etc.
//! seed = "{seed}"                        # a value that is exactly one placeholder keeps its type
//! ```
//!
//! Placeholders: {image} {out} {seed} {polys} {texture} {quality} {pbr} {plugin} (manifest folder)
//! {sidecars} (bundled helper scripts) {hunyuan} {triposr} {data} (configured folders).
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::fal::{Fal, err};
use crate::{Engine, EngineInfo, GenOptions, Log, download};

const HUNYUAN_LOCAL: &str = include_str!("../manifests/hunyuan-local.toml");
const TRIPOSR: &str = include_str!("../manifests/triposr.toml");
const SIDECARS: &[(&str, &str)] = &[
    ("hunyuan_local.py", include_str!("../sidecars/hunyuan_local.py")),
    ("detail.py", include_str!("../sidecars/detail.py")),
    ("triposr_cpu.py", include_str!("../sidecars/triposr_cpu.py")),
];

#[derive(Debug, Clone, Deserialize, Default)]
pub struct QualityArgs {
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub meta: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub id: String,
    #[serde(rename = "type", default = "command")]
    pub kind: String,
    pub label: String,
    #[serde(default)]
    pub cost: String,
    #[serde(default)]
    pub multiview: bool,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub setup: Option<String>,
    #[serde(default)]
    pub meta: BTreeMap<String, Value>,
    // command
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub ready: Option<String>,
    #[serde(default)]
    pub exclusive: bool,
    #[serde(default)]
    pub view_args: Vec<String>,
    #[serde(default)]
    pub keep_lighting_args: Vec<String>,
    #[serde(default)]
    pub detail_args: Vec<String>,
    #[serde(default)]
    pub progress: Option<String>,
    #[serde(default)]
    pub quality: BTreeMap<String, QualityArgs>,
    // fal
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub payload: Option<Value>,
    #[serde(skip)]
    pub source: String,
    #[serde(skip)]
    pub dir: PathBuf,
}

fn command() -> String {
    "command".into()
}

pub struct Plugin(pub Manifest);

/// The bundled local engines.
pub fn builtin() -> Vec<Arc<dyn Engine>> {
    [HUNYUAN_LOCAL, TRIPOSR]
        .iter()
        .map(|t| {
            let mut m: Manifest = toml::from_str(t).expect("bundled manifest");
            m.source = "builtin".into();
            m.dir = sidecars_dir();
            Arc::new(Plugin(m)) as Arc<dyn Engine>
        })
        .collect()
}

/// Manifests from the plugins folder, sorted by file name. Broken ones are skipped with a warning.
pub fn load_user() -> Vec<Arc<dyn Engine>> {
    let dir = &mesh_core::config().plugins;
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "toml")).collect())
        .unwrap_or_default();
    files.sort();
    files
        .into_iter()
        .filter_map(|p| match parse(&p) {
            Ok(m) => Some(Arc::new(Plugin(m)) as Arc<dyn Engine>),
            Err(e) => {
                eprintln!("ratmesh: skipping engine plugin {}: {e:#}", p.display());
                None
            }
        })
        .collect()
}

pub fn parse(path: &Path) -> Result<Manifest> {
    let mut m: Manifest = toml::from_str(&std::fs::read_to_string(path)?).with_context(|| format!("{}", path.display()))?;
    m.source = path.display().to_string();
    m.dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    match m.kind.as_str() {
        "command" if m.command.is_empty() => anyhow::bail!("a command engine needs `command`"),
        "fal" if m.endpoint.is_none() || m.payload.is_none() => anyhow::bail!("a fal engine needs `endpoint` and `[payload]`"),
        "command" | "fal" => Ok(m),
        other => anyhow::bail!("unknown engine type {other:?} (use \"command\" or \"fal\")"),
    }
}

pub fn sidecars_dir() -> PathBuf {
    mesh_core::config().runtime.join("sidecars")
}

fn write_sidecars() -> Result<()> {
    for (name, body) in SIDECARS {
        mesh_core::config::materialize(&sidecars_dir().join(name), body.as_bytes())?;
    }
    Ok(())
}

/// Replace {name} placeholders in `s`.
pub fn fill(s: &str, vars: &BTreeMap<&str, String>) -> String {
    let mut out = s.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

impl Plugin {
    fn vars(&self, opt: Option<&GenOptions>) -> BTreeMap<&'static str, String> {
        let c = mesh_core::config();
        let mut v = BTreeMap::from([
            ("plugin", self.0.dir.display().to_string()),
            ("sidecars", sidecars_dir().display().to_string()),
            ("hunyuan", c.hunyuan.display().to_string()),
            ("triposr", c.triposr.display().to_string()),
            ("data", c.data_dir.display().to_string()),
        ]);
        if let Some(o) = opt {
            v.insert("seed", o.seed.to_string());
            v.insert("polys", o.polys.to_string());
            v.insert("texture", o.texture.to_string());
            v.insert("quality", o.quality.clone());
            v.insert("pbr", o.pbr.to_string());
        }
        v
    }

    fn quality(&self, q: &str) -> QualityArgs {
        self.0.quality.get(q).or_else(|| self.0.quality.get("standard")).cloned().unwrap_or_default()
    }

    fn run_command(&self, img: &Path, out: &Path, opt: &GenOptions, log: Log) -> Result<Value> {
        let m = &self.0;
        write_sidecars()?;
        let mut vars = self.vars(Some(opt));
        vars.insert("image", img.display().to_string());
        vars.insert("out", out.display().to_string());
        let mut args: Vec<String> = m.command.iter().map(|a| fill(a, &vars)).collect();
        let q = self.quality(&opt.quality);
        args.extend(q.args.iter().map(|a| fill(a, &vars)));
        if m.multiview {
            for (side, path) in &opt.views {
                let mut vv = vars.clone();
                vv.insert("side", side.clone());
                vv.insert("path", path.display().to_string());
                args.extend(m.view_args.iter().map(|a| fill(a, &vv)));
            }
        }
        if !opt.delight {
            args.extend(m.keep_lighting_args.iter().cloned());
        }
        if let Some(d) = &opt.detail_source {
            let mut vv = vars.clone();
            vv.insert("path", d.display().to_string());
            args.extend(m.detail_args.iter().map(|a| fill(a, &vv)));
        }

        let mut cmd = Command::new(&args[0]);
        cmd.args(&args[1..]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(k) = &m.key {
            cmd.env(k, crate::fal::need(k)?);
        }
        // One heavy local job per machine: each can need ~16 GB, and two at once exhaust RAM.
        // A file lock covers studio jobs and CLI jobs alike.
        let _lock = if m.exclusive {
            let p = mesh_core::config().cache_dir.join("local-engine.lock");
            std::fs::create_dir_all(p.parent().unwrap())?;
            let f = std::fs::File::create(&p)?;
            if fs2::FileExt::try_lock_exclusive(&f).is_err() {
                log("waiting for another local job to finish");
                fs2::FileExt::lock_exclusive(&f)?;
            }
            Some(f)
        } else {
            None
        };
        let mut child = cmd.spawn().map_err(|e| err(format!("{} could not start {}: {e}", m.label, args[0])))?;
        let tail = Arc::new(Mutex::new(Vec::<String>::new()));
        let push = {
            let tail = tail.clone();
            move |line: String| {
                let mut t = tail.lock().unwrap();
                t.push(line);
                let n = t.len();
                if n > 25 {
                    t.drain(..n - 25);
                }
            }
        };
        let stderr = child.stderr.take().unwrap();
        let push_err = push.clone();
        let t_err = std::thread::spawn(move || {
            let mut s = String::new();
            let mut r = BufReader::new(stderr);
            while r.read_line(&mut s).unwrap_or(0) > 0 {
                push_err(s.trim_end().to_string());
                s.clear();
            }
        });
        let fmt = m.progress.clone().unwrap_or_else(|| format!("{} {{stage}}", m.id));
        for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
            if line.starts_with('{')
                && let Ok(st) = serde_json::from_str::<Value>(&line)
            {
                let mut pv = BTreeMap::new();
                pv.insert("stage", st["stage"].as_str().unwrap_or("").to_string());
                pv.insert("t", st["t"].as_f64().map(|t| format!("{t:.0}")).unwrap_or_default());
                log(fill(&fmt, &pv).replace(" (s)", "").trim());
                continue;
            }
            push(line);
        }
        let status = child.wait()?;
        t_err.join().ok();
        if !status.success() || !out.exists() {
            return Err(err(format!("{} failed:\n{}", m.label, tail.lock().unwrap().join("\n"))));
        }
        let mut meta = json!({"engine": m.id, "host": "local"});
        for (k, v) in m.meta.iter().chain(q.meta.iter()) {
            meta[k] = v.clone();
        }
        if m.multiview {
            meta["views"] = json!(opt.views.keys().collect::<Vec<_>>());
        }
        Ok(meta)
    }

    fn run_fal(&self, img: &Path, out: &Path, opt: &GenOptions, log: Log) -> Result<Value> {
        let m = &self.0;
        let fal = Fal::from_keys()?;
        let mut urls: BTreeMap<String, Option<String>> = BTreeMap::new();
        let text = m.payload.as_ref().unwrap().to_string();
        if text.contains("{image_url}") {
            urls.insert("image_url".into(), Some(fal.upload(img)?));
        }
        for side in ["back", "left", "right"] {
            let key = format!("{side}_image_url");
            if text.contains(&format!("{{{key}}}")) {
                urls.insert(key, opt.views.get(side).map(|p| fal.upload(p)).transpose()?);
            }
        }
        let vars = self.vars(Some(opt));
        let payload = substitute(m.payload.as_ref().unwrap(), &vars, &urls, opt);
        let r = fal.run(m.endpoint.as_ref().unwrap(), &payload, log, 4)?;
        let path = m.result.as_deref().unwrap_or("model_glb.url|model_mesh.url|model_urls.glb.url");
        let url =
            path.split('|').find_map(|p| dotted(&r, p.trim()).and_then(Value::as_str)).ok_or_else(|| err(format!("{}: no GLB at {path} in {r}", m.label)))?;
        download(url, out)?;
        let mut meta = json!({"engine": m.id, "host": "fal.ai", "seed": opt.seed});
        for (k, v) in &m.meta {
            meta[k] = v.clone();
        }
        Ok(meta)
    }
}

fn dotted<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |v, k| match k.parse::<usize>() {
        Ok(i) => v.get(i),
        Err(_) => v.get(k),
    })
}

/// Fill a fal payload. A string that is exactly one placeholder keeps the value's type;
/// keys whose view placeholder has no view are dropped.
fn substitute(v: &Value, vars: &BTreeMap<&str, String>, urls: &BTreeMap<String, Option<String>>, opt: &GenOptions) -> Value {
    match v {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, val) in map {
                if let Value::String(s) = val
                    && let Some(name) = s.strip_prefix('{').and_then(|s| s.strip_suffix('}'))
                {
                    if let Some(u) = urls.get(name) {
                        if let Some(u) = u {
                            out.insert(k.clone(), u.clone().into());
                        }
                        continue;
                    }
                    let typed = match name {
                        "seed" => Some(json!(opt.seed)),
                        "polys" => Some(json!(opt.polys)),
                        "texture" => Some(json!(opt.texture)),
                        "pbr" => Some(json!(opt.pbr)),
                        _ => None,
                    };
                    if let Some(t) = typed {
                        out.insert(k.clone(), t);
                        continue;
                    }
                }
                out.insert(k.clone(), substitute(val, vars, urls, opt));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| substitute(x, vars, urls, opt)).collect()),
        Value::String(s) => Value::String(fill(s, vars)),
        other => other.clone(),
    }
}

impl Engine for Plugin {
    fn id(&self) -> &str {
        &self.0.id
    }

    fn info(&self) -> EngineInfo {
        let m = &self.0;
        let key = if m.kind == "fal" { m.key.clone().or(Some("FAL_KEY".into())) } else { m.key.clone() };
        EngineInfo {
            label: m.label.clone(),
            key,
            cost: m.cost.clone(),
            multiview: m.multiview,
            local: m.kind == "command" && m.key.is_none(),
            source: m.source.clone(),
            setup: m.setup.clone(),
        }
    }

    fn ready(&self, keys: &BTreeMap<String, String>) -> bool {
        let m = &self.0;
        let key_ok = |k: &str| keys.get(k).is_some_and(|v| !v.is_empty());
        match m.kind.as_str() {
            "fal" => key_ok(m.key.as_deref().unwrap_or("FAL_KEY")),
            _ => m.key.as_deref().is_none_or(key_ok) && m.ready.as_ref().is_none_or(|r| Path::new(&fill(r, &self.vars(None))).exists()),
        }
    }

    fn run(&self, img: &Path, out: &Path, opt: &GenOptions, log: Log) -> Result<Value> {
        match self.0.kind.as_str() {
            "fal" => self.run_fal(img, out, opt, log),
            _ => self.run_command(img, out, opt, log),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_manifests_parse() {
        let b = builtin();
        assert_eq!(b.iter().map(|e| e.id().to_string()).collect::<Vec<_>>(), ["hunyuan-local", "triposr"]);
        assert!(b[0].info().multiview && b[0].info().local);
    }

    #[test]
    fn fal_payload_typing() {
        let opt = GenOptions { seed: 7, polys: 30000, texture: 2048, pbr: true, ..Default::default() };
        let payload =
            json!({"image_url": "{image_url}", "back_image_url": "{back_image_url}", "seed": "{seed}", "note": "s{seed}-{texture}", "opts": {"pbr": "{pbr}"}});
        let urls = BTreeMap::from([("image_url".to_string(), Some("https://cdn/a.png".to_string())), ("back_image_url".to_string(), None)]);
        let mut vars = BTreeMap::new();
        vars.insert("seed", "7".to_string());
        vars.insert("texture", "2048".to_string());
        let out = substitute(&payload, &vars, &urls, &opt);
        assert_eq!(out, json!({"image_url": "https://cdn/a.png", "seed": 7, "note": "s7-2048", "opts": {"pbr": true}}));
    }

    #[test]
    fn dotted_paths() {
        let v = json!({"a": {"b": [{"url": "x"}]}});
        assert_eq!(dotted(&v, "a.b.0.url"), Some(&json!("x")));
        assert_eq!(dotted(&v, "a.c"), None);
    }
}

#[cfg(test)]
mod example_plugins {
    use super::*;

    /// The examples shipped in plugins/ must stay valid manifests.
    #[test]
    fn examples_parse() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        let hf = Plugin(parse(&root.join("trellis2-hf/trellis2-hf.toml")).unwrap());
        assert_eq!(hf.id(), "trellis2-hf");
        assert_eq!(hf.info().key.as_deref(), Some("HF_TOKEN"));
        assert!(!hf.info().local);
        assert!(!hf.ready(&BTreeMap::new()));
        assert!(hf.ready(&BTreeMap::from([("HF_TOKEN".to_string(), "x".to_string())])));
        let fal = Plugin(parse(&root.join("fal-template/my-fal-model.toml")).unwrap());
        assert_eq!(fal.info().key.as_deref(), Some("FAL_KEY"));
    }
}
