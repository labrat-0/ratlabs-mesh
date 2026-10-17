"""Concept stage: turn a description plus style references into a clean, single-object
picture that image-to-3D engines read well (one subject, whole, plain background).

Uses Nano Banana edit on fal (~$0.04 an image) with the reference paintings as style
and material guides, so props come out in the art's own look.
"""
from pathlib import Path

from .engines import EngineError, _fal, download, fal_url

FRAME = (
    "Product reference photo for a 3D artist. Show ONLY {what}, alone, whole and uncropped, "
    "centred, three-quarter front view from slightly above, filling about 70% of a square frame. "
    "Plain flat mid-grey studio background, soft even light, no floor clutter, no other objects, "
    "no people or creatures unless they are part of the object, no text overlays. "
    "Match the materials, wear, colours and era of the attached lab artwork: {style}"
)
STYLE = ("grimy retro-futurist underground lab, scuffed olive and gunmetal steel, brass fittings, "
         "worn rubber cables, green phosphor glow, dark and gritty but clearly lit")


def concept(what, out, refs=(), style=STYLE, seed=None, log=print):
    payload = {
        "prompt": FRAME.format(what=what, style=style),
        "image_urls": [fal_url(r) for r in refs],
        "aspect_ratio": "1:1",
        "output_format": "png",
        "num_images": 1,
    }
    if seed is not None:
        payload["seed"] = int(seed)
    r = _fal("fal-ai/nano-banana/edit", payload, log)
    imgs = r.get("images") or []
    if not imgs:
        raise EngineError(f"no image returned: {r.get('description', '')[:200]}")
    Path(out).parent.mkdir(parents=True, exist_ok=True)
    download(imgs[0]["url"], out)
    return out
