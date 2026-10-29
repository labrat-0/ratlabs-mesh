//! Files from AI clients: a local path, an http(s) URL, or a data: URI.
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use base64::Engine as _;

pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp"];

/// A scratch folder for downloaded inputs; jobs copy what they keep.
fn inbox() -> PathBuf {
    let d = mesh_core::config().cache_dir.join("inbox");
    std::fs::create_dir_all(&d).ok();
    d
}

fn ext_ok(p: &Path, exts: &[&str]) -> bool {
    p.extension().is_some_and(|e| exts.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Resolve `s` to a readable local file with one of `exts`.
pub fn resolve(s: &str, exts: &[&str]) -> Result<PathBuf> {
    let s = s.trim();
    let stamp = format!("{}-{}", std::process::id(), mesh_core::now() as u64 % 1_000_000_000);
    if let Some(rest) = s.strip_prefix("data:") {
        let (head, data) = rest.split_once(',').ok_or_else(|| anyhow::anyhow!("bad data: URI"))?;
        let ext = match head.split(';').next().unwrap_or("") {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            "model/gltf-binary" | "application/octet-stream" => "glb",
            other => bail!("unsupported data: URI type {other}"),
        };
        if !exts.contains(&ext) {
            bail!("expected one of {exts:?}");
        }
        let p = inbox().join(format!("{stamp}.{ext}"));
        std::fs::write(&p, base64::engine::general_purpose::STANDARD.decode(data.trim())?)?;
        return Ok(p);
    }
    if s.starts_with("http://") || s.starts_with("https://") {
        let name = s.split('?').next().unwrap_or(s).rsplit('/').next().unwrap_or("download");
        let mut p = inbox().join(format!("{stamp}-{name}"));
        if !ext_ok(&p, exts) {
            p = p.with_extension(exts[0]);
        }
        let bytes = reqwest::blocking::get(s)?.error_for_status()?.bytes()?;
        std::fs::write(&p, bytes)?;
        return Ok(p);
    }
    let p = mesh_core::config::expand(s);
    if !p.is_file() {
        bail!("no such file: {}", p.display());
    }
    if !ext_ok(&p, exts) {
        bail!("{} is not one of {exts:?}", p.display());
    }
    Ok(p)
}

pub fn b64_file(p: &Path) -> Result<String> {
    Ok(base64::engine::general_purpose::STANDARD.encode(std::fs::read(p)?))
}
