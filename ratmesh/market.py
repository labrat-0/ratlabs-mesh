"""Market: search free asset sites from one place and grab assets into a local library.

Sources (all have real public APIs; nothing is scraped):
  polyhaven   CC0 models (glTF), textures, HDRIs        no key
  ambientcg   CC0 PBR materials, HDRIs                  no key
  sketchfab   downloadable models, licence per model    search free, download needs SKETCHFAB_TOKEN
  polypizza   low-poly models (Quaternius, Kenney...)   POLYPIZZA_KEY (free account)

Grabbed assets land in library/<source>/<id>/ with meta.json recording name, author,
licence, source page and files, so attribution is never lost.
"""
import json
import shutil
import subprocess
import time
import zipfile
from pathlib import Path
from urllib.parse import quote

import httpx

from .config import BLENDER, ROOT, load_keys

LIB = ROOT / "library"
CACHE = Path.home() / ".cache/ratlabs-mesh/market"
UA = {"User-Agent": "RatLabs-Mesh/1.0 (private asset browser)"}
KINDS = ("model", "material", "hdri")


class MarketError(RuntimeError):
    pass


def _get(url, **kw):
    r = httpx.get(url, headers={**UA, **kw.pop("headers", {})}, timeout=60, follow_redirects=True, **kw)
    if r.status_code >= 400:
        raise MarketError(f"{url.split('?')[0]}: {r.status_code} {r.text[:200]}")
    return r


def _download(url, dest, headers=None):
    dest.parent.mkdir(parents=True, exist_ok=True)
    with httpx.stream("GET", url, headers={**UA, **(headers or {})}, timeout=600, follow_redirects=True) as r:
        if r.status_code >= 400:
            raise MarketError(f"download {r.status_code}: {url}")
        tmp = dest.with_suffix(dest.suffix + ".part")
        with open(tmp, "wb") as f:
            for chunk in r.iter_bytes(1 << 16):
                f.write(chunk)
        tmp.rename(dest)
    return dest


def _unzip(zpath, into):
    with zipfile.ZipFile(zpath) as z:
        for m in z.infolist():
            target = (into / m.filename).resolve()
            if not str(target).startswith(str(into.resolve())):  # zip-slip guard
                continue
            z.extract(m, into)
    zpath.unlink()


def _item(source, id, name, kind, thumb, licence, author, page, **extra):
    return {"source": source, "id": id, "name": name, "kind": kind, "thumb": thumb,
            "licence": licence, "author": author, "page": page, **extra}


# ---------- Poly Haven ----------

PH_TYPES = {"model": "models", "material": "textures", "hdri": "hdris"}


def _ph_catalog(kind):
    """The whole Poly Haven list for a kind, cached for a day (it's ~1 MB and rarely changes)."""
    f = CACHE / f"polyhaven-{kind}.json"
    if f.exists() and time.time() - f.stat().st_mtime < 86400:
        return json.loads(f.read_text())
    data = _get(f"https://api.polyhaven.com/assets?t={PH_TYPES[kind]}").json()
    f.parent.mkdir(parents=True, exist_ok=True)
    f.write_text(json.dumps(data))
    return data


def polyhaven_search(q, kind, page=0, per=40):
    words = q.lower().split()
    hits = []
    for id, a in _ph_catalog(kind).items():
        hay = " ".join([id, a["name"], *a.get("tags", []), *a.get("categories", [])]).lower()
        if all(w in hay for w in words):
            hits.append((a.get("download_count", 0), id, a))
    hits.sort(key=lambda h: -h[0])
    out = []
    for _, id, a in hits[page * per:(page + 1) * per]:
        out.append(_item("polyhaven", id, a["name"], kind,
                         f"https://cdn.polyhaven.com/asset_img/thumbs/{id}.png?width=256&height=256",
                         "CC0", ", ".join(a.get("authors", {})), f"https://polyhaven.com/a/{id}",
                         polycount=a.get("polycount")))
    return out, len(hits)


def polyhaven_grab(id, kind, res="2k"):
    files = _get(f"https://api.polyhaven.com/files/{id}").json()
    d = LIB / "polyhaven" / id
    got = []
    if kind == "model":
        g = (files["gltf"].get(res) or files["gltf"][sorted(files["gltf"])[0]])["gltf"]
        main = _download(g["url"], d / Path(g["url"]).name)
        for rel, inc in g.get("include", {}).items():
            _download(inc["url"], d / rel)
        got.append(main)
    elif kind == "material":
        for m in ("Diffuse", "nor_gl", "Rough", "AO", "Displacement", "arm", "Metal"):
            if m in files and res in files[m]:
                f = files[m][res].get("jpg") or files[m][res].get("png")
                got.append(_download(f["url"], d / Path(f["url"]).name))
    else:
        h = files["hdri"].get(res) or files["hdri"]["1k"]
        got.append(_download(h["hdr"]["url"], d / Path(h["hdr"]["url"]).name))
    return d, got


# ---------- ambientCG ----------

ACG_TYPES = {"material": "Material", "hdri": "HDRI"}


def ambientcg_search(q, kind, page=0, per=40):
    if kind not in ACG_TYPES:
        return [], 0
    r = _get("https://ambientcg.com/api/v2/full_json", params={
        "q": q, "type": ACG_TYPES[kind], "limit": per, "offset": page * per,
        "sort": "Popular", "include": "previewData"}).json()
    out = []
    for a in r.get("foundAssets", []):
        if a.get("dataType") != ACG_TYPES[kind]:
            continue
        out.append(_item("ambientcg", a["assetId"], a.get("displayName") or a["assetId"], kind,
                         a.get("previewImage", {}).get("256-PNG", ""), "CC0", "ambientCG",
                         a.get("shortLink") or f"https://ambientcg.com/view?id={a['assetId']}"))
    return out, r.get("numberOfResults", len(out))


def ambientcg_grab(id, kind, res="2k"):
    r = _get("https://ambientcg.com/api/v2/full_json", params={"id": id, "include": "downloadData"}).json()
    a = r["foundAssets"][0]
    downloads = [x for c in a["downloadFolders"]["default"]["downloadFiletypeCategories"].values()
                 for x in c["downloads"]]
    want = [f"{res.upper()}-JPG", res.upper(), f"{res.upper()}-HDR"]
    pick = next((x for w in want for x in downloads if x["attribute"] == w), downloads[0])
    d = LIB / "ambientcg" / id
    z = _download(pick["fullDownloadPath"], d / pick["fileName"])
    if z.suffix == ".zip":
        _unzip(z, d)
    files = sorted((p for p in d.iterdir() if p.name != "meta.json"), key=lambda p: ("Color" not in p.name, p.name))
    return d, files


# ---------- Sketchfab ----------

def sketchfab_search(q, kind, page=0, per=24):
    if kind != "model":
        return [], 0
    r = _get("https://api.sketchfab.com/v3/search", params={
        "type": "models", "q": q, "downloadable": "true", "count": per,
        "cursor": page * per, "sort_by": "-likeCount"}).json()
    out = []
    for m in r.get("results", []):
        imgs = sorted(m["thumbnails"]["images"], key=lambda i: abs(i["width"] - 256))
        out.append(_item("sketchfab", m["uid"], m["name"], "model", imgs[0]["url"] if imgs else "",
                         (m.get("license") or {}).get("label", "see page"), m["user"]["displayName"],
                         m["viewerUrl"], polycount=m.get("faceCount"), animated=bool(m.get("animationCount"))))
    return out, r.get("totalCount") or len(out)


def sketchfab_grab(id, kind, res=None):
    token = load_keys().get("SKETCHFAB_TOKEN")
    if not token:
        raise MarketError("Sketchfab downloads need a free API token: sketchfab.com > Settings > Password & API. Add it under Keys.")
    r = _get(f"https://api.sketchfab.com/v3/models/{id}/download", headers={"Authorization": f"Token {token}"}).json()
    d = LIB / "sketchfab" / id
    if r.get("glb"):
        return d, [_download(r["glb"]["url"], d / "model.glb")]
    z = _download(r["gltf"]["url"], d / "model.zip")
    _unzip(z, d)
    return d, sorted(d.rglob("*.gltf"))


def sketchfab_info(id):
    m = _get(f"https://api.sketchfab.com/v3/models/{id}").json()
    return {"name": m["name"], "author": m["user"]["displayName"], "page": m["viewerUrl"],
            "licence": (m.get("license") or {}).get("label", "see page")}


# ---------- Poly Pizza ----------

def _pp_headers():
    key = load_keys().get("POLYPIZZA_KEY")
    if not key:
        raise MarketError("Poly Pizza needs a free API key: poly.pizza/settings/api. Add it under Keys.")
    return {"x-auth-token": key}


def polypizza_search(q, kind, page=0, per=32):
    if kind != "model":
        return [], 0
    r = _get(f"https://api.poly.pizza/v1.1/search/{quote(q or 'prop')}",
             params={"Limit": per, "Page": page}, headers=_pp_headers()).json()
    out = []
    for m in r.get("results", []):
        out.append(_item("polypizza", m["ID"], m["Title"], "model", m.get("Thumbnail", ""),
                         m.get("Licence", "see page"), (m.get("Creator") or {}).get("Username", ""),
                         f"https://poly.pizza/m/{m['ID']}", polycount=m.get("Tri Count"),
                         animated=bool(m.get("Animated")), download=m.get("Download")))
    return out, r.get("total", len(out))


def polypizza_grab(id, kind, res=None):
    m = _get(f"https://api.poly.pizza/v1.1/model/{id}", headers=_pp_headers()).json()
    d = LIB / "polypizza" / id
    return d, [_download(m["Download"], d / "model.glb")]


# ---------- registry ----------

SOURCES = {
    "polyhaven": {"label": "Poly Haven", "kinds": ["model", "material", "hdri"], "key": None,
                  "search": polyhaven_search, "grab": polyhaven_grab, "licence": "CC0",
                  "note": "Photoscanned props, furniture, rocks; textures; HDRI skies. All CC0."},
    "ambientcg": {"label": "ambientCG", "kinds": ["material", "hdri"], "key": None,
                  "search": ambientcg_search, "grab": ambientcg_grab, "licence": "CC0",
                  "note": "Thousands of PBR materials and HDRIs. All CC0."},
    "sketchfab": {"label": "Sketchfab", "kinds": ["model"], "key": "SKETCHFAB_TOKEN", "key_for": "download",
                  "search": sketchfab_search, "grab": sketchfab_grab, "licence": "per model",
                  "note": "The biggest free library. Licences vary (CC-BY needs credit); shown on every card."},
    "polypizza": {"label": "Poly Pizza", "kinds": ["model"], "key": "POLYPIZZA_KEY", "key_for": "search",
                  "search": polypizza_search, "grab": polypizza_grab, "licence": "CC0 / CC-BY",
                  "note": "Low-poly game models, including Quaternius and Kenney packs."},
}


def sources():
    keys = load_keys()
    return {k: {"label": s["label"], "kinds": s["kinds"], "licence": s["licence"], "note": s["note"],
                "key": s["key"], "key_for": s.get("key_for"), "ready": not s["key"] or bool(keys.get(s["key"]))}
            for k, s in SOURCES.items()}


def search(source, q, kind, page=0):
    s = SOURCES[source]
    if kind not in s["kinds"]:
        return {"items": [], "total": 0}
    items, total = s["search"](q.strip(), kind, page)
    for it in items:
        it["have"] = (LIB / source / it["id"] / "meta.json").exists()
    return {"items": items, "total": total}


def grab(item, res="2k"):
    """item: a search result dict (source, id, kind, name, licence, author, page)."""
    source, id, kind = item["source"], item["id"], item["kind"]
    d, files = SOURCES[source]["grab"](id, kind, res)
    meta = {k: item.get(k) for k in ("source", "id", "name", "kind", "licence", "author", "page", "thumb")}
    if source == "sketchfab":
        meta.update(sketchfab_info(id))
    meta.update({"res": res, "grabbed": int(time.time()),
                 "files": [str(Path(f).relative_to(d)) for f in files],
                 "main": str(Path(files[0]).relative_to(d)) if files else None})
    (d / "meta.json").write_text(json.dumps(meta, indent=1))
    return meta


def to_glb(path):
    """Pack a .gltf with its external files into one .glb next to it (Blender does the work)."""
    path = Path(path)
    if path.suffix.lower() == ".glb":
        return path
    out = path.with_suffix(".glb")
    expr = ("import bpy,sys;bpy.ops.wm.read_factory_settings(use_empty=True);"
            f"bpy.ops.import_scene.gltf(filepath={str(path)!r});"
            f"bpy.ops.export_scene.gltf(filepath={str(out)!r},export_format='GLB')")
    r = subprocess.run([BLENDER, "-b", "--python-expr", expr], capture_output=True, text=True, timeout=600)
    if not out.exists():
        raise MarketError("could not pack glTF: " + (r.stderr or r.stdout)[-400:])
    return out


def import_to_studio(source, id, kind="prop", height=0, texture=None, polys=None):
    """Create a studio job that finishes a grabbed model (cleanup, budget, renders). Returns the job."""
    from . import jobs
    d = LIB / source / id
    meta = json.loads((d / "meta.json").read_text())
    if meta["kind"] != "model":
        raise MarketError("only models can be finished in the studio")
    glb = to_glb(d / meta["main"])
    return jobs.create(None, glb=str(glb), name=meta["name"], kind=kind, height=height, texture=texture, polys=polys,
                       keep_parts=True)


def library():
    out = []
    for m in sorted(LIB.glob("*/*/meta.json"), key=lambda p: -p.stat().st_mtime):
        meta = json.loads(m.read_text())
        meta["path"] = str(m.parent)
        out.append(meta)
    return out


def remove(source, id):
    d = (LIB / source / id).resolve()
    if d.parent.parent != LIB.resolve():
        raise MarketError("bad path")
    shutil.rmtree(d, ignore_errors=True)
