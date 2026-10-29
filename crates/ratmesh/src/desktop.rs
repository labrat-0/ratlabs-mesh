//! `ratmesh install-desktop`: a background service plus an app-launcher entry (Linux).
//!   - systemd user service ratlabs-mesh.service on 127.0.0.1:<port>
//!   - ~/.local/bin/ratlabs-mesh launcher that opens the studio as an app window
//!   - desktop entry and icons so it shows in the launcher
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use mesh_core::config;

#[derive(rust_embed::Embed)]
#[folder = "../../assets"]
#[include = "icon-*.png"]
struct Icons;

const SIZES: &[u32] = &[32, 48, 64, 128, 256, 512];

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// The binary the service should run. Run from a download folder, copy it to ~/.local/bin
/// first, so deleting the download doesn't break the app. Installs that already live on
/// PATH (cargo install, a package manager) are used where they are.
fn install_binary(h: &std::path::Path) -> Result<PathBuf> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let settled = [h.join(".local/bin"), h.join(".cargo/bin"), PathBuf::from("/usr"), PathBuf::from("/opt")];
    if settled.iter().any(|d| exe.starts_with(d)) {
        return Ok(exe);
    }
    let dest = h.join(".local/bin/ratmesh");
    std::fs::create_dir_all(dest.parent().unwrap())?;
    let tmp = dest.with_extension("new");
    std::fs::copy(&exe, &tmp)?;
    std::fs::rename(&tmp, &dest)?; // atomic, works while an older copy is running
    println!("Installed {}", dest.display());
    let on_path = std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d == dest.parent().unwrap()));
    if !on_path {
        println!("  Add ~/.local/bin to your PATH to run `ratmesh` from a terminal.");
    }
    Ok(dest)
}

fn systemctl(args: &[&str]) -> bool {
    Command::new("systemctl").arg("--user").args(args).status().is_ok_and(|s| s.success())
}

pub fn run(uninstall: bool) -> Result<()> {
    if !cfg!(target_os = "linux") {
        bail!("install-desktop is for Linux; elsewhere run `ratmesh serve` and open {}", config().url());
    }
    let h = home();
    let unit = h.join(".config/systemd/user/ratlabs-mesh.service");
    let bin = h.join(".local/bin/ratlabs-mesh");
    let desktop = h.join(".local/share/applications/ratlabs-mesh.desktop");
    let icon = |n: u32| h.join(format!(".local/share/icons/hicolor/{n}x{n}/apps/ratlabs-mesh.png"));

    if uninstall {
        systemctl(&["disable", "--now", "ratlabs-mesh.service"]);
        for p in [&unit, &bin, &desktop] {
            std::fs::remove_file(p).ok();
        }
        let copied = h.join(".local/bin/ratmesh");
        if copied.is_file() && !copied.is_symlink() {
            std::fs::remove_file(&copied).ok();
            println!("Removed {}.", copied.display());
        }
        for &n in SIZES {
            std::fs::remove_file(icon(n)).ok();
        }
        systemctl(&["daemon-reload"]);
        println!("Removed the service and launcher. Your jobs and library are untouched.");
        return Ok(());
    }

    let exe = install_binary(&h)?;
    let port = config().port;
    let url = config().url();
    for p in [&unit, &bin, &desktop] {
        std::fs::create_dir_all(p.parent().unwrap())?;
    }
    std::fs::write(
        &unit,
        format!(
            "[Unit]\nDescription=RatLabs Mesh (image to 3D)\n\n[Service]\nEnvironment=RATMESH_PORT={port}\nExecStart={} serve --port {port}\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
            exe.display()
        ),
    )?;
    // Opens the studio as an app window, starting the service if needed. Prefers Omarchy's
    // web-app launcher, then any Chromium-family browser in app mode, then the default browser.
    std::fs::write(
        &bin,
        format!(
            r#"#!/usr/bin/env bash
set -euo pipefail
URL="{url}"
up() {{ curl -fsS -o /dev/null --max-time 1 "${{URL}}api/config"; }}
if ! up; then
  systemctl --user start ratlabs-mesh.service
  for _ in $(seq 1 80); do up && break; sleep 0.1; done
  up || {{ notify-send "RatLabs Mesh" "Studio failed to start: journalctl --user -u ratlabs-mesh" 2>/dev/null; exit 1; }}
fi
if command -v omarchy-launch-webapp >/dev/null; then exec omarchy-launch-webapp "$URL"; fi
for b in chromium google-chrome-stable google-chrome brave brave-browser microsoft-edge-stable vivaldi; do
  command -v "$b" >/dev/null && exec "$b" --app="$URL"
done
exec xdg-open "$URL"
"#
        ),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))?;
    }
    for &n in SIZES {
        if let Some(f) = Icons::get(&format!("icon-{n}.png")) {
            std::fs::create_dir_all(icon(n).parent().unwrap())?;
            std::fs::write(icon(n), &f.data)?;
        }
    }
    Command::new("gtk-update-icon-cache").args(["-q"]).arg(h.join(".local/share/icons/hicolor")).stderr(std::process::Stdio::null()).status().ok();
    std::fs::write(
        &desktop,
        format!(
            "[Desktop Entry]\nType=Application\nName=RatLabs Mesh\nComment=Turn character art into finished 3D models\nExec={}\nIcon=ratlabs-mesh\nTerminal=false\nCategories=Graphics;3DGraphics;\n",
            bin.display()
        ),
    )?;
    systemctl(&["daemon-reload"]);
    systemctl(&["enable", "ratlabs-mesh.service"]);
    systemctl(&["restart", "ratlabs-mesh.service"]);
    let t = Instant::now();
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(1)).build()?;
    while t.elapsed() < Duration::from_secs(10) {
        if client.get(format!("{url}api/config")).send().is_ok_and(|r| r.status().is_success()) {
            println!("✓ RatLabs Mesh is running on {url}  (open it from your app launcher)");
            println!("  Connect an AI: `ratmesh connect`");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    bail!("the service did not come up: journalctl --user -u ratlabs-mesh")
}
