"""Put the art's own fine detail back onto the painted front view.

Hunyuan's painter repaints every view at 512 px, so a face drawn at 1000+ px in the
art comes back soft. The painted front view is pixel-aligned with the mesh; the art
is not (the shape model widens bodies, art is often slightly three-quarter). So:

1. fit the art's silhouette onto the mesh's front silhouette (bounding box),
2. refine with an affine ECC fit, which ignores brightness and contrast differences,
3. correct what is left with a smoothed, capped optical flow on structure-only images,
4. add only the art's high-frequency detail where local structure agrees (NCC),
   so misaligned bits fade out instead of ghosting.

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


def _grey(rgb):
    return cv2.cvtColor(rgb, cv2.COLOR_RGB2GRAY)


def _structure(g, sigma):
    """Local contrast normalisation: what's left is shape, not brightness."""
    mu = cv2.GaussianBlur(g, (0, 0), sigma)
    sd = np.sqrt(np.maximum(cv2.GaussianBlur(g * g, (0, 0), sigma) - mu * mu, 1e-6))
    return (g - mu) / (sd + 0.02)


def _ncc(a, b, sigma):
    """Local normalised cross-correlation of two grey images, -1..1 per pixel."""
    ma, mb = cv2.GaussianBlur(a, (0, 0), sigma), cv2.GaussianBlur(b, (0, 0), sigma)
    va = cv2.GaussianBlur(a * a, (0, 0), sigma) - ma * ma
    vb = cv2.GaussianBlur(b * b, (0, 0), sigma) - mb * mb
    cov = cv2.GaussianBlur(a * b, (0, 0), sigma) - ma * mb
    return cov / np.sqrt(np.maximum(va * vb, 1e-8))


def _warp(img, flow):
    R = img.shape[0]
    yy, xx = np.mgrid[0:R, 0:R].astype(np.float32)
    return cv2.remap(img, xx + flow[..., 0], yy + flow[..., 1], cv2.INTER_LINEAR, borderMode=cv2.BORDER_CONSTANT)


def transfer(painted, art_rgba, sil, strength=0.9, debug=None):
    """painted: PIL RGB front view. sil: bool HxW mesh silhouette at the render size."""
    R = sil.shape[0]
    G = np.asarray(painted.convert("RGB").resize((R, R), Image.LANCZOS)).astype(np.float32) / 255
    S0, Sa = fit_to_silhouette(art_rgba, sil)
    m = sil.astype(np.float32)

    # 1. affine ECC at 512 on blurred grey; ECC is invariant to brightness/contrast.
    n = 512
    k = R / n
    gs = cv2.GaussianBlur(cv2.resize(_grey(G) * m, (n, n), interpolation=cv2.INTER_AREA), (0, 0), 2)
    ss = cv2.GaussianBlur(cv2.resize(_grey(S0) * Sa, (n, n), interpolation=cv2.INTER_AREA), (0, 0), 2)
    A = np.eye(2, 3, dtype=np.float32)
    try:
        _, A = cv2.findTransformECC(gs, ss, A, cv2.MOTION_AFFINE,
                                    (cv2.TERM_CRITERIA_EPS | cv2.TERM_CRITERIA_COUNT, 200, 1e-5), None, 5)
    except cv2.error:
        pass
    A[:, 2] *= k
    flags = cv2.INTER_LINEAR | cv2.WARP_INVERSE_MAP
    S1 = cv2.warpAffine(S0, A, (R, R), flags=flags)
    Sa1 = cv2.warpAffine(Sa, A, (R, R), flags=flags)

    # 2. residual flow on structure-only images, smoothed and capped so it can't tear.
    n = 1024
    k = R / n
    gst = _structure(cv2.resize(_grey(G), (n, n), interpolation=cv2.INTER_AREA), 12)
    sst = _structure(cv2.resize(_grey(S1), (n, n), interpolation=cv2.INTER_AREA), 12)
    to8 = lambda x: np.clip(x * 40 + 128, 0, 255).astype(np.uint8)
    dis = cv2.DISOpticalFlow_create(cv2.DISOPTICAL_FLOW_PRESET_MEDIUM)
    flow = dis.calc(to8(gst), to8(sst), None)
    flow = cv2.GaussianBlur(flow, (0, 0), 10)
    mag = np.linalg.norm(flow, axis=-1, keepdims=True)
    cap = n * 0.02
    flow = flow * np.minimum(1, cap / (mag + 1e-6))
    flow = cv2.resize(flow, (R, R), interpolation=cv2.INTER_LINEAR) * k
    Sw, Swa = _warp(S1, flow), _warp(Sa1, flow)

    # 3. detail band from the art, trusted where local structure matches.
    sigma = R / 160  # ~13 px at 2048: eyes, tattoo lines, stitching, fur clumps
    detail = Sw - cv2.GaussianBlur(Sw, (0, 0), sigma)
    ncc = _ncc(_grey(G), _grey(Sw), R / 96)
    trust = np.clip((ncc - 0.15) / 0.45, 0, 1)
    inside = cv2.erode(sil.astype(np.uint8), np.ones((9, 9), np.uint8)).astype(np.float32)
    w = cv2.GaussianBlur(trust * np.clip(Swa, 0, 1) * inside, (0, 0), 4)
    # Match the art's detail contrast to the painted view so dark art doesn't punch holes.
    sel = (Swa > 0.5) & sil
    gain = np.clip((G[sel].std(0) + 1e-4) / (Sw[sel].std(0) + 1e-4), 0.5, 2.0) if sel.any() else 1.0
    out = np.clip(G + strength * w[..., None] * detail * gain, 0, 1)
    if debug is not None:
        debug.mkdir(parents=True, exist_ok=True)
        for name, img in (("front_painted", G), ("front_art_warped", Sw), ("front_detail", out)):
            Image.fromarray((img * 255).astype(np.uint8)).save(debug / f"{name}.png")
        Image.fromarray((w * 255).astype(np.uint8)).save(debug / "front_weight.png")
    return Image.fromarray((out * 255).astype(np.uint8)), float(w[sil].mean())
