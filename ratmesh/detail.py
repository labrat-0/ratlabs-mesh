"""Put the art's own fine detail back onto the painted front view.

Hunyuan's painter repaints every view at 512 px, so a face drawn at 1000+ px in the
art comes back soft. The painted front view is pixel-aligned with the mesh; the art
is not (the shape model widens bodies, art is often slightly three-quarter). So:

1. fit the art's silhouette onto the mesh's front silhouette (bounding box),
2. warp it onto the painted view with dense optical flow,
3. add only the art's high-frequency detail, faded wherever the two disagree,
   so misaligned bits never ghost.

Runs in the Hunyuan venv (numpy, OpenCV, PIL).
"""
import cv2
import numpy as np
from PIL import Image


def fit_to_silhouette(art_rgba, sil):
    """Scale and place the art so its bounding box matches the mesh silhouette's."""
    R = sil.shape[0]
    a = np.asarray(art_rgba.convert("RGBA"))
    sy, sx = np.nonzero(a[..., 3] > 127)
    my, mx = np.nonzero(sil)
    crop = Image.fromarray(a[sy.min():sy.max() + 1, sx.min():sx.max() + 1])
    s = (my.max() - my.min() + 1) / crop.height
    crop = crop.resize((max(1, round(crop.width * s)), max(1, round(crop.height * s))), Image.LANCZOS)
    canvas = Image.new("RGBA", (R, R), (0, 0, 0, 0))
    canvas.paste(crop, (round((mx.min() + mx.max()) / 2 - crop.width / 2), int(my.min())), crop)
    c = np.asarray(canvas).astype(np.float32) / 255
    return c[..., :3], c[..., 3]


def transfer(painted, art_rgba, sil, strength=0.9, debug=None):
    """painted: PIL RGB front view at render size. sil: bool HxW mesh silhouette, same size."""
    R = sil.shape[0]
    G = np.asarray(painted.convert("RGB").resize((R, R), Image.LANCZOS)).astype(np.float32) / 255
    S0, Sa = fit_to_silhouette(art_rgba, sil)

    # Flow on grey, mid-grey background so the silhouettes don't dominate.
    def grey(rgb, mask):
        g = cv2.cvtColor((rgb * 255).astype(np.uint8), cv2.COLOR_RGB2GRAY).astype(np.float32)
        g = np.where(mask > 0.5, g, 128)
        # Equalise so a dark art and a lighter delit view still match structurally.
        return cv2.equalizeHist(g.astype(np.uint8))

    small = 1024
    gs = cv2.resize(grey(G, sil.astype(np.float32)), (small, small), interpolation=cv2.INTER_AREA)
    ss = cv2.resize(grey(S0, Sa), (small, small), interpolation=cv2.INTER_AREA)
    dis = cv2.DISOpticalFlow_create(cv2.DISOPTICAL_FLOW_PRESET_MEDIUM)
    flow = dis.calc(gs, ss, None)
    flow = cv2.resize(flow, (R, R), interpolation=cv2.INTER_LINEAR) * (R / small)
    yy, xx = np.mgrid[0:R, 0:R].astype(np.float32)
    mx, my = xx + flow[..., 0], yy + flow[..., 1]
    Sw = cv2.remap(S0, mx, my, cv2.INTER_LINEAR, borderMode=cv2.BORDER_CONSTANT)
    Swa = cv2.remap(Sa, mx, my, cv2.INTER_LINEAR, borderMode=cv2.BORDER_CONSTANT)

    sigma = R / 160  # ~13 px at 2048: eyes, tattoo lines, stitching, fur clumps
    blur = lambda x: cv2.GaussianBlur(x, (0, 0), sigma)
    detail = Sw - blur(Sw)
    # Where the warped art and the painted view agree in their broad colour/structure,
    # trust the art's detail; where they don't, the flow is wrong, so back off.
    gl, sl = blur(G), blur(Sw)
    gn = (gl - gl.mean((0, 1))) / (gl.std((0, 1)) + 1e-4)
    sn = (sl - sl.mean((0, 1))) / (sl.std((0, 1)) + 1e-4)
    agree = np.exp(-(np.abs(gn - sn).mean(-1) / 0.9) ** 2)
    inside = cv2.erode(sil.astype(np.uint8), np.ones((9, 9), np.uint8)).astype(np.float32)
    w = cv2.GaussianBlur(agree * np.clip(Swa, 0, 1) * inside, (0, 0), 3)
    # Match the detail's contrast to the painted view so dark art doesn't punch holes.
    gain = (G.std((0, 1)) + 1e-4) / (Sw[Swa > 0.5].std(0) + 1e-4) if (Swa > 0.5).any() else 1.0
    out = np.clip(G + strength * w[..., None] * detail * np.clip(gain, 0.5, 2.0), 0, 1)
    if debug is not None:
        debug.mkdir(parents=True, exist_ok=True)
        for name, img in (("front_painted", G), ("front_art_warped", Sw), ("front_detail", out)):
            Image.fromarray((img * 255).astype(np.uint8)).save(debug / f"{name}.png")
        Image.fromarray((w * 255).astype(np.uint8)).save(debug / "front_weight.png")
    return Image.fromarray((out * 255).astype(np.uint8)), float(w[sil].mean())
