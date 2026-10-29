//! Everything that needs Blender: finishing a raw generation (cleanup, retopo, sizing,
//! PBR, normal bake, review renders), auto-rigging with the CC0 animation libraries,
//! LODs, and packing a .gltf into a .glb.
//!
//! The scripts run inside Blender's own Python, so they stay Python; they are embedded in
//! this binary and written to the runtime folder before each run.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use mesh_core::{Job, config};
use serde_json::{Value, json};

const FINISH: &str = include_str!("../scripts/finish_blender.py");
const RIG: &str = include_str!("../scripts/rig_blender.py");
const LOD: &str = include_str!("../scripts/lod_blender.py");

fn script(name: &str, body: &str) -> Result<PathBuf> {
    let p = config().runtime.join("blender").join(name);
    mesh_core::config::materialize(&p, body.as_bytes())?;
    Ok(p)
}

/// Blender's version line, or None when it can't be run.
pub fn version() -> Option<String> {
    let out = Command::new(&config().blender).arg("--version").stderr(Stdio::null()).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().next().map(str::to_string)
}

/// Run `blender -b` with `args`, killing it after `timeout`. Returns (success, stdout+stderr).
fn run(args: &[&str], timeout: Duration) -> Result<(bool, String)> {
    let mut child = Command::new(&config().blender)
        .arg("-b")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("could not start Blender ({}): {e}. Install Blender or set RATMESH_BLENDER.", config().blender))?;
    let mut so = child.stdout.take().unwrap();
    let mut se = child.stderr.take().unwrap();
    let t_out = std::thread::spawn(move || {
        let mut s = String::new();
        so.read_to_string(&mut s).ok();
        s
    });
    let t_err = std::thread::spawn(move || {
        let mut s = String::new();
        se.read_to_string(&mut s).ok();
        s
    });
    let start = Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break Some(st);
        }
        if start.elapsed() > timeout {
            child.kill().ok();
            child.wait().ok();
            break None;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let text = t_out.join().unwrap_or_default() + &t_err.join().unwrap_or_default();
    match status {
        Some(st) => Ok((st.success(), text)),
        None => Ok((false, format!("{text}\nBlender timed out after {} s", timeout.as_secs()))),
    }
}

fn tail(s: &str, n: usize) -> &str {
    let mut i = s.len().saturating_sub(n);
    while !s.is_char_boundary(i) {
        i += 1;
    }
    &s[i..]
}

fn run_spec(script_name: &str, body: &str, spec: &Value, spec_path: &Path, timeout: Duration) -> Result<(bool, String)> {
    std::fs::write(spec_path, serde_json::to_vec(spec)?)?;
    let s = script(script_name, body)?;
    let r = run(&["--python-exit-code", "1", "--python", &s.to_string_lossy(), "--", &spec_path.to_string_lossy()], timeout);
    if matches!(r, Ok((true, _))) {
        std::fs::remove_file(spec_path).ok();
    }
    r
}

fn with_suffix(p: &Path, ext: &str) -> PathBuf {
    p.with_extension(ext)
}

/// Clean, retopo to the job's budget, size, texture, bake and render one variant.
/// Returns the stats Blender wrote (tris, dimensions, textures, warnings...).
pub fn finish(raw: &Path, out: &Path, job: &Job, renders: Option<&Path>) -> Result<Value> {
    let stats = with_suffix(out, "json");
    let mut spec = json!({
        "in": raw, "out": out, "height": job.height, "polys": job.polys, "texture": job.texture,
        "origin": job.origin, "rotate": job.rotate, "renders": renders, "stats": stats,
        "name": job.slug(), "keep_parts": job.keep_parts,
    });
    let hipoly = raw.parent().unwrap_or(Path::new(".")).join("work/hipoly.glb");
    if hipoly.exists() {
        spec["hipoly"] = json!(hipoly);
    } else if mesh_glb::tris(raw) > 3 * job.polys as u64 && !job.keep_parts {
        // Downloaded props carry authored normal maps and often several materials; keep those.
        // Remote engines return dense meshes; bake what decimation removes into a normal map.
        spec["hipoly"] = json!(raw);
    }
    let (ok, text) = run_spec("finish_blender.py", FINISH, &spec, &with_suffix(out, "spec.json"), Duration::from_secs(1800))?;
    if !ok || !out.exists() {
        bail!("Blender finishing failed:\n{}", tail(&text, 1500));
    }
    if let Some(r) = renders {
        mesh_glb::review_sheet(r).ok();
    }
    Ok(serde_json::from_slice(&std::fs::read(&stats)?)?)
}

/// Auto-rig one finished GLB and retarget the animation libraries onto it.
/// Returns (rigged GLB path, stats with clips/bones/warnings).
pub fn rig(glb: &Path, renders: &Path) -> Result<(PathBuf, Value)> {
    let libs = config().anim_libs();
    if let Some(missing) = libs.iter().find(|p| !p.exists()) {
        bail!("animation library missing ({}); run `ratmesh setup animations`", missing.display());
    }
    let out = glb.with_file_name(format!("{}-rigged.glb", glb.file_stem().unwrap().to_string_lossy()));
    let stats = with_suffix(&out, "json");
    let spec = json!({"in": glb, "out": out, "libs": libs, "stats": stats, "renders": renders.join("rig")});
    let (ok, text) = run_spec("rig_blender.py", RIG, &spec, &with_suffix(&out, "spec.json"), Duration::from_secs(3600))?;
    if !ok || !out.exists() {
        bail!("rigging failed: {}", tail(&text, 1500));
    }
    let st: Value = serde_json::from_slice(&std::fs::read(&stats)?)?;
    Ok((out, st))
}

/// A lighter copy of a GLB keeping names, hierarchy, skins and clips.
/// `spec` takes "target" (total tris) or "ratio", plus optional "min_tris" and "clips".
pub fn lod(input: &Path, out: &Path, mut spec: Value) -> Result<Value> {
    let stats = with_suffix(out, "json");
    spec["in"] = json!(input);
    spec["out"] = json!(out);
    spec["stats"] = json!(stats);
    let (ok, text) = run_spec("lod_blender.py", LOD, &spec, &with_suffix(out, "spec.json"), Duration::from_secs(1800))?;
    if !ok || !out.exists() {
        bail!("LOD failed: {}", tail(&text, 1500));
    }
    Ok(serde_json::from_slice(&std::fs::read(&stats)?)?)
}

/// Pack a .gltf with its external files into one .glb next to it.
pub fn to_glb(path: &Path) -> Result<PathBuf> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("glb")) {
        return Ok(path.to_path_buf());
    }
    let out = path.with_extension("glb");
    let lit = |p: &Path| serde_json::to_string(&p.to_string_lossy()).unwrap();
    let expr = format!(
        "import bpy;bpy.ops.wm.read_factory_settings(use_empty=True);bpy.ops.import_scene.gltf(filepath={});bpy.ops.export_scene.gltf(filepath={},export_format='GLB')",
        lit(path),
        lit(&out)
    );
    let (_, text) = run(&["--python-expr", &expr], Duration::from_secs(600)).context("packing glTF")?;
    if !out.exists() {
        bail!("could not pack glTF: {}", tail(&text, 400));
    }
    Ok(out)
}
