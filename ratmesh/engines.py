"""Stage 2: the GPU step. Each engine turns a prepared cut-out into a raw textured GLB.

No GPU on this machine, so every high-quality engine runs remotely:
  trellis2   Microsoft TRELLIS.2 (MIT, open weights) on fal.ai      FAL_KEY
  hunyuan3   Tencent Hunyuan3D v3 on fal.ai; takes back/left/right  FAL_KEY
  meshy      Meshy's own API (meshy-7.1 / latest)                   MESHY_API_KEY
  trellis2-hf  TRELLIS.2 on the official Hugging Face Space        HF_TOKEN (PRO quota)
  hunyuan-local  Tencent Hunyuan3D-2 shape + paint on this CPU. Free, offline, minutes per model.
  triposr    TripoSR on this CPU. Free and offline, coarse: drafts only.
"""
import base64
import fcntl
import json
import subprocess
import time
from pathlib import Path

import httpx

from .config import HUNYUAN, ROOT, TRIPOSR, load_keys

QUALITY = {"draft": 512, "standard": 1024, "high": 1536}
LOCAL_LOCK = Path.home() / ".cache/ratlabs-mesh-local.lock"


class EngineError(RuntimeError):
    pass


def data_uri(path):
    return "data:image/png;base64," + base64.b64encode(Path(path).read_bytes()).decode()


def download(url, out):
    with httpx.stream("GET", url, timeout=300, follow_redirects=True) as r:
        r.raise_for_status()
        with open(out, "wb") as f:
            for chunk in r.iter_bytes():
                f.write(chunk)


def _need(key):
    keys = load_keys()
    if not keys.get(key):
        raise EngineError(f"{key} is not set. Add it to ~/.config/ratlabs-mesh/keys.env")
    return keys[key]


# ---------- fal.ai queue ----------

def fal_url(path):
    """Upload an image to fal's CDN and return its URL. Some fal models (Hunyuan3D v3)
    hand the image to another provider that cannot read data: URIs."""
    headers = {"Authorization": "Key " + _need("FAL_KEY")}
    with httpx.Client(timeout=120, headers=headers) as c:
        r = c.post("https://rest.alpha.fal.ai/storage/upload/initiate?storage_type=fal-cdn-v3",
                   json={"content_type": "image/png", "file_name": Path(path).name})
        if r.status_code >= 400:
            raise EngineError(f"fal upload {r.status_code}: {r.text[:400]}")
        j = r.json()
    with httpx.Client(timeout=300) as c:
        u = c.put(j["upload_url"], content=Path(path).read_bytes(), headers={"Content-Type": "image/png"})
        if u.status_code >= 400:
            raise EngineError(f"fal upload {u.status_code}: {u.text[:400]}")
    return j["file_url"]


def _fal(endpoint, payload, log, poll=3):
    headers = {"Authorization": "Key " + _need("FAL_KEY")}
    with httpx.Client(timeout=120, headers=headers) as c:
        r = c.post(f"https://queue.fal.run/{endpoint}", json=payload)
        if r.status_code >= 400:
            raise EngineError(f"fal submit {r.status_code}: {r.text[:400]}")
        job = r.json()
        log(f"fal queued {job.get('request_id')}")
        last = None
        while True:
            s = c.get(job["status_url"], params={"logs": 1}).json()
            st = s.get("status")
            if st != last:
                log(f"fal {st.lower()}" + (f" (position {s['queue_position']})" if s.get("queue_position") is not None else ""))
                last = st
            if st == "COMPLETED":
                break
            if st not in ("IN_QUEUE", "IN_PROGRESS"):
                raise EngineError(f"fal status {s}")
            time.sleep(poll)
        res = c.get(job["response_url"])
        if res.status_code >= 400:
            raise EngineError(f"fal result {res.status_code}: {res.text[:400]}")
        return res.json()


def trellis2(img, out, opt, log):
    res = QUALITY[opt.get("quality", "standard")]
    payload = {
        "image_url": fal_url(img),
        "resolution": str(res),
        "seed": int(opt.get("seed", 0)),
        # TRELLIS bakes the texture after decimating, which beats decimating a baked mesh.
        # Its target counts vertices, but ragged cloth and many parts give well under 2 tris
        # per vertex (15k verts came back as 13.7k tris), so ask for the full budget and let
        # the finisher trim.
        "decimation_target": max(20000, int(opt["polys"])),
        "texture_size": str(min(4096, max(1024, int(opt["texture"])))),
        "remesh": True,
    }
    r = _fal("fal-ai/trellis-2", payload, log)
    download(r["model_glb"]["url"], out)
    return {"engine": "trellis2", "host": "fal.ai", "resolution": res, "seed": payload["seed"]}


def hunyuan3(img, out, opt, log):
    payload = {
        "input_image_url": fal_url(img),
        "enable_pbr": bool(opt.get("pbr", True)),
        # face_count is billed as an extra; the default 500k mesh is what we want anyway,
        # since the finisher decimates and bakes the surplus detail into a normal map.
        "generate_type": "Normal",
    }
    for side in ("back", "left", "right"):
        if opt.get("views", {}).get(side):
            payload[f"{side}_image_url"] = fal_url(opt["views"][side])
    r = _fal("fal-ai/hunyuan3d-v3/image-to-3d", payload, log)
    download((r.get("model_glb") or r["model_urls"]["glb"])["url"], out)
    return {"engine": "hunyuan3", "host": "fal.ai", "views": sorted(opt.get("views", {}))}


# ---------- Meshy ----------

def meshy(img, out, opt, log):
    headers = {"Authorization": "Bearer " + _need("MESHY_API_KEY")}
    views = [img] + [opt["views"][k] for k in ("back", "left", "right") if opt.get("views", {}).get(k)]
    body = {
        "ai_model": "latest",
        "should_remesh": True,
        "topology": "triangle",
        "target_polycount": min(300000, int(opt["polys"])),
        "should_texture": True,
        "enable_pbr": bool(opt.get("pbr", True)),
        "texture_resolution": "4k" if int(opt["texture"]) >= 4096 else "2k",
        "target_formats": ["glb"],
        "remove_lighting": bool(opt.get("delight", True)),
    }
    if opt.get("pose"):
        body["pose_mode"] = opt["pose"]
    if len(views) > 1:
        path, body["image_urls"] = "multi-image-to-3d", [data_uri(v) for v in views[:4]]
    else:
        path, body["image_url"] = "image-to-3d", data_uri(img)
    with httpx.Client(base_url="https://api.meshy.ai/openapi/v1", headers=headers, timeout=120) as c:
        r = c.post(f"/{path}", json=body)
        if r.status_code >= 400:
            raise EngineError(f"meshy {r.status_code}: {r.text[:400]}")
        tid = r.json()["result"]
        log(f"meshy task {tid}")
        last = -1
        while True:
            t = c.get(f"/{path}/{tid}").json()
            if t.get("progress") != last:
                log(f"meshy {t['status'].lower()} {t.get('progress', 0)}%")
                last = t.get("progress")
            if t["status"] == "SUCCEEDED":
                break
            if t["status"] in ("FAILED", "CANCELED"):
                raise EngineError(f"meshy {t['status']}: {t.get('task_error')}")
            time.sleep(5)
    download(t["model_urls"]["glb"], out)
    return {"engine": "meshy", "task": tid, "credits": t.get("consumed_credits")}


# ---------- Hugging Face Space ----------

def trellis2_hf(img, out, opt, log):
    from gradio_client import Client, handle_file
    c = Client("microsoft/TRELLIS.2", hf_token=_need("HF_TOKEN"), verbose=False)
    c.predict(api_name="/start_session")
    log("space: preprocessing")
    pre = c.predict(handle_file(str(img)), api_name="/preprocess_image")
    log("space: generating")
    res = QUALITY[opt.get("quality", "standard")]
    c.predict(handle_file(pre), int(opt.get("seed", 0)), str(res), api_name="/image_to_3d")
    log("space: extracting GLB")
    g = c.predict(max(10000, int(opt["polys"]) // 2), min(4096, int(opt["texture"])), api_name="/extract_glb")
    Path(out).write_bytes(Path(g[0]).read_bytes())
    return {"engine": "trellis2-hf", "host": "huggingface", "resolution": res}


# ---------- local CPU ----------

def triposr(img, out, opt, log):
    py = TRIPOSR / ".venv/bin/python"
    if not py.exists():
        raise EngineError(f"TripoSR not installed at {TRIPOSR}")
    res = {"draft": 160, "standard": 224, "high": 288}[opt.get("quality", "standard")]
    cmd = [str(py), "-I", str(ROOT / "ratmesh/triposr_cpu.py"), str(img), str(out), "--engine", str(TRIPOSR), "--resolution", str(res)]
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    for line in p.stdout:
        if line.startswith("{"):
            log("triposr " + json.loads(line).get("stage", ""))
    if p.wait():
        raise EngineError("TripoSR failed; see the job log")
    return {"engine": "triposr", "host": "local-cpu", "resolution": res}


def hunyuan_local(img, out, opt, log):
    py = HUNYUAN / ".venv/bin/python"
    if not py.exists():
        raise EngineError(f"Hunyuan3D-2 not installed at {HUNYUAN}; run local/setup_hunyuan.sh")
    q = opt.get("quality", "standard")
    # The 1.1B model at 320 keeps fur strands and whiskers the 0.6B mini loses, at about the same CPU time.
    model, octree = {"draft": ("mini", 256), "standard": ("full", 320), "high": ("full", 380)}[q]
    cmd = [str(py), "-I", str(ROOT / "ratmesh/hunyuan_local.py"), str(img), str(out), "--engine", str(HUNYUAN),
           "--model", model, "--octree", str(octree), "--faces", str(int(opt["polys"])), "--seed", str(int(opt.get("seed", 0)))]
    for side, path in opt.get("views", {}).items():
        cmd += ["--" + side, str(path)]
    if not opt.get("delight", True):
        cmd.append("--keep-lighting")
    if opt.get("detail_source"):
        cmd += ["--detail-source", opt["detail_source"]]
    # One local job per machine: each needs ~16 GB, and two at once exhaust 30 GB of RAM.
    # A file lock covers studio jobs and CLI jobs alike.
    with open(LOCAL_LOCK, "w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            log("waiting for another local job to finish")
            fcntl.flock(lock, fcntl.LOCK_EX)
        p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        tail = []
        for line in p.stdout:
            tail = (tail + [line.rstrip()])[-25:]
            if line.startswith("{"):
                st = json.loads(line)
                log(f"hunyuan {st['stage']} ({st['t']:.0f}s)")
        if p.wait():
            raise EngineError("Hunyuan3D local failed:\n" + "\n".join(tail))
    return {"engine": "hunyuan-local", "host": "local-cpu", "model": model, "octree": octree, "views": sorted(opt.get("views", {}))}


ENGINES = {
    "hunyuan3": {"run": hunyuan3, "label": "Hunyuan3D v3 (fal.ai)", "key": "FAL_KEY", "cost": "~$0.38, +$0.15 with PBR; best faces", "multiview": True},
    "trellis2": {"run": trellis2, "label": "TRELLIS.2 (fal.ai)", "key": "FAL_KEY", "cost": "~$0.25–0.35", "multiview": False},
    "meshy": {"run": meshy, "label": "Meshy (latest)", "key": "MESHY_API_KEY", "cost": "Meshy credits", "multiview": True},
    "trellis2-hf": {"run": trellis2_hf, "label": "TRELLIS.2 (HF Space)", "key": "HF_TOKEN", "cost": "HF GPU quota", "multiview": False},
    "hunyuan-local": {"run": hunyuan_local, "label": "Hunyuan3D-2 (local CPU)", "key": None, "cost": "free, ~25 min on this CPU", "multiview": True, "needs": HUNYUAN},
    "triposr": {"run": triposr, "label": "TripoSR (local CPU draft)", "key": None, "cost": "free", "multiview": False, "needs": TRIPOSR},
}


def available():
    keys = load_keys()

    def ready(e):
        if e["key"] is None:
            return (e["needs"] / ".venv/bin/python").exists()
        return bool(keys.get(e["key"]))

    return {k: {**{x: v for x, v in e.items() if x not in ("run", "needs")}, "ready": ready(e)} for k, e in ENGINES.items()}
