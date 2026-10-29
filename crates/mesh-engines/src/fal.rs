//! fal.ai: upload to its CDN, run a queue request, and the two built-in fal engines.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};

use crate::{Engine, EngineInfo, GenOptions, Log, download};

/// An engine-side failure worth showing the user as-is (bad key, provider error...).
#[derive(Debug)]
pub struct EngineError(pub String);

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for EngineError {}

pub fn err(msg: impl Into<String>) -> anyhow::Error {
    EngineError(msg.into()).into()
}

/// fal endpoints; tests point these at a mock server.
#[derive(Clone)]
pub struct Fal {
    pub key: String,
    pub rest: String,
    pub queue: String,
    pub poll: Duration,
    pub retry_base: Duration,
}

pub fn need(key: &str) -> Result<String> {
    mesh_core::keys::get(key).ok_or_else(|| err(format!("{key} is not set. Add it with Keys in the studio or in {}", mesh_core::config().keys_file.display())))
}

fn short(s: &str) -> &str {
    let mut n = s.len().min(400);
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    &s[..n]
}

impl Fal {
    pub fn from_keys() -> Result<Fal> {
        Ok(Fal {
            key: need("FAL_KEY")?,
            rest: "https://rest.alpha.fal.ai".into(),
            queue: "https://queue.fal.run".into(),
            poll: Duration::from_secs(3),
            retry_base: Duration::from_secs(20),
        })
    }

    fn client(&self, timeout: u64) -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder().timeout(Duration::from_secs(timeout)).build().unwrap()
    }

    /// Upload an image to fal's CDN and return its URL. Some fal models (Hunyuan3D v3)
    /// hand the image to another provider that cannot read data: URIs.
    pub fn upload(&self, path: &Path) -> Result<String> {
        let r = self
            .client(120)
            .post(format!("{}/storage/upload/initiate?storage_type=fal-cdn-v3", self.rest))
            .header("Authorization", format!("Key {}", self.key))
            .json(&json!({"content_type": "image/png", "file_name": path.file_name().unwrap_or_default().to_string_lossy()}))
            .send()?;
        let status = r.status();
        let text = r.text()?;
        if !status.is_success() {
            return Err(err(format!("fal upload {}: {}", status.as_u16(), short(&text))));
        }
        let j: Value = serde_json::from_str(&text)?;
        let u = self.client(300).put(j["upload_url"].as_str().unwrap_or_default()).header("Content-Type", "image/png").body(std::fs::read(path)?).send()?;
        if !u.status().is_success() {
            let code = u.status().as_u16();
            return Err(err(format!("fal upload {code}: {}", short(&u.text().unwrap_or_default()))));
        }
        Ok(j["file_url"].as_str().unwrap_or_default().to_string())
    }

    /// Run a fal queue request, retrying when the provider behind fal is briefly unavailable
    /// (Hunyuan3D v3 answers 504 "downstream service unavailable" under load; those aren't billed).
    pub fn run(&self, endpoint: &str, payload: &Value, log: Log, tries: u32) -> Result<Value> {
        let mut attempt = 0;
        loop {
            match self.run_once(endpoint, payload, log) {
                Ok(v) => return Ok(v),
                Err(e) => {
                    let msg = e.to_string();
                    let busy = msg.contains("downstream") || msg.contains(" 502") || msg.contains(" 503");
                    if attempt + 1 >= tries || !busy {
                        return Err(e);
                    }
                    let wait = self.retry_base * 2u32.pow(attempt);
                    log(&format!("fal provider busy, retrying in {}s", wait.as_secs()));
                    std::thread::sleep(wait);
                    attempt += 1;
                }
            }
        }
    }

    fn run_once(&self, endpoint: &str, payload: &Value, log: Log) -> Result<Value> {
        let c = self.client(120);
        let auth = format!("Key {}", self.key);
        let r = c.post(format!("{}/{endpoint}", self.queue)).header("Authorization", &auth).json(payload).send()?;
        let status = r.status();
        let text = r.text()?;
        if !status.is_success() {
            return Err(err(format!("fal submit {}: {}", status.as_u16(), short(&text))));
        }
        let job: Value = serde_json::from_str(&text)?;
        log(&format!("fal queued {}", job["request_id"].as_str().unwrap_or("?")));
        let mut last = String::new();
        loop {
            let s: Value = c.get(job["status_url"].as_str().unwrap_or_default()).query(&[("logs", "1")]).header("Authorization", &auth).send()?.json()?;
            let st = s["status"].as_str().unwrap_or("").to_string();
            if st != last {
                let pos = s["queue_position"].as_i64().map(|p| format!(" (position {p})")).unwrap_or_default();
                log(&format!("fal {}{pos}", st.to_lowercase()));
                last = st.clone();
            }
            match st.as_str() {
                "COMPLETED" => break,
                "IN_QUEUE" | "IN_PROGRESS" => std::thread::sleep(self.poll),
                _ => return Err(err(format!("fal status {s}"))),
            }
        }
        let res = c.get(job["response_url"].as_str().unwrap_or_default()).header("Authorization", &auth).send()?;
        let status = res.status();
        let text = res.text()?;
        if !status.is_success() {
            return Err(err(format!("fal result {}: {}", status.as_u16(), short(&text))));
        }
        Ok(serde_json::from_str(&text)?)
    }
}

pub fn quality_px(q: &str) -> u32 {
    match q {
        "draft" => 512,
        "high" => 1536,
        _ => 1024,
    }
}

fn has_key(keys: &BTreeMap<String, String>, k: &str) -> bool {
    keys.get(k).is_some_and(|v| !v.is_empty())
}

fn info(label: &str, cost: &str, multiview: bool) -> EngineInfo {
    EngineInfo {
        label: label.into(),
        key: Some("FAL_KEY".into()),
        cost: cost.into(),
        multiview,
        local: false,
        source: "builtin".into(),
        setup: Some("Add a fal.ai key under Keys (fal.ai/dashboard/keys)".into()),
    }
}

pub struct Trellis2;

impl Trellis2 {
    pub fn payload(image_url: String, opt: &GenOptions) -> Value {
        json!({
            "image_url": image_url,
            "resolution": quality_px(&opt.quality).to_string(),
            "seed": opt.seed,
            // TRELLIS bakes the texture after decimating, which beats decimating a baked mesh.
            // Its target counts vertices, but ragged cloth and many parts give well under 2 tris
            // per vertex (15k verts came back as 13.7k tris), so ask for the full budget and let
            // the finisher trim.
            "decimation_target": opt.polys.max(20000),
            "texture_size": opt.texture.clamp(1024, 4096).to_string(),
            "remesh": true,
        })
    }

    pub fn run_with(fal: &Fal, img: &Path, out: &Path, opt: &GenOptions, log: Log) -> Result<Value> {
        let payload = Self::payload(fal.upload(img)?, opt);
        let r = fal.run("fal-ai/trellis-2", &payload, log, 4)?;
        download(r["model_glb"]["url"].as_str().ok_or_else(|| err(format!("fal returned no model: {r}")))?, out)?;
        Ok(json!({"engine": "trellis2", "host": "fal.ai", "resolution": quality_px(&opt.quality), "seed": opt.seed}))
    }
}

impl Engine for Trellis2 {
    fn id(&self) -> &str {
        "trellis2"
    }
    fn info(&self) -> EngineInfo {
        info("TRELLIS.2 (fal.ai)", "~$0.25–0.35", false)
    }
    fn ready(&self, keys: &BTreeMap<String, String>) -> bool {
        has_key(keys, "FAL_KEY")
    }
    fn run(&self, img: &Path, out: &Path, opt: &GenOptions, log: Log) -> Result<Value> {
        Self::run_with(&Fal::from_keys()?, img, out, opt, log)
    }
}

pub struct Hunyuan3;

impl Hunyuan3 {
    pub fn run_with(fal: &Fal, img: &Path, out: &Path, opt: &GenOptions, log: Log) -> Result<Value> {
        let mut payload = json!({
            "input_image_url": fal.upload(img)?,
            "enable_pbr": opt.pbr,
            // face_count is billed as an extra; the default 500k mesh is what we want anyway,
            // since the finisher decimates and bakes the surplus detail into a normal map.
            "generate_type": "Normal",
        });
        for side in ["back", "left", "right"] {
            if let Some(p) = opt.views.get(side) {
                payload[format!("{side}_image_url")] = fal.upload(p)?.into();
            }
        }
        let r = fal.run("fal-ai/hunyuan3d-v3/image-to-3d", &payload, log, 4)?;
        let url = r["model_glb"]["url"].as_str().or(r["model_urls"]["glb"]["url"].as_str()).ok_or_else(|| err(format!("fal returned no model: {r}")))?;
        download(url, out)?;
        Ok(json!({"engine": "hunyuan3", "host": "fal.ai", "views": opt.views.keys().collect::<Vec<_>>()}))
    }
}

impl Engine for Hunyuan3 {
    fn id(&self) -> &str {
        "hunyuan3"
    }
    fn info(&self) -> EngineInfo {
        info("Hunyuan3D v3 (fal.ai)", "~$0.38, +$0.15 with PBR; best faces", true)
    }
    fn ready(&self, keys: &BTreeMap<String, String>) -> bool {
        has_key(keys, "FAL_KEY")
    }
    fn run(&self, img: &Path, out: &Path, opt: &GenOptions, log: Log) -> Result<Value> {
        Self::run_with(&Fal::from_keys()?, img, out, opt, log)
    }
}
