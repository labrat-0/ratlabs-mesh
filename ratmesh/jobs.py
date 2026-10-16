"""A job is one folder: source art in, finished GLBs and review renders out.

jobs/<id>/
  job.json            options + live status (the UI polls this)
  job.log
  source.png          what you uploaded; views/ holds optional back/left/right art
  prep.png            the cut-out sent to the engine
  v1/raw.glb          straight from the engine
  v1/<name>.glb       finished asset
  v1/renders/*.png    front / three_quarter / side / back
"""
import json
import re
import shutil
import subprocess
import threading
import time
import traceback
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from . import engines, prep
from .config import BLENDER, JOBS, PRESETS, ROOT

_lock = threading.Lock()


def slug(s):
    return re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")[:40] or "asset"


def job_dir(jid):
    d = (JOBS / jid).resolve()
    if d.parent != JOBS.resolve():
        raise ValueError("bad job id")
    return d


def load(jid):
    return json.loads((job_dir(jid) / "job.json").read_text())


def save(job):
    d = job_dir(job["id"])
    with _lock:
        tmp = d / "job.json.tmp"
        tmp.write_text(json.dumps(job, indent=2))
        tmp.replace(d / "job.json")


def list_jobs():
    out = []
    if JOBS.exists():
        for p in sorted(JOBS.glob("*/job.json"), reverse=True):
            try:
                out.append(json.loads(p.read_text()))
            except json.JSONDecodeError:
                pass
    return out


def create(source, name=None, kind="character", engine="trellis2", variants=1, quality="standard",
           height=None, polys=None, texture=None, origin=None, crop=None, rotate=0, views=None,
           pbr=True, pose=None, seed=None, glb=None, keep_background=False):
    """Make a job folder. `glb` imports an existing model instead of generating one."""
    p = PRESETS[kind]
    name = name or Path(source or glb).stem
    jid = time.strftime("%Y%m%d-%H%M%S-") + slug(name)
    d = JOBS / jid
    (d / "views").mkdir(parents=True)
    if source:
        shutil.copy(source, d / ("source" + Path(source).suffix.lower()))
    for side, path in (views or {}).items():
        shutil.copy(path, d / "views" / (side + Path(path).suffix.lower()))
    if glb:
        shutil.copy(glb, d / "import.glb")
    job = {
        "id": jid, "name": name, "kind": kind, "engine": "import" if glb else engine,
        "variants": 1 if glb else max(1, min(4, int(variants))), "quality": quality,
        "height": float(height or p["height"]), "polys": int(polys or p["polys"]),
        "texture": int(texture or p["texture"]), "origin": origin or p["origin"],
        "crop": crop, "rotate": float(rotate or 0), "pbr": pbr, "pose": pose,
        "seed": int(seed if seed is not None else int(time.time()) % 100000),
        "keep_background": keep_background,
        "status": "queued", "stage": "queued", "created": time.time(), "results": [], "prep": None, "error": None,
    }
    save(job)
    return job


def _log(job, msg):
    line = time.strftime("%H:%M:%S ") + msg
    with open(job_dir(job["id"]) / "job.log", "a") as f:
        f.write(line + "\n")
    job["stage"] = msg
    save(job)


def finish(raw, out, job, renders):
    spec = {"in": str(raw), "out": str(out), "height": job["height"], "polys": job["polys"],
            "texture": job["texture"], "origin": job["origin"], "rotate": job["rotate"],
            "renders": str(renders) if renders else None, "stats": str(out.with_suffix(".json")), "name": slug(job["name"])}
    sp = out.with_suffix(".spec.json")
    sp.write_text(json.dumps(spec))
    r = subprocess.run([BLENDER, "-b", "--python", str(ROOT / "ratmesh/finish_blender.py"), "--", str(sp)],
                       capture_output=True, text=True, timeout=1800)
    if r.returncode or not out.exists():
        raise RuntimeError("Blender finishing failed:\n" + (r.stdout + r.stderr)[-1500:])
    sp.unlink()
    if renders:
        subprocess.run(["magick", "montage", *[str(renders / f"{v}.png") for v in ("front", "three_quarter", "side", "back")],
                        "-tile", "4x1", "-geometry", "512x512+0+0", "-background", "#06070a", str(renders / "sheet.jpg")],
                       capture_output=True)
    return json.loads(out.with_suffix(".json").read_text())


def run(jid):
    job = load(jid)
    d = job_dir(jid)
    job.update(status="running", error=None, results=[])
    save(job)
    try:
        opt = {k: job[k] for k in ("polys", "texture", "quality", "pbr", "pose")}
        if job["engine"] == "import":
            raws = [(1, d / "import.glb", {"engine": "import"})]
        else:
            need = engines.ENGINES[job["engine"]]["key"]
            if need and not engines.load_keys().get(need):
                raise engines.EngineError(f"{need} is not set. Add it with Keys in the UI or ~/.config/ratlabs-mesh/keys.env")
            _log(job, "prep: cutting out the subject")
            src = next(d.glob("source.*"))
            job["prep"] = prep.prepare(src, d / "prep.png", crop=job["crop"], keep_background=job["keep_background"])
            for note in job["prep"]["notes"]:
                _log(job, "prep note: " + note)
            views = {}
            for v in sorted((d / "views").glob("*.*")):
                if v.name.endswith(".prep.png"):
                    continue
                out = d / "views" / (v.stem + ".prep.png")
                prep.prepare(v, out)
                views[v.stem] = str(out)
            opt["views"] = views
            run_engine = engines.ENGINES[job["engine"]]["run"]

            def gen(i):
                vd = d / f"v{i}"
                vd.mkdir(exist_ok=True)
                o = dict(opt, seed=job["seed"] + i - 1)
                meta = run_engine(d / "prep.png", vd / "raw.glb", o, lambda m: _log(job, f"v{i} {m}"))
                return i, vd / "raw.glb", meta

            _log(job, f"generate: {job['engine']} x{job['variants']}")
            with ThreadPoolExecutor(job["variants"]) as ex:
                raws = sorted(ex.map(gen, range(1, job["variants"] + 1)))
        for i, raw, meta in raws:
            vd = d / f"v{i}"
            vd.mkdir(exist_ok=True)
            _log(job, f"v{i} finish: clean, retopo to {job['polys']} tris, {job['height']} m, renders")
            stats = finish(raw, vd / f"{slug(job['name'])}.glb", job, vd / "renders")
            job["results"].append({"variant": i, "glb": f"v{i}/{slug(job['name'])}.glb", "raw": str(raw.relative_to(d)),
                                   "renders": f"v{i}/renders", "meta": meta, "stats": stats})
            save(job)
        job["status"] = "done"
        _log(job, "done")
    except Exception as e:
        job["status"] = "failed"
        job["error"] = f"{type(e).__name__}: {e}"
        _log(job, "failed: " + job["error"])
        with open(d / "job.log", "a") as f:
            f.write(traceback.format_exc())
    save(job)
    return job


def refinish(jid, **changes):
    """Re-run only the finishing stage with new size/budget/orientation. No GPU cost."""
    job = load(jid)
    job.update({k: v for k, v in changes.items() if v is not None})
    d = job_dir(jid)
    job["status"] = "running"
    save(job)
    try:
        for r in job["results"]:
            _log(job, f"v{r['variant']} refinish")
            r["stats"] = finish(d / r["raw"], d / r["glb"], job, d / r["renders"])
        job["status"] = "done"
        _log(job, "done")
    except Exception as e:
        job["status"], job["error"] = "failed", f"{type(e).__name__}: {e}"
        _log(job, "failed: " + job["error"])
    save(job)
    return job
