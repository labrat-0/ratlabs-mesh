"""Remove cut-out fringe colours (pure red / yellow halos from badly matted PNG art)
that image-to-3D engines paint into hair and edges. Edits the GLB's base-colour
texture in place: matching pixels are replaced by a blur of their neutral neighbours.

python -m ratmesh.fringe model.glb
"""
import io
import sys

import numpy as np
from PIL import Image, ImageFilter

from .glbtool import _read, _write


def clean(path):
    j, binary = _read(path)
    base = set()
    for m in j.get("materials", []):
        t = m.get("pbrMetallicRoughness", {}).get("baseColorTexture")
        if t is not None:
            base.add(j["textures"][t["index"]]["source"])
    out, changed = bytearray(), 0
    views = {img["bufferView"]: i for i, img in enumerate(j["images"]) if "bufferView" in img}
    for vi, bv in enumerate(j["bufferViews"]):
        start = bv.get("byteOffset", 0)
        data = binary[start:start + bv["byteLength"]]
        if views.get(vi) in base:
            img = Image.open(io.BytesIO(data)).convert("RGB")
            a = np.asarray(img).astype(np.int16)
            r, g, b = a[..., 0], a[..., 1], a[..., 2]
            # saturated reds only; skin and ear pinks keep g and b above half of r
            red = (r > 95) & (g < 0.5 * r) & (b < 0.5 * r) & (r - np.maximum(g, b) > 55)
            yellow = (r > 160) & (g > 140) & (b < 0.45 * g) & (np.minimum(r, g) - b > 100)
            mask = red | yellow
            changed = int(mask.sum())
            if changed:
                m8 = Image.fromarray((mask * 255).astype(np.uint8)).filter(ImageFilter.MaxFilter(5))
                grow = np.asarray(m8) > 0
                # neutral fill: the image with fringe pixels replaced by their local grey level
                grey = np.repeat(a.mean(-1, keepdims=True), 3, -1)
                fill = np.asarray(Image.fromarray(np.where(grow[..., None], grey, a).astype(np.uint8)).filter(ImageFilter.GaussianBlur(3)))
                a = np.where(grow[..., None], fill, a)
                buf = io.BytesIO()
                Image.fromarray(a.astype(np.uint8)).save(buf, "JPEG" if "jpeg" in j["images"][views[vi]].get("mimeType", "") else "PNG", quality=92)
                data = buf.getvalue()
        out += b"\0" * (-len(out) % 4)
        bv["byteOffset"] = len(out)
        bv["byteLength"] = len(data)
        out += data
    j["buffers"][0]["byteLength"] = len(out)
    _write(path, j, bytes(out))
    return changed


if __name__ == "__main__":
    for f in sys.argv[1:]:
        print(f, clean(f), "fringe pixels replaced")
