"""Stage 1: turn any picture into a clean, centred subject cut-out.

Image-to-3D models are only as good as their input. Meshy quietly does this
step for you; here it is explicit so it can be checked before paying for a GPU.
"""
from functools import lru_cache
from pathlib import Path

import numpy as np
from PIL import Image, ImageOps
from scipy import ndimage

SIZE = 1024      # what TRELLIS.2 / Hunyuan / Meshy expect
MARGIN = 0.08    # empty border around the subject, as a fraction of the square


@lru_cache(maxsize=2)
def _session(model):
    from rembg import new_session
    return new_session(model)


def cutout(im, model="birefnet-general"):
    from rembg import remove
    return remove(im.convert("RGB"), session=_session(model))


def prepare(src, out, crop=None, model="birefnet-general", keep_background=False):
    """Write a square RGBA cut-out to `out` and return quality notes.

    crop: optional (x0, y0, x1, y1) as 0..1 fractions of the source image,
    used to pick one character out of a busy scene.
    """
    im = ImageOps.exif_transpose(Image.open(src)).convert("RGBA")
    if crop:
        w, h = im.size
        x0, y0, x1, y1 = crop
        im = im.crop((int(x0 * w), int(y0 * h), int(x1 * w), int(y1 * h)))

    if keep_background:
        rgba = im
    else:
        rgba = cutout(im, model)

    a = np.asarray(rgba)[:, :, 3].astype(np.float32) / 255
    solid = a > 0.5
    # Keep the main subject plus any sizeable attached pieces; drop specks.
    labels, n = ndimage.label(solid)
    if n:
        sizes = ndimage.sum(solid, labels, range(1, n + 1))
        keep = np.isin(labels, 1 + np.flatnonzero(sizes >= max(sizes.max() * 0.02, 64)))
        near = ndimage.binary_dilation(keep, iterations=6)
        a = np.where(near, a, 0)
        solid = a > 0.5

    notes = []
    coverage = float(solid.mean())
    fuzzy = float(((a > 0.08) & (a < 0.92)).sum() / max(solid.sum(), 1))
    if coverage < 0.002:
        raise ValueError("No subject found. Draw a crop box around the character.")
    if fuzzy > 0.6:
        notes.append("Mask is mostly semi-transparent: this looks like a scene, not one subject. Draw a crop box around the character.")

    arr = np.asarray(rgba).copy()
    arr[:, :, 3] = (a * 255).astype(np.uint8)
    ys, xs = np.nonzero(arr[:, :, 3] > 8)
    y0, y1, x0, x1 = ys.min(), ys.max() + 1, xs.min(), xs.max() + 1
    subject = Image.fromarray(arr[y0:y1, x0:x1])
    sw, sh = subject.size
    if max(sw, sh) < 512:
        notes.append(f"Subject is only {max(sw, sh)}px tall/wide; a larger source gives sharper textures.")

    inner = int(SIZE * (1 - 2 * MARGIN))
    scale = inner / max(sw, sh)
    subject = subject.resize((max(1, round(sw * scale)), max(1, round(sh * scale))), Image.LANCZOS)
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    canvas.paste(subject, ((SIZE - subject.width) // 2, (SIZE - subject.height) // 2), subject)
    Path(out).parent.mkdir(parents=True, exist_ok=True)
    canvas.save(out)
    return {
        "subject_px": [int(sw), int(sh)],
        "aspect": round(sh / sw, 3),
        "coverage": round(coverage, 4),
        "edge_fuzz": round(fuzzy, 3),
        "notes": notes,
    }
