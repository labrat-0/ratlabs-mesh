//! The fal and Meshy adapters against a mock API (no keys, no spend).
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mesh_engines::GenOptions;
use mesh_engines::fal::{Fal, Hunyuan3, Trellis2};
use mesh_engines::meshy::{Api, Meshy};
use serde_json::{Value, json};

type Calls = Arc<Mutex<Vec<(String, String, String, Vec<u8>)>>>;

fn img() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/input/rat.png")
}

/// Answers like fal + Meshy + their CDNs; records (method, path, auth, body).
fn mock() -> (String, Calls) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", server.server_addr().to_ip().unwrap());
    let calls: Calls = Arc::default();
    let rec = calls.clone();
    let b = base.clone();
    std::thread::spawn(move || {
        let mut polls = 0;
        for mut req in server.incoming_requests() {
            let mut body = vec![];
            req.as_reader().read_to_end(&mut body).unwrap();
            let url = req.url().to_string();
            let path = url.split('?').next().unwrap().to_string();
            let auth = req.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.to_string()).unwrap_or_default();
            let method = req.method().to_string();
            rec.lock().unwrap().push((method.clone(), path.clone(), auth, body));
            let j =
                |v: Value| tiny_http::Response::from_string(v.to_string()).with_header("Content-Type: application/json".parse::<tiny_http::Header>().unwrap());
            let resp = match (method.as_str(), path.as_str()) {
                (_, "/storage/upload/initiate") => j(json!({"upload_url": format!("{b}/put"), "file_url": "https://cdn/in.png"})),
                (_, "/put") => j(json!({})),
                ("POST", p) if p.starts_with("/fal-ai/") => {
                    j(json!({"request_id": "r1", "status_url": format!("{b}/q/r1/status"), "response_url": format!("{b}/q/r1")}))
                }
                (_, "/q/r1/status") => {
                    polls += 1;
                    j(json!({"status": if polls < 2 { "IN_QUEUE" } else { "COMPLETED" }, "queue_position": 0}))
                }
                (_, "/q/r1") => j(json!({"model_glb": {"url": format!("{b}/model.glb")}})),
                ("POST", p) if p.starts_with("/meshy/") => j(json!({"result": "t1"})),
                (_, p) if p.starts_with("/meshy/") => {
                    polls += 1;
                    let done = polls >= 2;
                    j(
                        json!({"status": if done { "SUCCEEDED" } else { "IN_PROGRESS" }, "progress": if done { 100 } else { 40 }, "model_urls": {"glb": format!("{b}/m.glb")}, "consumed_credits": 30}),
                    )
                }
                (_, "/model.glb") => tiny_http::Response::from_string("glb-from-fal"),
                (_, "/m.glb") => tiny_http::Response::from_string("glb-from-meshy"),
                _ => tiny_http::Response::from_string("nope").with_status_code(404),
            };
            req.respond(resp).unwrap();
        }
    });
    (base, calls)
}

fn fal(base: &str) -> Fal {
    Fal { key: "fk".into(), rest: base.into(), queue: base.into(), poll: Duration::ZERO, retry_base: Duration::ZERO }
}

fn opt() -> GenOptions {
    GenOptions { polys: 30000, texture: 2048, quality: "standard".into(), pbr: true, seed: 7, delight: true, ..Default::default() }
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mesh-engines-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join("o.glb")
}

#[test]
fn trellis2() {
    let (base, calls) = mock();
    let out = tmp("trellis");
    let meta = Trellis2::run_with(&fal(&base), &img(), &out, &opt(), &|_| {}).unwrap();
    let calls = calls.lock().unwrap();
    let sub = calls.iter().find(|c| c.1 == "/fal-ai/trellis-2").unwrap();
    assert_eq!(sub.2, "Key fk");
    assert_eq!(calls[1].1, "/put");
    assert_eq!(calls[1].3, std::fs::read(img()).unwrap());
    let body: Value = serde_json::from_slice(&sub.3).unwrap();
    assert_eq!(body["image_url"], "https://cdn/in.png");
    assert_eq!(body["resolution"], "1024");
    assert_eq!(body["decimation_target"], 30000);
    assert_eq!(body["texture_size"], "2048");
    assert_eq!(body["seed"], 7);
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "glb-from-fal");
    assert_eq!(meta["engine"], "trellis2");
}

#[test]
fn hunyuan3_views() {
    let (base, calls) = mock();
    let mut o = opt();
    o.views.insert("back".into(), img());
    Hunyuan3::run_with(&fal(&base), &img(), &tmp("hy3"), &o, &|_| {}).unwrap();
    let calls = calls.lock().unwrap();
    let body: Value = serde_json::from_slice(&calls.iter().find(|c| c.1.starts_with("/fal-ai/hunyuan3d")).unwrap().3).unwrap();
    assert!(body.get("back_image_url").is_some());
    assert!(body.get("left_image_url").is_none());
    assert!(body.get("face_count").is_none());
}

#[test]
fn meshy_single_and_multi() {
    let (base, calls) = mock();
    let api = Api { key: "mk".into(), base: format!("{base}/meshy"), poll: Duration::ZERO };
    let out = tmp("meshy");
    let mut o = opt();
    o.pose = Some("a-pose".into());
    let meta = Meshy::run_with(&api, &img(), &out, &o, &|_| {}).unwrap();
    {
        let c = calls.lock().unwrap();
        assert_eq!(c[0].1, "/meshy/image-to-3d");
        assert_eq!(c[0].2, "Bearer mk");
        let body: Value = serde_json::from_slice(&c[0].3).unwrap();
        assert_eq!(body["target_polycount"], 30000);
        assert_eq!(body["pose_mode"], "a-pose");
        assert_eq!(body["should_remesh"], true);
    }
    assert_eq!(meta["credits"], 30);
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "glb-from-meshy");
    calls.lock().unwrap().clear();
    let mut o = opt();
    o.views.insert("back".into(), img());
    o.views.insert("left".into(), img());
    Meshy::run_with(&api, &img(), &out, &o, &|_| {}).unwrap();
    let c = calls.lock().unwrap();
    assert_eq!(c[0].1, "/meshy/multi-image-to-3d");
    let body: Value = serde_json::from_slice(&c[0].3).unwrap();
    assert_eq!(body["image_urls"].as_array().unwrap().len(), 3);
}
