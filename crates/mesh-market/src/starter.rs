//! The starter pack: 30 CC0 Poly Haven props and the rigged Labrat, already finished by
//! RatLabs Mesh, so a new library isn't empty. Downloaded once, on request, from the project's GitHub release
//! (RATMESH_STARTER can point at another URL or a local zip). Built by tools/build-starter-pack.py.
use std::io::Read;
use std::path::Path;

use anyhow::{Result, bail};
use mesh_core::{Event, config, emit, jobs};
use serde_json::{Value, json};

use crate::merr;

pub const DEFAULT_URL: &str = "https://github.com/labrat-0/ratlabs-mesh/releases/download/starter-pack-v1/starter-pack.zip";
pub const ABOUT: &str = "30 free CC0 props from Poly Haven plus the rigged Labrat from Rat Underground, all finished by RatLabs Mesh (~85 MB, once)";

fn source() -> String {
    std::env::var("RATMESH_STARTER").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_URL.into())
}

/// How many starter items are in the library.
pub fn loaded() -> usize {
    std::fs::read_dir(&config().jobs).map(|rd| rd.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("starter-")).count()).unwrap_or(0)
}

/// Download (or open) the pack and add its items to the library, skipping any already there.
/// Returns {added, skipped, items}.
pub fn load(log: &mut dyn FnMut(&str)) -> Result<Value> {
    let src = source();
    let zpath = if src.starts_with("http://") || src.starts_with("https://") {
        let p = config().cache_dir.join("starter-pack.zip");
        if !p.exists() {
            log(&format!("downloading the starter pack: {ABOUT}"));
            std::fs::create_dir_all(p.parent().unwrap())?;
            let resp = crate::client(1800).get(&src).send()?;
            if !resp.status().is_success() {
                bail!(merr(format!("starter pack download failed ({}): {src}", resp.status().as_u16())));
            }
            let bytes = resp.bytes()?;
            let tmp = p.with_extension("part");
            std::fs::write(&tmp, &bytes)?;
            std::fs::rename(&tmp, &p)?;
        }
        p
    } else {
        mesh_core::config::expand(&src)
    };
    let mut z = zip::ZipArchive::new(std::fs::File::open(&zpath).map_err(|e| merr(format!("can't open the starter pack {}: {e}", zpath.display())))?)?;
    let mut pack: Value = json!({});
    if let Ok(mut f) = z.by_name("pack.json") {
        let mut s = String::new();
        f.read_to_string(&mut s)?;
        pack = serde_json::from_str(&s)?;
    }
    let jobs_root = &config().jobs;
    std::fs::create_dir_all(jobs_root)?;
    // Which items does the pack hold, and which are already in the library?
    let item_of = |rel: &Path| -> Option<String> {
        let rest = rel.strip_prefix("jobs").ok()?;
        let id = rest.iter().next()?.to_str()?.to_string();
        (id.starts_with("starter-") && jobs::job_dir(&id).is_ok()).then_some(id)
    };
    let mut all = std::collections::BTreeSet::new();
    for i in 0..z.len() {
        if let Some(id) = z.by_index(i)?.enclosed_name().as_deref().and_then(item_of) {
            all.insert(id);
        }
    }
    let new: std::collections::BTreeSet<String> = all.iter().filter(|id| !jobs_root.join(id).join("job.json").exists()).cloned().collect();
    for i in 0..z.len() {
        let mut f = z.by_index(i)?;
        let Some(rel) = f.enclosed_name() else { continue }; // zip-slip guard
        let Some(id) = item_of(&rel) else { continue };
        if !new.contains(&id) || f.is_dir() {
            continue;
        }
        let target = jobs_root.join(rel.strip_prefix("jobs").unwrap());
        std::fs::create_dir_all(target.parent().unwrap())?;
        // job.json goes in last, so a half-copied item never shows up in the library
        let out = if target.file_name().is_some_and(|n| n == "job.json") { target.with_extension("json.part") } else { target };
        std::io::copy(&mut f, &mut std::fs::File::create(&out)?)?;
    }
    for id in &new {
        let d = jobs_root.join(id);
        std::fs::rename(d.join("job.json.part"), d.join("job.json"))?;
        if let Ok(job) = jobs::load(id) {
            emit(Event::Job { job: Box::new(job) });
        }
    }
    let (added, skipped) = (new.len(), all.len() - new.len());
    log(&format!("starter pack: {added} added, {skipped} already in the library"));
    Ok(json!({"added": added, "skipped": skipped, "items": pack["items"], "note": pack["note"]}))
}

/// Whether `dir` is a starter-pack job (they can't be regenerated, only refinished).
pub fn is_starter(dir: &Path) -> bool {
    dir.file_name().is_some_and(|n| n.to_string_lossy().starts_with("starter-"))
}
