//! The tools. Each returns an MCP tool result; failures come back as `isError` results so
//! the model can read them and recover, never as protocol errors.
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use mesh_core::{Event, Job, NewJob, emit, jobs};
use mesh_pipeline::{Refinish, Work};
use serde_json::{Value, json};

use crate::input::{IMAGE_EXTS, b64_file, resolve};

const VIEWS: &[&str] = &["sheet", "front", "three_quarter", "side", "back"];
const MAX_WAIT: u64 = 600;

fn schema(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

fn tool(name: &str, title: &str, description: &str, input: Value, read_only: bool, open_world: bool) -> Value {
    json!({
        "name": name, "title": title, "description": description, "inputSchema": input,
        "annotations": {"title": title, "readOnlyHint": read_only, "destructiveHint": false, "idempotentHint": read_only, "openWorldHint": open_world},
    })
}

fn kinds() -> Vec<&'static str> {
    mesh_core::PRESETS.iter().map(|p| p.kind).collect()
}

fn finishing_props() -> Value {
    json!({
        "height": {"type": "number", "description": "Real-world height in metres (up axis). Default from kind: character 1.7, creature 1.2, critter 0.1, prop 0.5. 0 keeps the model's own scale."},
        "polys": {"type": "integer", "minimum": 500, "description": "Triangle budget. Default 30000 (prop 15000, hero 80000)."},
        "texture": {"type": "integer", "enum": [1024, 2048, 4096], "description": "Texture size in px. Default 2048 (hero 4096)."},
        "origin": {"type": "string", "enum": ["bottom", "center"], "description": "Pivot: feet on the ground (bottom) or centred."},
    })
}

fn merge(mut a: Value, b: Value) -> Value {
    if let (Some(a), Value::Object(b)) = (a.as_object_mut(), b) {
        a.extend(b);
    }
    a
}

pub fn list() -> Vec<Value> {
    let job_id = json!({"type": "string", "description": "Job id from generate_model, import_model or list_jobs."});
    let wait = json!({"type": "integer", "minimum": 0, "maximum": MAX_WAIT, "description": "Block up to this many seconds for the job to finish (0 returns at once). Remote engines take 1-5 min, local ones ~25 min."});
    let image = json!({"type": "string", "description": "The art: an absolute file path, an http(s) URL or a data: URI (png, jpg, webp)."});
    let crop = json!({"type": "array", "items": {"type": "number", "minimum": 0, "maximum": 1}, "minItems": 4, "maxItems": 4, "description": "[x0, y0, x1, y1] as 0-1 fractions of the image, to pick one subject out of a scene."});
    vec![
        tool(
            "mesh_status",
            "Mesh status",
            "What RatLabs Mesh can do right now: each engine (ready or not, cost, whether it is local/free, whether it takes back/left/right views, how to set it up), the asset kinds and their presets, which provider keys are set (names only), whether Blender is found, the work queue and the studio URL. Call this first.",
            schema(json!({}), &[]),
            true,
            false,
        ),
        tool(
            "preview_cutout",
            "Preview cut-out",
            "Cut the subject out of an image exactly as generation will, and return the cut-out as an image plus quality notes. Free and fast. Use it to check the background was removed cleanly before spending on generate_model; if notes say it looks like a scene, pass a crop.",
            schema(json!({"image": image, "crop": crop}), &["image"]),
            true,
            false,
        ),
        tool(
            "generate_model",
            "Generate 3D model",
            "Start turning one image into finished, game-ready GLB models: cut-out, image-to-3D, cleanup, retopology to the triangle budget, real-world sizing, PBR texture, normal-map bake and front/three-quarter/side/back review renders. Opens in the user's studio. Returns the job; follow it with get_job (wait_seconds) and look at it with view_model. Paid engines bill the user's account per variant.",
            schema(
                merge(
                    json!({
                        "image": image,
                        "kind": {"type": "string", "enum": kinds(), "description": "What it is; sets height, triangle budget and texture size. Default character."},
                        "engine": {"type": "string", "description": "Engine id from mesh_status (e.g. hunyuan3, trellis2, meshy, hunyuan-local). Default: the first ready one."},
                        "name": {"type": "string", "description": "Asset name; defaults to the file name."},
                        "variants": {"type": "integer", "minimum": 1, "maximum": 4, "description": "Seeds to generate and pick from. Default 1. Each variant costs a generation."},
                        "quality": {"type": "string", "enum": mesh_core::presets::QUALITY, "description": "Engine resolution. Default standard."},
                        "crop": crop,
                        "back": {"type": "string", "description": "Optional back view (path/URL/data URI) for multiview engines; sharper backs."},
                        "left": {"type": "string", "description": "Optional left view, for multiview engines."},
                        "right": {"type": "string", "description": "Optional right view, for multiview engines."},
                        "pose": {"type": "string", "enum": ["a-pose", "t-pose"], "description": "Meshy only: force a rig-friendly pose."},
                        "pbr": {"type": "boolean", "description": "PBR materials. Default true."},
                        "delight": {"type": "boolean", "description": "Remove baked lighting from the art. Default true; false keeps the art's own mood."},
                        "keep_background": {"type": "boolean", "description": "The art is already isolated on transparency; skip the cut-out."},
                        "seed": {"type": "integer"},
                        "wait_seconds": wait,
                    }),
                    finishing_props(),
                ),
                &["image"],
            ),
            false,
            true,
        ),
        tool(
            "import_model",
            "Finish an existing model",
            "Run an existing GLB (from anywhere) through the finishing pipeline only: cleanup, triangle budget, real-world size, texture limits and review renders. No generation cost.",
            schema(
                merge(
                    json!({
                        "glb": {"type": "string", "description": "Absolute path, http(s) URL or data: URI of a .glb."},
                        "kind": {"type": "string", "enum": kinds()},
                        "name": {"type": "string"},
                        "wait_seconds": wait,
                    }),
                    finishing_props(),
                ),
                &["glb"],
            ),
            false,
            false,
        ),
        tool(
            "get_job",
            "Get job",
            "A job's status (queued, running, done, failed), current stage, recent log, error, and for each finished variant: GLB path, rigged GLB path, review sheet path, triangle count, size in metres and warnings. Pass wait_seconds to block until it finishes.",
            schema(json!({"job_id": job_id, "wait_seconds": wait}), &["job_id"]),
            true,
            false,
        ),
        tool(
            "list_jobs",
            "List jobs",
            "Recent jobs in the user's library, newest first.",
            schema(
                json!({"limit": {"type": "integer", "minimum": 1, "maximum": 200, "description": "Default 20."}, "status": {"type": "string", "enum": ["queued", "running", "done", "failed"]}}),
                &[],
            ),
            true,
            false,
        ),
        tool(
            "view_model",
            "View model",
            "Look at a finished model: returns its textured review renders as an image (the 4-view sheet by default, or one view) and shows the same model to the user in the studio. This is how you see a model; clay/wire/neon views exist only in the user's studio (set them with show_in_studio). Use it to judge shape, texture and pose before refinishing or rigging.",
            schema(
                json!({"job_id": job_id, "variant": {"type": "integer", "minimum": 1, "maximum": 4}, "view": {"type": "string", "enum": VIEWS, "description": "Default sheet."}}),
                &["job_id"],
            ),
            true,
            false,
        ),
        tool(
            "show_in_studio",
            "Show in studio",
            "Point the user's studio at a job: select the variant and set the viewer to texture, clay (shape only) or wire (topology), with studio or neon lighting. This changes what the user sees; it returns no image. To look at the model yourself, use view_model (textured review renders). Don't try to screenshot the studio.",
            schema(
                json!({"job_id": job_id, "variant": {"type": "integer", "minimum": 1, "maximum": 4}, "mode": {"type": "string", "enum": ["texture", "clay", "wire"]}, "light": {"type": "string", "enum": ["studio", "neon"]}}),
                &["job_id"],
            ),
            false,
            false,
        ),
        tool(
            "refinish_model",
            "Refinish model",
            "Re-run only the finishing stage of a done job with a new height, triangle budget, texture size, pivot or rotation. Free (no generation), about a minute.",
            schema(
                merge(
                    json!({"job_id": job_id, "rotate": {"type": "number", "description": "Extra degrees about the up axis, if the model faces the wrong way (added to the current turn)."}, "wait_seconds": wait}),
                    finishing_props(),
                ),
                &["job_id"],
            ),
            false,
            false,
        ),
        tool(
            "rig_model",
            "Rig and animate",
            "Auto-rig every variant of a done job with a 22-bone skeleton and retarget 88 free CC0 animations (idle, walk, run, attack...). Works on standing bipeds with arms away from the body. Produces a rigged GLB per variant.",
            schema(json!({"job_id": job_id, "wait_seconds": wait}), &["job_id"]),
            false,
            false,
        ),
        tool(
            "rerun_job",
            "Run job again",
            "Run a job again from the start with the same settings (new generation; paid engines bill again).",
            schema(json!({"job_id": job_id, "wait_seconds": wait}), &["job_id"]),
            false,
            true,
        ),
        tool(
            "concept_art",
            "Concept art",
            "Make source art from a text description: one whole object, centred on a plain background, the kind of picture image-to-3D engines read well. Optional reference images steer style and materials. Uses fal.ai Nano Banana (~$0.04, needs FAL_KEY). Returns the image and its path for generate_model.",
            schema(
                json!({
                    "description": {"type": "string", "description": "What to show, e.g. 'a rusty oil drum with a hazard stripe'."},
                    "style": {"type": "string", "description": "Materials, era and mood to match. Default: grimy retro-futurist underground lab."},
                    "references": {"type": "array", "items": {"type": "string"}, "description": "Style reference images (paths/URLs/data URIs)."},
                    "seed": {"type": "integer"},
                }),
                &["description"],
            ),
            false,
            true,
        ),
        tool(
            "load_starter_pack",
            "Load starter pack",
            "Add the RatLabs Mesh starter pack to the user's library: 30 free CC0 props from Poly Haven (lab gear, tools, lamps, pipes, crates...) and the Labrat character from Rat Underground, rigged with 5 animations (RatLabs' own art: usable in the studio only, not for reuse), all finished at real-world size. Downloads ~80 MB once; items already present are skipped. Good for trying refinish, rig and view_model without paying for a generation.",
            schema(json!({}), &[]),
            false,
            true,
        ),
        tool(
            "market_search",
            "Search free assets",
            "Search free asset libraries: Poly Haven (CC0 models, materials, HDRIs), ambientCG (CC0 materials, HDRIs), Sketchfab (models, licence per model) and Poly Pizza (low-poly models). Every result carries its licence and author.",
            schema(
                json!({
                    "query": {"type": "string"},
                    "kind": {"type": "string", "enum": mesh_market::KINDS, "description": "Default model."},
                    "source": {"type": "string", "enum": mesh_market::SOURCES.iter().map(|s| s.id).collect::<Vec<_>>(), "description": "Default polyhaven."},
                    "page": {"type": "integer", "minimum": 0},
                }),
                &[],
            ),
            true,
            true,
        ),
        tool(
            "market_grab",
            "Download a free asset",
            "Download a market_search result into the user's library, keeping its licence, author and source page with it. Pass the result object as `item`.",
            schema(
                json!({"item": {"type": "object", "description": "One item from market_search, unchanged."}, "res": {"type": "string", "enum": ["1k", "2k", "4k"], "description": "Texture/HDRI resolution. Default 2k."}}),
                &["item"],
            ),
            false,
            true,
        ),
        tool(
            "market_library",
            "Downloaded assets",
            "Everything already downloaded from the Market, with licence, author and folder.",
            schema(json!({}), &[]),
            true,
            false,
        ),
        tool(
            "market_finish",
            "Finish a downloaded model",
            "Send a downloaded Market model through the finishing pipeline so it matches the scale and budget of everything else. Returns the job.",
            schema(
                json!({"source": {"type": "string"}, "id": {"type": "string"}, "kind": {"type": "string", "enum": kinds(), "description": "Default prop."}, "height": {"type": "number", "description": "Metres; 0 keeps the model's own scale (default)."}, "wait_seconds": wait}),
                &["source", "id"],
            ),
            false,
            false,
        ),
    ]
}

pub fn exists(name: &str) -> bool {
    list().iter().any(|t| t["name"] == name)
}

fn text_result(v: &Value) -> Value {
    json!({"content": [{"type": "text", "text": serde_json::to_string_pretty(v).unwrap()}], "structuredContent": v, "isError": false})
}

pub fn call(name: &str, args: &Value, client: &str) -> Value {
    match dispatch(name, args, client) {
        Ok(r) => r,
        Err(e) => json!({"content": [{"type": "text", "text": format!("{e:#}")}], "isError": true}),
    }
}

fn s<'a>(args: &'a Value, k: &str) -> Option<&'a str> {
    args[k].as_str().filter(|s| !s.is_empty())
}

fn u(args: &Value, k: &str) -> Option<u32> {
    args[k].as_u64().map(|v| v as u32)
}

fn studio_url(id: &str) -> String {
    format!("{}#job={id}", mesh_core::config().url())
}

/// What an AI needs to know about a job, with absolute paths.
fn summary(job: &Job) -> Value {
    let d = job.dir();
    let results: Vec<Value> = job
        .results
        .iter()
        .map(|r| {
            json!({
                "variant": r.variant,
                "glb": d.join(&r.glb),
                "rigged_glb": r.rigged.as_ref().map(|p| d.join(p)),
                "review_sheet": d.join(&r.renders).join("sheet.jpg"),
                "tris": r.stats["tris"],
                "dimensions_m": r.stats["dimensions_m"],
                "warnings": r.stats["warnings"],
                "engine": r.meta["engine"],
                "animations": r.rig.as_ref().and_then(|g| g["clips"].as_array().map(|c| c.len())),
            })
        })
        .collect();
    json!({
        "id": job.id, "name": job.name, "status": job.status, "stage": job.stage, "error": job.error,
        "engine": job.engine, "kind": job.kind, "variants": job.variants, "height_m": job.height, "polys": job.polys,
        "texture": job.texture, "prep_notes": job.prep.as_ref().map(|p| p["notes"].clone()),
        "results": results, "log": job.log_tail(12), "studio": studio_url(&job.id), "folder": d,
        "credit": job.extra.get("licence").map(|l| json!({"licence": l, "author": job.extra.get("author"), "source": job.extra.get("source")})),
    })
}

fn wait(id: &str, secs: u64) -> Result<Job> {
    let until = Instant::now() + Duration::from_secs(secs.min(MAX_WAIT));
    loop {
        let j = jobs::load(id)?;
        if !j.busy() || Instant::now() >= until {
            return Ok(j);
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
}

fn job_reply(id: &str, args: &Value) -> Result<Value> {
    let j = wait(id, args["wait_seconds"].as_u64().unwrap_or(0))?;
    Ok(text_result(&summary(&j)))
}

fn default_engine() -> Option<String> {
    let all = mesh_engines::available();
    let m = all.as_object()?;
    m.iter().find(|(k, e)| e["ready"] == true && *k != "triposr").or_else(|| m.iter().find(|(_, e)| e["ready"] == true)).map(|(k, _)| k.clone())
}

fn dispatch(name: &str, args: &Value, client: &str) -> Result<Value> {
    match name {
        "mesh_status" => {
            let keys = mesh_core::keys::load();
            Ok(text_result(&json!({
                "engines": mesh_engines::available(),
                "kinds": mesh_core::presets::as_json(),
                "quality": mesh_core::presets::QUALITY,
                "keys_set": keys.keys().collect::<Vec<_>>(),
                "blender": mesh_blender::version(),
                "animations_installed": mesh_core::config().anim_libs().iter().all(|p| p.exists()),
                "starter_pack_items": mesh_market::starter::loaded(),
                "queue": mesh_pipeline::queued(),
                "studio": mesh_core::config().url(),
                "jobs_folder": mesh_core::config().jobs,
            })))
        }
        "preview_cutout" => {
            let src = resolve(s(args, "image").ok_or_else(|| anyhow!("image is required"))?, IMAGE_EXTS)?;
            let out = mesh_core::config().cache_dir.join("inbox").join(format!("cutout-{}.png", std::process::id()));
            let crop = parse_crop(args)?;
            let info = mesh_prep::prepare(&src, &out, mesh_prep::PrepOptions { crop, keep_background: false }, None)?;
            let data = b64_file(&out)?;
            Ok(json!({
                "content": [
                    {"type": "image", "data": data, "mimeType": "image/png"},
                    {"type": "text", "text": serde_json::to_string_pretty(&info)?},
                ],
                "structuredContent": info,
                "isError": false,
            }))
        }
        "generate_model" => {
            let src = resolve(s(args, "image").ok_or_else(|| anyhow!("image is required"))?, IMAGE_EXTS)?;
            let engine = match s(args, "engine") {
                Some(e) => e.to_string(),
                None => default_engine().ok_or_else(|| anyhow!("no engine is ready; see mesh_status for how to set one up"))?,
            };
            let e = mesh_engines::get(&engine).ok_or_else(|| anyhow!("unknown engine {engine}; see mesh_status"))?;
            if !e.ready(&mesh_core::keys::load()) {
                bail!("engine {engine} is not ready: {}", e.info().setup.unwrap_or_default());
            }
            let mut views = BTreeMap::new();
            for side in ["back", "left", "right"] {
                if let Some(v) = s(args, side) {
                    views.insert(side.to_string(), resolve(v, IMAGE_EXTS)?);
                }
            }
            let opt = NewJob {
                name: s(args, "name").map(str::to_string),
                kind: s(args, "kind").map(str::to_string),
                engine: Some(engine),
                variants: u(args, "variants"),
                quality: s(args, "quality").map(str::to_string),
                height: args["height"].as_f64(),
                polys: u(args, "polys"),
                texture: u(args, "texture"),
                origin: s(args, "origin").map(str::to_string),
                crop: parse_crop(args)?,
                pbr: args["pbr"].as_bool(),
                pose: s(args, "pose").map(str::to_string),
                seed: args["seed"].as_i64(),
                keep_background: args["keep_background"].as_bool(),
                delight: args["delight"].as_bool(),
                by: Some(client.to_string()),
                ..Default::default()
            };
            if opt.kind.as_deref().is_some_and(|k| mesh_core::preset(k).is_none()) {
                bail!("unknown kind; use one of {:?}", kinds());
            }
            let job = jobs::create(Some(&src), None, &views, opt)?;
            start(&job.id, Work::Run(job.id.clone()))?;
            job_reply(&job.id, args)
        }
        "import_model" => {
            let glb = resolve(s(args, "glb").ok_or_else(|| anyhow!("glb is required"))?, &["glb"])?;
            let opt = NewJob {
                name: s(args, "name").map(str::to_string),
                kind: s(args, "kind").map(str::to_string),
                height: args["height"].as_f64(),
                polys: u(args, "polys"),
                texture: u(args, "texture"),
                origin: s(args, "origin").map(str::to_string),
                keep_parts: Some(true),
                by: Some(client.to_string()),
                ..Default::default()
            };
            let job = jobs::create(None, Some(&glb), &BTreeMap::new(), opt)?;
            start(&job.id, Work::Run(job.id.clone()))?;
            job_reply(&job.id, args)
        }
        "get_job" => job_reply(&id(args)?, args),
        "list_jobs" => {
            let limit = args["limit"].as_u64().unwrap_or(20) as usize;
            let status = s(args, "status");
            let list: Vec<Value> = jobs::list()
                .iter()
                .filter(|j| status.is_none_or(|st| j.status == st))
                .take(limit)
                .map(|j| json!({"id": j.id, "name": j.name, "status": j.status, "engine": j.engine, "kind": j.kind, "variants": j.results.len(), "by": j.by}))
                .collect();
            Ok(text_result(&json!({"jobs": list})))
        }
        "view_model" => {
            let job = jobs::load(&id(args)?)?;
            let r = pick_variant(&job, u(args, "variant"))?;
            let view = s(args, "view").unwrap_or("sheet");
            if !VIEWS.contains(&view) {
                bail!("view must be one of {VIEWS:?}");
            }
            let renders = job.dir().join(&r.renders);
            let (file, mime) = if view == "sheet" { (renders.join("sheet.jpg"), "image/jpeg") } else { (renders.join(format!("{view}.png")), "image/png") };
            if !file.exists() {
                bail!("no {view} render for this job yet (status: {})", job.status);
            }
            emit(Event::Show { id: job.id.clone(), variant: Some(r.variant), mode: None, light: None });
            let stats =
                json!({"variant": r.variant, "tris": r.stats["tris"], "dimensions_m": r.stats["dimensions_m"], "warnings": r.stats["warnings"], "file": file});
            Ok(json!({
                "content": [{"type": "image", "data": b64_file(&file)?, "mimeType": mime}, {"type": "text", "text": serde_json::to_string_pretty(&stats)?}],
                "structuredContent": stats,
                "isError": false,
            }))
        }
        "show_in_studio" => {
            let job = jobs::load(&id(args)?)?;
            let mode = s(args, "mode").map(str::to_string);
            let light = s(args, "light").map(str::to_string);
            emit(Event::Show { id: job.id.clone(), variant: u(args, "variant"), mode, light });
            Ok(text_result(
                &json!({"shown": job.id, "studio": studio_url(&job.id), "note": "The user's studio now shows this model. You can't see that view; use view_model for the review renders."}),
            ))
        }
        "refinish_model" => {
            let jid = id(args)?;
            let job = jobs::load(&jid)?;
            if job.results.is_empty() {
                bail!("job has no finished variants to refinish");
            }
            let ch = Refinish {
                height: args["height"].as_f64(),
                polys: u(args, "polys"),
                texture: u(args, "texture"),
                rotate: args["rotate"].as_f64().map(|r| r + job.rotate),
                origin: s(args, "origin").map(str::to_string),
            };
            start(&jid, Work::Refinish(jid.clone(), ch))?;
            job_reply(&jid, args)
        }
        "rig_model" => {
            let jid = id(args)?;
            if jobs::load(&jid)?.results.is_empty() {
                bail!("job has no finished variants to rig");
            }
            start(&jid, Work::Rig(jid.clone()))?;
            job_reply(&jid, args)
        }
        "rerun_job" => {
            let jid = id(args)?;
            jobs::load(&jid)?;
            start(&jid, Work::Run(jid.clone()))?;
            job_reply(&jid, args)
        }
        "concept_art" => {
            let what = s(args, "description").ok_or_else(|| anyhow!("description is required"))?;
            let refs: Vec<PathBuf> =
                args["references"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|r| resolve(r, IMAGE_EXTS)).collect::<Result<_>>()?;
            let ref_paths: Vec<&std::path::Path> = refs.iter().map(|p| p.as_path()).collect();
            let out = mesh_core::config().data_dir.join("concepts").join(format!("{}-{}.png", chrono_stamp(), mesh_core::slug(what)));
            mesh_engines::concept::concept(what, &out, &ref_paths, s(args, "style"), args["seed"].as_i64(), &|_| {})?;
            let info = json!({"path": out, "next": "Pass this path as `image` to preview_cutout or generate_model (kind prop for objects)."});
            Ok(json!({
                "content": [{"type": "image", "data": b64_file(&out)?, "mimeType": "image/png"}, {"type": "text", "text": serde_json::to_string_pretty(&info)?}],
                "structuredContent": info,
                "isError": false,
            }))
        }
        "load_starter_pack" => Ok(text_result(&mesh_market::starter::load(&mut |_| {})?)),
        "market_search" => {
            let r = mesh_market::search(
                s(args, "source").unwrap_or("polyhaven"),
                s(args, "query").unwrap_or(""),
                s(args, "kind").unwrap_or("model"),
                u(args, "page").unwrap_or(0),
            )?;
            Ok(text_result(&r))
        }
        "market_grab" => Ok(text_result(&mesh_market::grab(&args["item"], s(args, "res").unwrap_or("2k"))?)),
        "market_library" => Ok(text_result(&json!({"assets": mesh_market::library()}))),
        "market_finish" => {
            let kind = s(args, "kind").unwrap_or("prop");
            if mesh_core::preset(kind).is_none() {
                bail!("unknown kind");
            }
            let job = mesh_market::import_to_studio(
                s(args, "source").unwrap_or(""),
                s(args, "id").unwrap_or(""),
                kind,
                args["height"].as_f64().unwrap_or(0.0),
                Some(client.to_string()),
            )?;
            start(&job.id, Work::Run(job.id.clone()))?;
            job_reply(&job.id, args)
        }
        _ => bail!("unknown tool {name}"),
    }
}

/// Queue work and point the studio at it so the user sees the AI's job straight away.
fn start(id: &str, w: Work) -> Result<()> {
    mesh_pipeline::submit(w)?;
    emit(Event::Show { id: id.to_string(), variant: None, mode: None, light: None });
    Ok(())
}

fn id(args: &Value) -> Result<String> {
    let id = s(args, "job_id").ok_or_else(|| anyhow!("job_id is required"))?;
    jobs::job_dir(id)?;
    Ok(id.to_string())
}

fn pick_variant(job: &Job, v: Option<u32>) -> Result<&mesh_core::JobResult> {
    match v {
        Some(v) => job.results.iter().find(|r| r.variant == v).ok_or_else(|| anyhow!("no variant {v}")),
        None => job.results.first().ok_or_else(|| anyhow!("job has no finished variants yet (status: {}, stage: {})", job.status, job.stage)),
    }
}

fn parse_crop(args: &Value) -> Result<Option<[f64; 4]>> {
    match args["crop"].as_array() {
        None => Ok(None),
        Some(a) if a.len() == 4 => {
            let v: Vec<f64> = a.iter().map(|x| x.as_f64().unwrap_or(-1.0)).collect();
            if v.iter().any(|x| !(0.0..=1.0).contains(x)) || v[2] <= v[0] || v[3] <= v[1] {
                bail!("crop must be [x0, y0, x1, y1] fractions with x1 > x0 and y1 > y0");
            }
            Ok(Some([v[0], v[1], v[2], v[3]]))
        }
        Some(_) => bail!("crop needs four numbers"),
    }
}

fn chrono_stamp() -> String {
    format!("{}", mesh_core::now() as u64)
}
