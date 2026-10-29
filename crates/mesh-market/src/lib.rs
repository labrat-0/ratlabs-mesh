//! Market: search free asset sites from one place and grab assets into a local library.
//!
//! Sources (all have real public APIs; nothing is scraped):
//!   polyhaven   CC0 models (glTF), textures, HDRIs        no key
//!   ambientcg   CC0 PBR materials, HDRIs                  no key
//!   sketchfab   downloadable models, licence per model    search free, download needs SKETCHFAB_TOKEN
//!   polypizza   low-poly models (Quaternius, Kenney...)   POLYPIZZA_KEY (free account)
//!
//! Grabbed assets land in library/<source>/<id>/ with meta.json recording name, author,
//! licence, source page and files, so attribution is never lost.
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use mesh_core::{Job, NewJob, config};
use serde_json::{Value, json};

pub mod matpack;
mod sources;
pub mod starter;

pub use sources::SOURCES;

pub const KINDS: &[&str] = &["model", "material", "hdri"];
const UA: &str = "RatLabs-Mesh/0.2 (asset browser; https://github.com/labrat-0/ratlabs-mesh)";

/// A user-facing Market failure (missing key, bad id, upstream error).
#[derive(Debug)]
pub struct MarketError(pub String);
impl std::fmt::Display for MarketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for MarketError {}

pub(crate) fn merr(msg: impl Into<String>) -> anyhow::Error {
    MarketError(msg.into()).into()
}

pub fn lib_dir() -> &'static Path {
    &config().library
}

fn cache_dir() -> PathBuf {
    config().cache_dir.join("market")
}

pub(crate) fn client(timeout: u64) -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder().user_agent(UA).timeout(Duration::from_secs(timeout)).build().unwrap()
}

pub(crate) fn get(url: &str, query: &[(&str, String)], headers: &[(&str, String)]) -> Result<Value> {
    let mut r = client(60).get(url).query(query);
    for (k, v) in headers {
        r = r.header(*k, v);
    }
    let resp = r.send()?;
    let status = resp.status();
    let text = resp.text()?;
    if !status.is_success() {
        bail!(merr(format!("{}: {} {}", url.split('?').next().unwrap_or(url), status.as_u16(), &text[..text.len().min(200)])));
    }
    Ok(serde_json::from_str(&text)?)
}

pub(crate) fn download(url: &str, dest: &Path, headers: &[(&str, String)]) -> Result<PathBuf> {
    std::fs::create_dir_all(dest.parent().unwrap())?;
    let mut r = client(600).get(url);
    for (k, v) in headers {
        r = r.header(*k, v);
    }
    let mut resp = r.send()?;
    if !resp.status().is_success() {
        bail!(merr(format!("download {}: {url}", resp.status().as_u16())));
    }
    let tmp = dest.with_extension(format!("{}.part", dest.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()));
    let mut f = std::fs::File::create(&tmp)?;
    std::io::copy(&mut resp, &mut f)?;
    std::fs::rename(&tmp, dest)?;
    Ok(dest.to_path_buf())
}

/// Extract a zip into `into`, skipping entries that would escape it.
pub(crate) fn unzip(zpath: &Path, into: &Path) -> Result<()> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(zpath)?)?;
    for i in 0..z.len() {
        let mut f = z.by_index(i)?;
        let Some(rel) = f.enclosed_name() else { continue }; // zip-slip guard
        let target = into.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            std::fs::create_dir_all(target.parent().unwrap())?;
            std::io::copy(&mut f, &mut std::fs::File::create(&target)?)?;
        }
    }
    std::fs::remove_file(zpath)?;
    Ok(())
}

/// A day-old-at-most JSON cache file under the market cache folder.
pub(crate) fn cached(name: &str, fetch: impl FnOnce() -> Result<Value>) -> Result<Value> {
    let f = cache_dir().join(name);
    let fresh = std::fs::metadata(&f).ok().and_then(|m| m.modified().ok()).is_some_and(|t| t.elapsed().map(|e| e.as_secs() < 86400).unwrap_or(false));
    if fresh && let Ok(v) = serde_json::from_slice(&std::fs::read(&f)?) {
        return Ok(v);
    }
    let v = fetch()?;
    std::fs::create_dir_all(f.parent().unwrap())?;
    std::fs::write(&f, v.to_string())?;
    Ok(v)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn item(source: &str, id: &str, name: &str, kind: &str, thumb: &str, licence: &str, author: &str, page: &str, extra: Value) -> Value {
    let mut v = json!({"source": source, "id": id, "name": name, "kind": kind, "thumb": thumb, "licence": licence, "author": author, "page": page});
    if let Value::Object(m) = extra {
        for (k, x) in m {
            v[k] = x;
        }
    }
    v
}

/// `{id: {label, kinds, licence, note, key, key_for, ready}}`.
pub fn sources() -> Value {
    let keys = mesh_core::keys::load();
    let mut m = serde_json::Map::new();
    for s in SOURCES {
        let ready = s.key.is_none_or(|k| keys.get(k).is_some_and(|v| !v.is_empty()));
        m.insert(
            s.id.into(),
            json!({"label": s.label, "kinds": s.kinds, "licence": s.licence, "note": s.note, "key": s.key, "key_for": s.key_for, "ready": ready}),
        );
    }
    Value::Object(m)
}

pub fn source(id: &str) -> Option<&'static sources::Source> {
    SOURCES.iter().find(|s| s.id == id)
}

/// `{items, total}`; each item has `have` when it's already in the library.
pub fn search(source_id: &str, q: &str, kind: &str, page: u32) -> Result<Value> {
    let s = source(source_id).ok_or_else(|| merr("unknown source"))?;
    if !s.kinds.contains(&kind) {
        return Ok(json!({"items": [], "total": 0}));
    }
    let (mut items, total) = (s.search)(q.trim(), kind, page)?;
    for it in &mut items {
        let have = lib_dir().join(source_id).join(it["id"].as_str().unwrap_or("")).join("meta.json").exists();
        it["have"] = have.into();
    }
    Ok(json!({"items": items, "total": total}))
}

fn safe_id(id: &str) -> Result<&str> {
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.starts_with('.') {
        bail!(merr("bad id"));
    }
    Ok(id)
}

/// Download a search result into the library. `item` needs source, id, kind (plus name,
/// licence, author, page, thumb to record). Returns the meta written.
pub fn grab(item: &Value, res: &str) -> Result<Value> {
    let source_id = item["source"].as_str().unwrap_or("");
    let s = source(source_id).ok_or_else(|| merr("bad item"))?;
    let id = safe_id(item["id"].as_str().unwrap_or(""))?;
    let kind = item["kind"].as_str().unwrap_or("");
    if !KINDS.contains(&kind) {
        bail!(merr("bad item"));
    }
    let d = lib_dir().join(source_id).join(id);
    let files = (s.grab)(id, kind, res, &d)?;
    let mut meta = json!({});
    for k in ["source", "id", "name", "kind", "licence", "author", "page", "thumb"] {
        meta[k] = item.get(k).cloned().unwrap_or(Value::Null);
    }
    if source_id == "sketchfab"
        && let Ok(info) = sources::sketchfab_info(id)
    {
        for (k, v) in info.as_object().unwrap() {
            meta[k] = v.clone();
        }
    }
    let rel = |f: &PathBuf| f.strip_prefix(&d).unwrap_or(f).to_string_lossy().into_owned();
    meta["res"] = res.into();
    meta["grabbed"] = (mesh_core::now() as i64).into();
    meta["files"] = files.iter().map(rel).collect::<Vec<_>>().into();
    meta["main"] = files.first().map(rel).into();
    std::fs::write(d.join("meta.json"), serde_json::to_string_pretty(&meta)?)?;
    mesh_core::emit(mesh_core::Event::Library);
    Ok(meta)
}

/// Everything grabbed, newest first, each with its folder as `path`.
pub fn library() -> Vec<Value> {
    let mut out: Vec<(std::time::SystemTime, Value)> = vec![];
    let Ok(sources) = std::fs::read_dir(lib_dir()) else { return vec![] };
    for s in sources.flatten() {
        let Ok(items) = std::fs::read_dir(s.path()) else { continue };
        for it in items.flatten() {
            let mp = it.path().join("meta.json");
            let Ok(text) = std::fs::read_to_string(&mp) else { continue };
            let Ok(mut meta) = serde_json::from_str::<Value>(&text) else { continue };
            meta["path"] = it.path().display().to_string().into();
            let t = std::fs::metadata(&mp).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            out.push((t, meta));
        }
    }
    out.sort_by_key(|x| std::cmp::Reverse(x.0));
    out.into_iter().map(|x| x.1).collect()
}

pub fn remove(source_id: &str, id: &str) -> Result<()> {
    if source(source_id).is_none() {
        bail!(merr("bad path"));
    }
    let d = lib_dir().join(source_id).join(safe_id(id)?);
    std::fs::remove_dir_all(d).ok();
    mesh_core::emit(mesh_core::Event::Library);
    Ok(())
}

/// Create a studio job that finishes a grabbed model (cleanup, budget, renders). Returns the job.
pub fn import_to_studio(source_id: &str, id: &str, kind: &str, height: f64, by: Option<String>) -> Result<Job> {
    let d = lib_dir().join(source_id).join(safe_id(id)?);
    let meta: Value = serde_json::from_slice(&std::fs::read(d.join("meta.json")).map_err(|_| merr("not in the library"))?)?;
    if meta["kind"] != "model" {
        bail!(merr("only models can be finished in the studio"));
    }
    let main = meta["main"].as_str().ok_or_else(|| anyhow!("no main file"))?;
    let glb = mesh_blender::to_glb(&d.join(main))?;
    mesh_core::jobs::create(
        None,
        Some(&glb),
        &Default::default(),
        NewJob {
            name: meta["name"].as_str().map(str::to_string),
            kind: Some(kind.into()),
            height: Some(height),
            keep_parts: Some(true),
            by,
            ..Default::default()
        },
    )
}
