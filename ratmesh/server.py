"""RatLabs Mesh studio: a local web UI over the job pipeline. Binds to 127.0.0.1 only."""
import json
import os
import queue
import tempfile
import threading
from pathlib import Path

from fastapi import FastAPI, File, Form, HTTPException, UploadFile
from fastapi.responses import FileResponse, Response
from fastapi.staticfiles import StaticFiles

from . import engines, jobs, market, prep
from .config import JOBS, KEYS_FILE, PRESETS, ROOT, load_keys

app = FastAPI(title="RatLabs Mesh")
work = queue.Queue()
IMG_TYPES = {".png", ".jpg", ".jpeg", ".webp"}


def worker():
    while True:
        kind, jid, kw = work.get()
        try:
            {"run": jobs.run, "rig": jobs.rig}.get(kind, jobs.refinish)(jid, **kw)
        finally:
            work.task_done()


def _save_upload(up, d):
    ext = Path(up.filename or "x.png").suffix.lower()
    if ext not in IMG_TYPES:
        raise HTTPException(400, f"unsupported image type {ext}")
    p = Path(d) / ("upload" + ext)
    p.write_bytes(up.file.read())
    return p


@app.get("/api/config")
def config():
    return {"engines": engines.available(), "presets": PRESETS, "quality": list(engines.QUALITY),
            "keys": {k: bool(v) for k, v in load_keys().items()}, "queue": work.qsize()}


@app.post("/api/keys")
def set_keys(FAL_KEY: str = Form(""), MESHY_API_KEY: str = Form(""), HF_TOKEN: str = Form(""),
             SKETCHFAB_TOKEN: str = Form(""), POLYPIZZA_KEY: str = Form("")):
    keys = load_keys() if KEYS_FILE.exists() else {}
    for k, v in (("FAL_KEY", FAL_KEY), ("MESHY_API_KEY", MESHY_API_KEY), ("HF_TOKEN", HF_TOKEN),
                 ("SKETCHFAB_TOKEN", SKETCHFAB_TOKEN), ("POLYPIZZA_KEY", POLYPIZZA_KEY)):
        if v.strip():
            keys[k] = v.strip()
    KEYS_FILE.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(KEYS_FILE, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        f.write("".join(f"{k}={v}\n" for k, v in keys.items()))
    return {"saved": sorted(keys)}


@app.post("/api/cutout")
def cutout(image: UploadFile = File(...), crop: str = Form("")):
    """Preview the prep stage so a bad cut-out never reaches a paid GPU."""
    with tempfile.TemporaryDirectory() as d:
        src = _save_upload(image, d)
        out = Path(d) / "prep.png"
        try:
            info = prep.prepare(src, out, crop=json.loads(crop) if crop else None)
        except ValueError as e:
            raise HTTPException(422, str(e))
        return Response(out.read_bytes(), media_type="image/png", headers={"X-Prep": json.dumps(info)})


@app.get("/api/jobs")
def list_jobs():
    return jobs.list_jobs()


@app.get("/api/jobs/{jid}")
def get_job(jid: str):
    try:
        job = jobs.load(jid)
    except (FileNotFoundError, ValueError):
        raise HTTPException(404)
    log = jobs.job_dir(jid) / "job.log"
    job["log"] = log.read_text().splitlines()[-30:] if log.exists() else []
    return job


@app.post("/api/jobs")
def create_job(image: UploadFile = File(None), glb: UploadFile = File(None), back: UploadFile = File(None),
               left: UploadFile = File(None), right: UploadFile = File(None), options: str = Form("{}")):
    opt = json.loads(options)
    allowed = {"name", "kind", "engine", "variants", "quality", "height", "polys", "texture", "origin",
               "crop", "rotate", "pbr", "pose", "seed", "keep_background", "delight"}
    opt = {k: v for k, v in opt.items() if k in allowed and v not in ("", None)}
    if opt.get("kind", "character") not in PRESETS or opt.get("engine", "trellis2") not in engines.ENGINES:
        raise HTTPException(400, "unknown kind or engine")
    with tempfile.TemporaryDirectory() as d:
        src = g = None
        if glb is not None and glb.filename:
            g = Path(d) / "in.glb"
            g.write_bytes(glb.file.read())
        elif image is not None and image.filename:
            src = _save_upload(image, d)
        else:
            raise HTTPException(400, "send an image or a glb")
        views = {}
        for side, up in (("back", back), ("left", left), ("right", right)):
            if up is not None and up.filename:
                sd = Path(d) / side
                sd.mkdir()
                views[side] = _save_upload(up, sd)
        if not opt.get("name"):
            opt["name"] = Path((glb if g else image).filename).stem
        job = jobs.create(src, glb=g, views=views, **opt)
    work.put(("run", job["id"], {}))
    return job


@app.post("/api/jobs/{jid}/refinish")
def refinish(jid: str, options: str = Form("{}")):
    opt = {k: v for k, v in json.loads(options).items() if k in ("height", "polys", "texture", "rotate", "origin")}
    jobs.load(jid)
    work.put(("refinish", jid, opt))
    return {"queued": jid}


@app.post("/api/jobs/{jid}/rig")
def rig(jid: str):
    jobs.load(jid)
    work.put(("rig", jid, {}))
    return {"queued": jid}


@app.post("/api/jobs/{jid}/rerun")
def rerun(jid: str):
    jobs.load(jid)
    work.put(("run", jid, {}))
    return {"queued": jid}


# ---------- market ----------

@app.get("/api/market/sources")
def market_sources():
    return market.sources()


@app.get("/api/market/search")
def market_search(source: str, kind: str = "model", q: str = "", page: int = 0):
    if source not in market.SOURCES:
        raise HTTPException(404, "unknown source")
    try:
        return market.search(source, q, kind, page)
    except market.MarketError as e:
        raise HTTPException(400, str(e))


@app.post("/api/market/grab")
def market_grab(item: str = Form(...), res: str = Form("2k")):
    it = json.loads(item)
    if it.get("source") not in market.SOURCES or it.get("kind") not in market.KINDS:
        raise HTTPException(400, "bad item")
    try:
        return market.grab(it, res)
    except (market.MarketError, KeyError) as e:
        raise HTTPException(400, str(e))


@app.get("/api/market/library")
def market_library():
    return market.library()


@app.post("/api/market/import")
def market_import(source: str = Form(...), id: str = Form(...), kind: str = Form("prop"), height: float = Form(0)):
    if kind not in PRESETS:
        raise HTTPException(400, "bad kind")
    try:
        job = market.import_to_studio(source, id, kind, height)
    except (market.MarketError, FileNotFoundError) as e:
        raise HTTPException(400, str(e))
    work.put(("run", job["id"], {}))
    return job


@app.delete("/api/market/library/{source}/{id}")
def market_remove(source: str, id: str):
    market.remove(source, id)
    return {"removed": id}


@app.get("/library/{path:path}")
def library_files(path: str):
    p = (market.LIB / path).resolve()
    if market.LIB.resolve() not in p.parents or not p.is_file():
        raise HTTPException(404)
    return FileResponse(p)


@app.get("/files/{jid}/{path:path}")
def files(jid: str, path: str):
    d = jobs.job_dir(jid)
    p = (d / path).resolve()
    if d not in p.parents or not p.is_file():
        raise HTTPException(404)
    return FileResponse(p)


app.mount("/", StaticFiles(directory=ROOT / "web", html=True), name="web")


def serve(port=8771):
    import uvicorn
    JOBS.mkdir(parents=True, exist_ok=True)
    threading.Thread(target=worker, daemon=True).start()
    print(f"RatLabs Mesh on http://127.0.0.1:{port}")
    uvicorn.run(app, host="127.0.0.1", port=port, log_level="warning")
