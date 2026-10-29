//! Meshy's own API (latest model), single- or multi-image.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};

use crate::fal::{err, need};
use crate::{Engine, EngineInfo, GenOptions, Log, data_uri, download};

pub struct Meshy;

pub struct Api {
    pub key: String,
    pub base: String,
    pub poll: Duration,
}

impl Meshy {
    pub fn run_with(api: &Api, img: &Path, out: &Path, opt: &GenOptions, log: Log) -> Result<Value> {
        let mut views = vec![img.to_path_buf()];
        views.extend(["back", "left", "right"].iter().filter_map(|k| opt.views.get(*k).cloned()));
        let mut body = json!({
            "ai_model": "latest",
            "should_remesh": true,
            "topology": "triangle",
            "target_polycount": opt.polys.min(300000),
            "should_texture": true,
            "enable_pbr": opt.pbr,
            "texture_resolution": if opt.texture >= 4096 { "4k" } else { "2k" },
            "target_formats": ["glb"],
            "remove_lighting": opt.delight,
        });
        if let Some(p) = &opt.pose {
            body["pose_mode"] = p.clone().into();
        }
        let path = if views.len() > 1 {
            body["image_urls"] = views.iter().take(4).map(|v| data_uri(v)).collect::<Result<Vec<_>>>()?.into();
            "multi-image-to-3d"
        } else {
            body["image_url"] = data_uri(img)?.into();
            "image-to-3d"
        };
        let c = reqwest::blocking::Client::builder().timeout(Duration::from_secs(120)).build()?;
        let auth = format!("Bearer {}", api.key);
        let r = c.post(format!("{}/{path}", api.base)).header("Authorization", &auth).json(&body).send()?;
        let status = r.status();
        let text = r.text()?;
        if !status.is_success() {
            return Err(err(format!("meshy {}: {}", status.as_u16(), &text[..text.len().min(400)])));
        }
        let tid = serde_json::from_str::<Value>(&text)?["result"].as_str().unwrap_or_default().to_string();
        log(&format!("meshy task {tid}"));
        let mut last = Value::Null;
        let t = loop {
            let t: Value = c.get(format!("{}/{path}/{tid}", api.base)).header("Authorization", &auth).send()?.json()?;
            if t["progress"] != last {
                log(&format!("meshy {} {}%", t["status"].as_str().unwrap_or("").to_lowercase(), t["progress"].as_i64().unwrap_or(0)));
                last = t["progress"].clone();
            }
            match t["status"].as_str().unwrap_or("") {
                "SUCCEEDED" => break t,
                s @ ("FAILED" | "CANCELED") => return Err(err(format!("meshy {s}: {}", t["task_error"]))),
                _ => std::thread::sleep(api.poll),
            }
        };
        download(t["model_urls"]["glb"].as_str().unwrap_or_default(), out)?;
        Ok(json!({"engine": "meshy", "task": tid, "credits": t["consumed_credits"]}))
    }
}

impl Engine for Meshy {
    fn id(&self) -> &str {
        "meshy"
    }
    fn info(&self) -> EngineInfo {
        EngineInfo {
            label: "Meshy (latest)".into(),
            key: Some("MESHY_API_KEY".into()),
            cost: "Meshy credits".into(),
            multiview: true,
            local: false,
            source: "builtin".into(),
            setup: Some("Add a Meshy API key under Keys (meshy.ai > API)".into()),
        }
    }
    fn ready(&self, keys: &BTreeMap<String, String>) -> bool {
        keys.get("MESHY_API_KEY").is_some_and(|v| !v.is_empty())
    }
    fn run(&self, img: &Path, out: &Path, opt: &GenOptions, log: Log) -> Result<Value> {
        let api = Api { key: need("MESHY_API_KEY")?, base: "https://api.meshy.ai/openapi/v1".into(), poll: Duration::from_secs(5) };
        Self::run_with(&api, img, out, opt, log)
    }
}
