"""Pack a grabbed Market material into a game-ready folder:

  color.jpg   base colour
  normal.jpg  OpenGL normal (+Y), as three.js expects
  arm.jpg     R = ambient occlusion, G = roughness, B = metalness (glTF convention)
  alpha.jpg   only when the source has an opacity map (grates, catwalks)
  preview.png the source's thumbnail

Height maps and duplicate single-channel maps are dropped: three.js reads ao, roughness
and metalness from one ARM texture, and displacement needs tessellation the game doesn't use.
"""
import glob
import json
import re
from pathlib import Path

import httpx
from PIL import Image

from .market import LIB

ROLES = [("color", r"(_diff_|_Color)"), ("normal", r"(_nor_gl_|_NormalGL)"), ("roughness", r"(_rough_|_Roughness)"),
         ("ao", r"(_ao_|_AmbientOcclusion)"), ("metalness", r"(_metal_|_Metalness)"), ("arm", r"_arm_"),
         ("alpha", r"_Opacity")]


def pack(source, id, out):
    src = LIB / source / id
    meta = json.loads((src / "meta.json").read_text())
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    found = {}
    for f in sorted(p for p in src.iterdir() if p.suffix.lower() in (".jpg", ".png")):
        for role, pat in ROLES:
            if re.search(pat, f.name) and role not in found:
                found[role] = f
    if "color" not in found:
        raise FileNotFoundError(f"{source}/{id}: no colour map")
    size = Image.open(found["color"]).size
    Image.open(found["color"]).convert("RGB").save(out / "color.jpg", quality=82, optimize=True, progressive=True)
    maps = {"color": "color.jpg"}
    if "normal" in found:
        Image.open(found["normal"]).convert("RGB").save(out / "normal.jpg", quality=90, optimize=True, progressive=True)
        maps["normal"] = "normal.jpg"
    if "arm" in found:
        arm = Image.open(found["arm"]).convert("RGB")
    else:
        ch = lambda role, default: Image.open(found[role]).convert("L").resize(size) if role in found else Image.new("L", size, default)
        arm = Image.merge("RGB", (ch("ao", 255), ch("roughness", 204), ch("metalness", 0)))
    arm.save(out / "arm.jpg", quality=85, optimize=True, progressive=True)
    maps["arm"] = "arm.jpg"
    if "alpha" in found:
        Image.open(found["alpha"]).convert("L").save(out / "alpha.jpg", quality=90)
        maps["alpha"] = "alpha.jpg"
    if meta.get("thumb"):
        try:
            (out / "preview.png").write_bytes(httpx.get(meta["thumb"], timeout=30, follow_redirects=True).content)
        except httpx.HTTPError:
            pass
    return {"id": id, "name": meta["name"], "maps": maps, "licence": meta["licence"], "author": meta.get("author"),
            "source": meta["page"]}
