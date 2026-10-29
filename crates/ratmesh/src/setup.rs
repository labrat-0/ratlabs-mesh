//! `ratmesh setup ...` installs optional parts; `ratmesh doctor` says what's missing and how to fix it.
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Result, bail};
use clap::Subcommand;
use mesh_core::config;

/// The local-engine setup scripts, shipped inside the binary.
#[derive(rust_embed::Embed)]
#[folder = "../../local"]
struct Local;

#[derive(Subcommand)]
pub enum SetupCmd {
    /// Hunyuan3D-2 on this CPU: free, offline (~22 GB download, ~16 GB RAM while generating)
    Hunyuan {
        /// Install folder (default: the configured one)
        dir: Option<PathBuf>,
    },
    /// The 88 CC0 Quaternius animations used by Rig + animate (also fetched on the first rig)
    Animations,
    /// Download the BiRefNet cut-out model now instead of on first use (~970 MB)
    Cutout,
    /// Add the starter pack to your library: 30 free CC0 props, finished (~80 MB)
    Starter,
}

fn extract() -> Result<PathBuf> {
    let d = config().runtime.join("local");
    for f in Local::iter() {
        let data = Local::get(&f).unwrap();
        mesh_core::config::materialize(&d.join(f.as_ref()), &data.data)?;
    }
    #[cfg(unix)]
    for f in ["setup_hunyuan.sh", "get_animations.sh"] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.join(f), std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(d)
}

fn need(tool: &str, hint: &str) -> Result<()> {
    if which(tool).is_none() {
        bail!("{tool} is needed for this step: {hint}");
    }
    Ok(())
}

pub fn which(tool: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(tool)).find(|p| p.is_file()))
}

pub fn run(cmd: SetupCmd) -> Result<()> {
    match cmd {
        SetupCmd::Hunyuan { dir } => {
            need("uv", "https://docs.astral.sh/uv/")?;
            need("git", "install git")?;
            let d = extract()?;
            let target = dir.unwrap_or_else(|| config().hunyuan.clone());
            println!("Installing Hunyuan3D-2 into {}", target.display());
            let st = Command::new("bash").arg(d.join("setup_hunyuan.sh")).arg(&target).status()?;
            if !st.success() {
                bail!("Hunyuan3D-2 setup failed");
            }
            if target != config().hunyuan {
                println!("Add to {}: hunyuan = {:?}", config().config_dir.join("config.toml").display(), target);
            }
            Ok(())
        }
        SetupCmd::Animations => {
            mesh_pipeline::ensure_animations(&mut |m| println!("{m}"))?;
            println!("animation libraries in {}", config().anim.display());
            Ok(())
        }
        SetupCmd::Starter => {
            let r = mesh_market::starter::load(&mut |m| println!("{m}"))?;
            println!("{} props added to {}", r["added"], config().jobs.display());
            Ok(())
        }
        SetupCmd::Cutout => {
            #[cfg(feature = "cutout")]
            {
                let p = mesh_prep::cutout::ensure_model(&|m| println!("{m}"))?;
                println!("cut-out model ready: {}", p.display());
                Ok(())
            }
            #[cfg(not(feature = "cutout"))]
            bail!("this build has no cut-out support")
        }
    }
}

fn line(ok: bool, what: &str, detail: &str) {
    println!("{} {what:22} {detail}", if ok { "✓" } else { "✗" });
}

pub fn doctor() -> Result<()> {
    let c = config();
    println!("RatLabs Mesh {}\n", env!("CARGO_PKG_VERSION"));
    match mesh_blender::version() {
        Some(v) => line(true, "Blender", &v),
        None => line(
            false,
            "Blender",
            &format!(
                "not found as {:?}. Install it (Arch: pacman -S blender, Debian/Ubuntu: apt install blender, or blender.org) or set RATMESH_BLENDER",
                c.blender
            ),
        ),
    }
    #[cfg(feature = "cutout")]
    {
        let ok = mesh_prep::cutout::model_ready();
        line(
            ok,
            "Cut-out model",
            &if ok { mesh_prep::cutout::model_path().display().to_string() } else { "downloads on first use (~970 MB), or run `ratmesh setup cutout`".into() },
        );
    }
    #[cfg(not(feature = "cutout"))]
    line(false, "Cut-out", "built without it: art must already be isolated");
    let anim = c.anim_libs().iter().all(|p| p.exists());
    line(
        anim,
        "Animations (rigging)",
        &if anim { c.anim.display().to_string() } else { "download on the first rig (25 MB), or run `ratmesh setup animations`".into() },
    );

    println!("\nEngines");
    let mut any = false;
    for (id, e) in mesh_engines::available().as_object().unwrap() {
        let ready = e["ready"] == true;
        any |= ready;
        let detail = if ready { e["cost"].as_str().unwrap_or("").to_string() } else { e["setup"].as_str().unwrap_or("not set up").to_string() };
        line(ready, id, &detail);
    }
    if !any {
        println!("  No engine is ready yet. The quickest start: add a fal.ai key with `ratmesh serve` > Keys.");
    }

    println!("\nFolders");
    line(true, "Jobs", &c.jobs.display().to_string());
    line(true, "Market library", &c.library.display().to_string());
    line(true, "Keys", &c.keys_file.display().to_string());
    line(true, "Engine plugins", &format!("{} ({} found)", c.plugins.display(), mesh_engines::plugin::load_user().len()));
    #[cfg(unix)]
    if let Ok(m) = std::fs::metadata(&c.keys_file) {
        use std::os::unix::fs::PermissionsExt;
        let private = m.permissions().mode() & 0o077 == 0;
        line(private, "Keys file is private", if private { "mode 600" } else { "others can read it: chmod 600 the keys file" });
    }

    let studio = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(1))
        .build()?
        .get(format!("{}api/config", c.url()))
        .send()
        .is_ok_and(|r| r.status().is_success());
    println!("\nStudio");
    line(studio, "Running", &if studio { format!("{}  (MCP: {}mcp)", c.url(), c.url()) } else { "not running: `ratmesh serve`".into() });
    Ok(())
}
