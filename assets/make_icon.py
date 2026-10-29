"""Build the RatLabs Mesh app icon: the RatLabs Studio pixel rat, re-drawn as a
triangulated wireframe mesh in the studio's green (dots at the vertices).

python assets/make_icon.py  (RatLabs Mesh venv: numpy, scipy, Pillow)
"""
from pathlib import Path

import numpy as np
from PIL import Image, ImageChops, ImageDraw, ImageFilter
from scipy import ndimage
from scipy.spatial import Delaunay

HERE = Path(__file__).resolve().parent
SRC = Path.home() / ".local/share/icons/hicolor/512x512/apps/ratlabs-studio.png"
S = 1024
GREEN = (57, 255, 136)  # the studio accent, #39ff88

src = np.asarray(Image.open(SRC).convert("RGB").resize((S, S), Image.NEAREST)).astype(float)
grey = src.mean(-1)
# the rat: mid/light greys inside the tile, away from the frame
inner = np.zeros_like(grey, bool)
inner[90:S - 90, 60:S - 60] = True
rat = (grey > 40) & inner
rat = ndimage.binary_opening(rat, iterations=2)
# the eye: dark pixels surrounded by the white head
light = ndimage.uniform_filter((grey > 200).astype(float), 60) > 0.45
eye = (grey < 40) & light & inner
body = ndimage.binary_closing(rat, iterations=3)

# vertices: the silhouette outline plus a jittered interior grid
rng = np.random.default_rng(7)
edge = body & ~ndimage.binary_erosion(body, iterations=1)
ey, ex = np.nonzero(edge)
pick = rng.choice(len(ex), size=min(len(ex), 110), replace=False)
pts = [(ex[i], ey[i]) for i in pick]
step = 64
for y in range(0, S, step):
    for x in range(0, S, step):
        jx, jy = x + rng.uniform(-14, 14), y + rng.uniform(-14, 14)
        xi, yi = int(np.clip(jx, 0, S - 1)), int(np.clip(jy, 0, S - 1))
        if body[yi, xi]:
            pts.append((jx, jy))
pts = np.array(pts, float)
# thin out points closer than 18 px
keep = []
for p in pts:
    if all(np.hypot(*(p - q)) > 30 for q in keep):
        keep.append(p)
pts = np.array(keep)
tri = Delaunay(pts)
tris = [t for t in tri.simplices if body[int(pts[t, 1].mean()), int(pts[t, 0].mean())]]

# tile
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
d.rounded_rectangle((16, 16, S - 16, S - 16), radius=150, fill=(8, 10, 12, 255), outline=(58, 62, 66, 255), width=12)

# faint facet shading from the original pixel greys, so the rat keeps its markings
shade = Image.new("RGBA", (S, S), (0, 0, 0, 0))
sd = ImageDraw.Draw(shade)
for t in tris:
    c = pts[t]
    g = grey[int(c[:, 1].mean()), int(c[:, 0].mean())]
    # the studio rat's light head and dark saddle carry over as brighter and darker facets
    a = int(np.interp(g, [60, 140, 230], [18, 45, 120]))
    sd.polygon([tuple(p) for p in c], fill=(int(np.interp(g, [60, 230], [40, 200])), int(np.interp(g, [60, 230], [150, 255])), int(np.interp(g, [60, 230], [125, 230])), a))
img = Image.alpha_composite(img, shade)

# wireframe with a soft glow
wire = Image.new("RGBA", (S, S), (0, 0, 0, 0))
wd = ImageDraw.Draw(wire)
edges = set()
for t in tris:
    for a, b in ((t[0], t[1]), (t[1], t[2]), (t[2], t[0])):
        edges.add((min(a, b), max(a, b)))
for a, b in edges:
    wd.line([tuple(pts[a]), tuple(pts[b])], fill=GREEN + (170,), width=5)
for p in pts[np.unique(np.array(tris).ravel())]:
    r = 9
    wd.ellipse((p[0] - r, p[1] - r, p[0] + r, p[1] + r), fill=GREEN + (255,))
glow = wire.filter(ImageFilter.GaussianBlur(14))
glow = Image.fromarray((np.asarray(glow).astype(float) * [1, 1, 1, 0.9]).astype(np.uint8))
img = Image.alpha_composite(img, glow)
img = Image.alpha_composite(img, wire)

# eye: a dark hole with a small mint glint
if eye.any():
    yy, xx = np.nonzero(eye)
    cx, cy, r = xx.mean(), yy.mean(), max(14, (eye.sum() / np.pi) ** 0.5 * 1.1)
    d = ImageDraw.Draw(img)
    d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=(4, 6, 8, 255), outline=GREEN + (255,), width=4)
    d.ellipse((cx - r * .35 + 4, cy - r * .35 - 4, cx + r * .05 + 4, cy + r * .05 - 4), fill=GREEN + (255,))

# clip everything to the tile
mask = Image.new("L", (S, S), 0)
ImageDraw.Draw(mask).rounded_rectangle((16, 16, S - 16, S - 16), radius=150, fill=255)
img.putalpha(ImageChops.multiply(img.getchannel("A"), mask))
img.save(HERE / "icon-1024.png")
for n in (512, 256, 128, 64, 48, 32):
    img.resize((n, n), Image.LANCZOS).save(HERE / f"icon-{n}.png")
print("tris", len(tris), "verts", len(pts))
