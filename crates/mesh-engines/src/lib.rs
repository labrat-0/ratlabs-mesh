//! Stage 2: the GPU step. Each engine turns a prepared cut-out into a raw textured GLB.
//!
//! Built in:
//!   hunyuan3       Tencent Hunyuan3D v3 on fal.ai; takes back/left/right   FAL_KEY
//!   trellis2       Microsoft TRELLIS.2 (MIT, open weights) on fal.ai       FAL_KEY
//!   meshy          Meshy's own API                                         MESHY_API_KEY
//!   hunyuan-local  Tencent Hunyuan3D-2 on this CPU. Free, offline, ~25 min  (command manifest)
//!   triposr        TripoSR on this CPU. Free and offline, drafts only       (command manifest)
//!
//! Anything else plugs in with a TOML manifest in ~/.config/ratlabs-mesh/engines/:
//! `type = "command"` runs any program, `type = "fal"` calls any fal.ai model. See plugin.rs.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

pub mod concept;
pub mod fal;
pub mod meshy;
pub mod plugin;

pub use fal::EngineError;

/// What the pipeline hands an engine.
#[derive(Debug, Clone, Default)]
pub struct GenOptions {
    pub polys: u32,
    pub texture: u32,
    pub quality: String,
    pub pbr: bool,
    pub pose: Option<String>,
    pub seed: i64,
    pub delight: bool,
    /// Prepared extra views by side: back, left, right.
    pub views: BTreeMap<String, PathBuf>,
    /// Full-resolution cut-out, for engines that transfer fine detail back.
    pub detail_source: Option<PathBuf>,
}

/// How an engine describes itself to the studio and to AI clients.
#[derive(Debug, Clone, Serialize)]
pub struct EngineInfo {
    pub label: String,
    /// The key it needs, or None for local engines.
    pub key: Option<String>,
    pub cost: String,
    pub multiview: bool,
    /// Runs on this machine (one at a time, minutes per model).
    pub local: bool,
    /// Where it came from: "builtin" or the plugin manifest's path.
    pub source: String,
    /// How to install it when it isn't ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup: Option<String>,
}

pub type Log<'a> = &'a (dyn Fn(&str) + Sync);

pub trait Engine: Send + Sync {
    fn id(&self) -> &str;
    fn info(&self) -> EngineInfo;
    /// Keys present / installed.
    fn ready(&self, keys: &BTreeMap<String, String>) -> bool;
    /// Write a raw GLB to `out`; return metadata kept with the result.
    fn run(&self, img: &Path, out: &Path, opt: &GenOptions, log: Log) -> Result<Value>;
}

/// Every engine: built-ins first, then plugins (a plugin with a built-in's id replaces it).
pub fn registry() -> Vec<Arc<dyn Engine>> {
    let mut list: Vec<Arc<dyn Engine>> = vec![Arc::new(fal::Hunyuan3), Arc::new(fal::Trellis2), Arc::new(meshy::Meshy)];
    for e in plugin::builtin().into_iter().chain(plugin::load_user()) {
        match list.iter().position(|x| x.id() == e.id()) {
            Some(i) => list[i] = e,
            None => list.push(e),
        }
    }
    list
}

pub fn get(id: &str) -> Option<Arc<dyn Engine>> {
    registry().into_iter().find(|e| e.id() == id)
}

/// `{id: info + ready}` in display order.
pub fn available() -> Value {
    let keys = mesh_core::keys::load();
    let mut m = serde_json::Map::new();
    for e in registry() {
        let mut v = serde_json::to_value(e.info()).unwrap();
        v["ready"] = e.ready(&keys).into();
        m.insert(e.id().to_string(), v);
    }
    Value::Object(m)
}

/// Stream a URL to a file.
pub fn download(url: &str, out: &Path) -> Result<()> {
    let mut r = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(300)).build()?.get(url).send()?.error_for_status()?;
    let tmp = out.with_extension("part");
    let mut f = std::fs::File::create(&tmp)?;
    std::io::copy(&mut r, &mut f)?;
    std::fs::rename(tmp, out)?;
    Ok(())
}

pub fn data_uri(path: &Path) -> Result<String> {
    use base64::Engine as _;
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(std::fs::read(path)?)))
}
