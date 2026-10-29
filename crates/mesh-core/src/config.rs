//! Where RatLabs Mesh keeps things. Standard per-user folders by default:
//!
//!   ~/.config/ratlabs-mesh/   keys.env, config.toml, engines/*.toml (engine plugins)
//!   ~/.local/share/ratlabs-mesh/  jobs/, library/, engines/ (local engine installs), runtime/
//!   ~/.cache/ratlabs-mesh/    market catalogues, the local-engine lock
//!
//! Each path can be overridden in config.toml or with an environment variable
//! (RATMESH_JOBS, RATMESH_LIBRARY, RATMESH_BLENDER, RATMESH_HUNYUAN, RATMESH_TRIPOSR, RATMESH_ANIM).
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

pub const DEFAULT_PORT: u16 = 8771;

#[derive(Debug, Clone)]
pub struct Config {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub jobs: PathBuf,
    pub library: PathBuf,
    /// Embedded helper scripts (Blender, local engines) are written here before they run.
    pub runtime: PathBuf,
    pub keys_file: PathBuf,
    pub plugins: PathBuf,
    pub blender: String,
    pub hunyuan: PathBuf,
    pub triposr: PathBuf,
    pub anim: PathBuf,
    pub port: u16,
}

#[derive(Deserialize, Default)]
struct File {
    jobs: Option<PathBuf>,
    library: Option<PathBuf>,
    blender: Option<String>,
    hunyuan: Option<PathBuf>,
    triposr: Option<PathBuf>,
    anim: Option<PathBuf>,
    port: Option<u16>,
}

fn home() -> PathBuf {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
}

/// Expand a leading `~/`.
pub fn expand(p: impl AsRef<Path>) -> PathBuf {
    let p = p.as_ref();
    match p.strip_prefix("~") {
        Ok(rest) => home().join(rest),
        Err(_) => p.to_path_buf(),
    }
}

/// The first candidate that exists, else the first one.
fn first_existing(cands: &[PathBuf]) -> PathBuf {
    cands.iter().find(|p| p.exists()).unwrap_or(&cands[0]).clone()
}

impl Config {
    pub fn load() -> Config {
        let dirs = directories::ProjectDirs::from("tech", "RatLabs", "ratlabs-mesh");
        let (config_dir, data_dir, cache_dir) = match &dirs {
            Some(d) => (d.config_dir().to_path_buf(), d.data_dir().to_path_buf(), d.cache_dir().to_path_buf()),
            None => (home().join(".config/ratlabs-mesh"), home().join(".local/share/ratlabs-mesh"), home().join(".cache/ratlabs-mesh")),
        };
        let file: File = std::fs::read_to_string(config_dir.join("config.toml")).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default();
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let pick = |var: &str, from_file: Option<PathBuf>, default: PathBuf| env(var).or(from_file).map(expand).unwrap_or(default);
        // Local engines installed before 0.2 live in ~/tools; keep finding them there.
        let tools = home().join("tools");
        Config {
            jobs: pick("RATMESH_JOBS", file.jobs, data_dir.join("jobs")),
            library: pick("RATMESH_LIBRARY", file.library, data_dir.join("library")),
            runtime: data_dir.join("runtime"),
            keys_file: config_dir.join("keys.env"),
            plugins: config_dir.join("engines"),
            blender: std::env::var("RATMESH_BLENDER").ok().or(file.blender).unwrap_or_else(|| "blender".into()),
            hunyuan: pick("RATMESH_HUNYUAN", file.hunyuan, first_existing(&[data_dir.join("engines/Hunyuan3D-2"), tools.join("Hunyuan3D-2")])),
            triposr: pick("RATMESH_TRIPOSR", file.triposr, first_existing(&[data_dir.join("engines/TripoSR"), tools.join("TripoSR")])),
            anim: pick("RATMESH_ANIM", file.anim, first_existing(&[data_dir.join("anim"), tools.join("anim")])),
            port: std::env::var("RATMESH_PORT").ok().and_then(|p| p.parse().ok()).or(file.port).unwrap_or(DEFAULT_PORT),
            config_dir,
            data_dir,
            cache_dir,
        }
    }

    /// Quaternius Universal Animation Library 1 + 2 (CC0), fetched by `ratmesh setup animations`.
    pub fn anim_libs(&self) -> Vec<PathBuf> {
        vec![
            self.anim.join("universal_animation_librarystandard/Animation Library[Standard]/Godot/AnimationLibrary_Godot_Standard.glb"),
            self.anim.join("universal_animation_library_2standard/Universal Animation Library 2 [Standard]/Unreal-Godot/UAL2_Standard.glb"),
        ]
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }
}

static CONFIG: OnceLock<Config> = OnceLock::new();

pub fn config() -> &'static Config {
    CONFIG.get_or_init(Config::load)
}

/// Write `contents` to `path` only when it differs, creating parent folders.
/// Used to drop embedded helper scripts on disk where Blender or Python can run them.
pub fn materialize(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if std::fs::read(path).map(|old| old == contents).unwrap_or(false) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(tmp, path)
}
