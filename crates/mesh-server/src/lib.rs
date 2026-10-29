//! RatLabs Mesh studio server: the web UI, its JSON API, a live event stream, and the MCP
//! endpoint AI clients connect to. Binds to 127.0.0.1 only, and refuses requests whose
//! Host or Origin isn't this machine (no DNS-rebinding drive-bys from web pages).
use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path as UrlPath, Query, Request};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as Sse, KeepAlive};
use axum::response::{IntoResponse, Response, Sse as SseResponse};
use axum::routing::{delete, get, post};
use mesh_core::{Job, NewJob, jobs};
use mesh_pipeline::{Refinish, Work};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::services::ServeFile;

mod connect;

#[derive(rust_embed::Embed)]
#[folder = "../../web"]
struct Web;

const IMG_TYPES: &[&str] = &["png", "jpg", "jpeg", "webp"];

// ---------- errors: FastAPI's {"detail": ...} shape, which the UI reads ----------

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, axum::Json(json!({"detail": self.1}))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        let user = e.downcast_ref::<mesh_market::MarketError>().is_some() || e.downcast_ref::<mesh_engines::EngineError>().is_some();
        let code = if e.to_string().starts_with("no job") || e.to_string() == "bad job id" {
            StatusCode::NOT_FOUND
        } else if user {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::UNPROCESSABLE_ENTITY
        };
        ApiError(code, format!("{e:#}"))
    }
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

type ApiResult<T> = Result<T, ApiError>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> ApiResult<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?.map_err(ApiError::from)
}

// ---------- multipart ----------

/// A parsed multipart form: text fields, plus uploaded files saved into a temp folder.
struct Form {
    fields: BTreeMap<String, String>,
    files: BTreeMap<String, PathBuf>,
    _dir: TempDir,
}

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

static UPLOAD_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

async fn form(mut mp: Multipart) -> ApiResult<Form> {
    let n = UPLOAD_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = mesh_core::config().cache_dir.join("uploads").join(format!("{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| bad(e.to_string()))?;
    let mut f = Form { fields: BTreeMap::new(), files: BTreeMap::new(), _dir: TempDir(dir.clone()) };
    while let Some(field) = mp.next_field().await.map_err(|e| bad(e.to_string()))? {
        let name = field.name().unwrap_or("").to_string();
        match field.file_name().map(str::to_string) {
            Some(fname) if !fname.is_empty() => {
                let ext = Path::new(&fname).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                let data = field.bytes().await.map_err(|e| bad(e.to_string()))?;
                let sub = dir.join(&name);
                std::fs::create_dir_all(&sub).map_err(|e| bad(e.to_string()))?;
                // keep the original stem so job names default to the file name
                let stem = Path::new(&fname).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "upload".into());
                let safe: String = stem.chars().map(|c| if c.is_alphanumeric() || "-_ .".contains(c) { c } else { '_' }).collect();
                let p = sub.join(format!("{safe}.{ext}"));
                std::fs::write(&p, data).map_err(|e| bad(e.to_string()))?;
                f.files.insert(name, p);
            }
            _ => {
                let text = field.text().await.map_err(|e| bad(e.to_string()))?;
                f.fields.insert(name, text);
            }
        }
    }
    Ok(f)
}

fn image_file(f: &Form, name: &str) -> ApiResult<Option<PathBuf>> {
    match f.files.get(name) {
        None => Ok(None),
        Some(p) => {
            let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !IMG_TYPES.contains(&ext.as_str()) {
                return Err(bad(format!("unsupported image type .{ext}")));
            }
            Ok(Some(p.clone()))
        }
    }
}

// ---------- config / keys ----------

async fn config() -> ApiResult<axum::Json<Value>> {
    blocking(|| {
        let keys: serde_json::Map<String, Value> = mesh_core::keys::load().into_iter().map(|(k, v)| (k, Value::Bool(!v.is_empty()))).collect();
        Ok(axum::Json(json!({
            "engines": mesh_engines::available(),
            "presets": mesh_core::presets::as_json(),
            "quality": mesh_core::presets::QUALITY,
            "keys": keys,
            "queue": mesh_pipeline::queued(),
            "version": env!("CARGO_PKG_VERSION"),
            "blender": mesh_blender::version(),
            "starter": {"loaded": mesh_market::starter::loaded(), "about": mesh_market::starter::ABOUT},
        })))
    })
    .await
}

async fn set_keys(mp: Multipart) -> ApiResult<axum::Json<Value>> {
    let f = form(mp).await?;
    let saved = mesh_core::keys::save(&f.fields).map_err(|e| bad(e.to_string()))?;
    mesh_core::emit(mesh_core::Event::Config);
    Ok(axum::Json(json!({"saved": saved})))
}

async fn connect_info() -> axum::Json<Value> {
    axum::Json(connect::info())
}

// ---------- cut-out preview ----------

async fn cutout(mp: Multipart) -> ApiResult<Response> {
    let f = form(mp).await?;
    let src = image_file(&f, "image")?.ok_or_else(|| bad("send an image"))?;
    let crop: Option<[f64; 4]> = match f.fields.get("crop").filter(|c| !c.is_empty()) {
        Some(c) => Some(serde_json::from_str(c).map_err(|_| bad("bad crop"))?),
        None => None,
    };
    let out = src.with_file_name("prep.png");
    let o = out.clone();
    let info = blocking(move || mesh_prep::prepare(&src, &o, mesh_prep::PrepOptions { crop, keep_background: false }, None)).await?;
    let bytes = std::fs::read(&out).map_err(|e| bad(e.to_string()))?;
    drop(f);
    let mut r = Response::new(Body::from(bytes));
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
    r.headers_mut().insert("X-Prep", HeaderValue::from_str(&serde_json::to_string(&info).unwrap()).unwrap());
    Ok(r)
}

// ---------- jobs ----------

async fn list_jobs() -> ApiResult<axum::Json<Vec<Job>>> {
    blocking(|| Ok(axum::Json(jobs::list()))).await
}

async fn get_job(UrlPath(id): UrlPath<String>) -> ApiResult<axum::Json<Value>> {
    blocking(move || {
        let job = jobs::load(&id)?;
        let mut v = serde_json::to_value(&job)?;
        v["log"] = json!(job.log_tail(30));
        Ok(axum::Json(v))
    })
    .await
}

const CREATE_FIELDS: &[&str] = &[
    "name",
    "kind",
    "engine",
    "variants",
    "quality",
    "height",
    "polys",
    "texture",
    "origin",
    "crop",
    "rotate",
    "pbr",
    "pose",
    "seed",
    "keep_background",
    "delight",
];

fn options(f: &Form, allowed: &[&str]) -> ApiResult<Value> {
    let raw: Value = serde_json::from_str(f.fields.get("options").map(String::as_str).unwrap_or("{}")).map_err(|_| bad("options must be JSON"))?;
    let mut out = serde_json::Map::new();
    for (k, v) in raw.as_object().into_iter().flatten() {
        if allowed.contains(&k.as_str()) && !v.is_null() && v != "" {
            out.insert(k.clone(), v.clone());
        }
    }
    Ok(Value::Object(out))
}

async fn create_job(mp: Multipart) -> ApiResult<axum::Json<Job>> {
    let f = form(mp).await?;
    let opt = options(&f, CREATE_FIELDS)?;
    let mut new: NewJob = serde_json::from_value(opt).map_err(|e| bad(format!("bad options: {e}")))?;
    if new.kind.as_deref().is_some_and(|k| mesh_core::preset(k).is_none()) || new.engine.as_deref().is_some_and(|e| mesh_engines::get(e).is_none()) {
        return Err(bad("unknown kind or engine"));
    }
    new.by = Some("studio".into());
    let glb = f.files.get("glb").cloned();
    let image = if glb.is_none() { image_file(&f, "image")? } else { None };
    if glb.is_none() && image.is_none() {
        return Err(bad("send an image or a glb"));
    }
    let mut views = BTreeMap::new();
    for side in ["back", "left", "right"] {
        if let Some(p) = image_file(&f, side)? {
            views.insert(side.to_string(), p);
        }
    }
    let job = blocking(move || {
        let job = jobs::create(image.as_deref(), glb.as_deref(), &views, new)?;
        drop(f);
        mesh_pipeline::submit(Work::Run(job.id.clone()))?;
        Ok(job)
    })
    .await?;
    Ok(axum::Json(job))
}

async fn refinish(UrlPath(id): UrlPath<String>, mp: Multipart) -> ApiResult<axum::Json<Value>> {
    let f = form(mp).await?;
    let ch: Refinish = serde_json::from_value(options(&f, &["height", "polys", "texture", "rotate", "origin"])?).map_err(|e| bad(e.to_string()))?;
    let jid = id.clone();
    blocking(move || mesh_pipeline::submit(Work::Refinish(jid.clone(), ch))).await?;
    Ok(axum::Json(json!({"queued": id})))
}

async fn rig(UrlPath(id): UrlPath<String>) -> ApiResult<axum::Json<Value>> {
    let jid = id.clone();
    blocking(move || mesh_pipeline::submit(Work::Rig(jid))).await?;
    Ok(axum::Json(json!({"queued": id})))
}

async fn rerun(UrlPath(id): UrlPath<String>) -> ApiResult<axum::Json<Value>> {
    let jid = id.clone();
    blocking(move || mesh_pipeline::submit(Work::Run(jid))).await?;
    Ok(axum::Json(json!({"queued": id})))
}

// ---------- market ----------

async fn load_starter() -> ApiResult<axum::Json<Value>> {
    blocking(|| Ok(axum::Json(mesh_market::starter::load(&mut |_| {})?))).await
}

async fn market_sources() -> ApiResult<axum::Json<Value>> {
    blocking(|| Ok(axum::Json(mesh_market::sources()))).await
}

#[derive(Deserialize)]
struct SearchQ {
    source: String,
    #[serde(default = "model")]
    kind: String,
    #[serde(default)]
    q: String,
    #[serde(default)]
    page: u32,
}
fn model() -> String {
    "model".into()
}

async fn market_search(Query(q): Query<SearchQ>) -> ApiResult<axum::Json<Value>> {
    if mesh_market::source(&q.source).is_none() {
        return Err(ApiError(StatusCode::NOT_FOUND, "unknown source".into()));
    }
    blocking(move || Ok(axum::Json(mesh_market::search(&q.source, &q.q, &q.kind, q.page)?))).await
}

async fn market_grab(mp: Multipart) -> ApiResult<axum::Json<Value>> {
    let f = form(mp).await?;
    let item: Value = serde_json::from_str(f.fields.get("item").map(String::as_str).unwrap_or("")).map_err(|_| bad("bad item"))?;
    let res = f.fields.get("res").cloned().unwrap_or_else(|| "2k".into());
    blocking(move || Ok(axum::Json(mesh_market::grab(&item, &res)?))).await
}

async fn market_library() -> ApiResult<axum::Json<Vec<Value>>> {
    blocking(|| Ok(axum::Json(mesh_market::library()))).await
}

async fn market_import(mp: Multipart) -> ApiResult<axum::Json<Job>> {
    let f = form(mp).await?;
    let g = |k: &str| f.fields.get(k).cloned().unwrap_or_default();
    let (source, id, kind) = (g("source"), g("id"), f.fields.get("kind").cloned().unwrap_or_else(|| "prop".into()));
    let height: f64 = g("height").parse().unwrap_or(0.0);
    if mesh_core::preset(&kind).is_none() {
        return Err(bad("bad kind"));
    }
    let job = blocking(move || {
        let job = mesh_market::import_to_studio(&source, &id, &kind, height, Some("studio".into()))?;
        mesh_pipeline::submit(Work::Run(job.id.clone()))?;
        Ok(job)
    })
    .await?;
    Ok(axum::Json(job))
}

async fn market_remove(UrlPath((source, id)): UrlPath<(String, String)>) -> ApiResult<axum::Json<Value>> {
    let i = id.clone();
    blocking(move || mesh_market::remove(&source, &i)).await?;
    Ok(axum::Json(json!({"removed": id})))
}

// ---------- files ----------

/// Serve `rel` from inside `root` only.
async fn serve_inside(root: PathBuf, rel: &str, req: Request) -> Response {
    let Ok(root) = root.canonicalize() else { return StatusCode::NOT_FOUND.into_response() };
    let Ok(p) = root.join(rel).canonicalize() else { return StatusCode::NOT_FOUND.into_response() };
    if !p.starts_with(&root) || !p.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match ServeFile::new(p).try_call(req).await {
        Ok(r) => r.map(Body::new).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn job_files(UrlPath((id, path)): UrlPath<(String, String)>, req: Request) -> Response {
    match jobs::job_dir(&id) {
        Ok(d) => serve_inside(d, &path, req).await,
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn library_files(UrlPath(path): UrlPath<String>, req: Request) -> Response {
    serve_inside(mesh_market::lib_dir().to_path_buf(), &path, req).await
}

async fn static_files(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Web::get(path) {
        Some(f) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref().to_string()), (header::CACHE_CONTROL, "no-cache".to_string())], f.data.into_owned()).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

// ---------- live events ----------

async fn events() -> SseResponse<impl tokio_stream::Stream<Item = Result<Sse, Infallible>>> {
    // A first event right away, so browsers report the stream open without waiting for the keep-alive.
    let hello = tokio_stream::once(Ok(Sse::default().data(r#"{"type":"hello"}"#)));
    let stream = hello
        .chain(BroadcastStream::new(mesh_core::subscribe()).filter_map(|ev| ev.ok()).map(|ev| Ok(Sse::default().data(serde_json::to_string(&ev).unwrap()))));
    SseResponse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

// ---------- MCP over streamable HTTP ----------

static SESSIONS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

async fn mcp_post(headers: HeaderMap, body: axum::body::Bytes) -> Response {
    let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
        return (StatusCode::BAD_REQUEST, axum::Json(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}})))
            .into_response();
    };
    let session = headers.get("mcp-session-id").and_then(|v| v.to_str().ok()).map(str::to_string);
    let first = msg.as_array().and_then(|a| a.first()).unwrap_or(&msg);
    let new_client = mesh_mcp::client_name(first);
    let (client, new_session) = {
        let mut g = SESSIONS.lock().unwrap();
        let map = g.get_or_insert_with(HashMap::new);
        match &new_client {
            Some(name) => {
                let sid = format!("{:x}{:x}", std::process::id(), (mesh_core::now() * 1e6) as u64);
                map.insert(sid.clone(), name.clone());
                (name.clone(), Some(sid))
            }
            None => (
                session
                    .as_ref()
                    .and_then(|s| map.get(s).cloned())
                    .or_else(|| headers.get("x-ratmesh-client").and_then(|v| v.to_str().ok()).map(str::to_string))
                    .unwrap_or_else(|| "AI client".into()),
                None,
            ),
        }
    };
    let reply = tokio::task::spawn_blocking(move || mesh_mcp::handle(&msg, &client)).await.ok().flatten();
    let mut r = match reply {
        Some(v) => axum::Json(v).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    };
    if let Some(sid) = new_session {
        r.headers_mut().insert("mcp-session-id", HeaderValue::from_str(&sid).unwrap());
    }
    r
}

async fn mcp_delete(headers: HeaderMap) -> StatusCode {
    if let Some(s) = headers.get("mcp-session-id").and_then(|v| v.to_str().ok())
        && let Some(map) = SESSIONS.lock().unwrap().as_mut()
    {
        map.remove(s);
    }
    StatusCode::OK
}

// ---------- local-only guard ----------

fn local_host(h: &str, port: u16) -> bool {
    let h = h.trim();
    [format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")].iter().any(|ok| ok == h)
}

async fn local_only(req: Request, next: Next) -> Response {
    let port = mesh_core::config().port;
    let host_ok = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).is_some_and(|h| local_host(h, port));
    let origin_ok = match req.headers().get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
        None => true,
        Some(o) => o.strip_prefix("http://").is_some_and(|h| local_host(h, port)),
    };
    if !host_ok || !origin_ok {
        return (StatusCode::FORBIDDEN, "RatLabs Mesh only answers this machine").into_response();
    }
    next.run(req).await
}

pub fn router() -> Router {
    Router::new()
        .route("/api/config", get(config))
        .route("/api/keys", post(set_keys))
        .route("/api/connect", get(connect_info))
        .route("/api/cutout", post(cutout))
        .route("/api/jobs", get(list_jobs).post(create_job))
        .route("/api/jobs/{id}", get(get_job))
        .route("/api/jobs/{id}/refinish", post(refinish))
        .route("/api/jobs/{id}/rig", post(rig))
        .route("/api/jobs/{id}/rerun", post(rerun))
        .route("/api/events", get(events))
        .route("/api/starter", post(load_starter))
        .route("/api/market/sources", get(market_sources))
        .route("/api/market/search", get(market_search))
        .route("/api/market/grab", post(market_grab))
        .route("/api/market/library", get(market_library))
        .route("/api/market/import", post(market_import))
        .route("/api/market/library/{source}/{id}", delete(market_remove))
        .route("/library/{*path}", get(library_files))
        .route("/files/{id}/{*path}", get(job_files))
        .route("/mcp", post(mcp_post).delete(mcp_delete).get(|| async { StatusCode::METHOD_NOT_ALLOWED }))
        .fallback(static_files)
        .layer(DefaultBodyLimit::max(512 * 1024 * 1024))
        .layer(middleware::from_fn(local_only))
}

/// Run the studio until Ctrl-C.
pub async fn serve(port: u16) -> anyhow::Result<()> {
    let c = mesh_core::config();
    std::fs::create_dir_all(&c.jobs)?;
    // Jobs left "running" by a server that stopped never finish; say so instead of spinning forever.
    for mut j in jobs::list().into_iter().filter(|j| j.busy()) {
        j.status = "failed".into();
        j.error = Some("interrupted: the studio stopped while this ran. Run it again.".into());
        j.save().ok();
    }
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    println!("RatLabs Mesh on http://127.0.0.1:{port}/   (MCP: http://127.0.0.1:{port}/mcp)");
    axum::serve(listener, router())
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;
    Ok(())
}
