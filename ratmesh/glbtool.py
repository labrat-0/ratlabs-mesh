"""Shrink a GLB's embedded textures for the web: cap the size, turn opaque PNGs into
JPEGs, re-encode JPEGs. Geometry is untouched. Pure Python (Pillow), no Blender.

python -m ratmesh.glbtool file.glb [more.glb ...] [--max 1024] [--quality 82]
"""
import io
import json
import struct
import sys
from pathlib import Path

from PIL import Image


def _read(path):
    b = Path(path).read_bytes()
    magic, version, _ = struct.unpack("<4sII", b[:12])
    if magic != b"glTF" or version != 2:
        raise ValueError(f"{path}: not a GLB v2")
    jlen = struct.unpack("<I", b[12:16])[0]
    j = json.loads(b[20:20 + jlen])
    off = 20 + jlen
    blen = struct.unpack("<I", b[off:off + 4])[0] if off < len(b) else 0
    return j, b[off + 8:off + 8 + blen]


def _write(path, j, binary):
    jb = json.dumps(j, separators=(",", ":")).encode()
    jb += b" " * (-len(jb) % 4)
    binary += b"\0" * (-len(binary) % 4)
    total = 12 + 8 + len(jb) + 8 + len(binary)
    out = struct.pack("<4sII", b"glTF", 2, total) + struct.pack("<I4s", len(jb), b"JSON") + jb
    out += struct.pack("<I4s", len(binary), b"BIN\0") + binary
    Path(path).write_bytes(out)


def _recode(data, mime, max_size, quality):
    im = Image.open(io.BytesIO(data))
    im.load()
    if max(im.size) > max_size:
        s = max_size / max(im.size)
        im = im.resize((max(1, round(im.width * s)), max(1, round(im.height * s))), Image.LANCZOS)
    has_alpha = im.mode in ("RGBA", "LA", "PA") and im.getchannel("A").getextrema()[0] < 250
    buf = io.BytesIO()
    if has_alpha:
        im.save(buf, "PNG", optimize=True)
        new_mime = "image/png"
    else:
        im.convert("RGB").save(buf, "JPEG", quality=quality, optimize=True, progressive=True)
        new_mime = "image/jpeg"
    new = buf.getvalue()
    return (new, new_mime) if len(new) < len(data) else (data, mime)


def shrink(path, max_size=1024, quality=82):
    j, binary = _read(path)
    image_views = {img["bufferView"]: i for i, img in enumerate(j.get("images", [])) if "bufferView" in img}
    out, before = bytearray(), len(binary)
    for vi, bv in enumerate(j.get("bufferViews", [])):
        start = bv.get("byteOffset", 0)
        data = binary[start:start + bv["byteLength"]]
        if vi in image_views:
            img = j["images"][image_views[vi]]
            data, img["mimeType"] = _recode(data, img.get("mimeType", "image/png"), max_size, quality)
        out += b"\0" * (-len(out) % 4)
        bv["byteOffset"] = len(out)
        bv["byteLength"] = len(data)
        out += data
    if j.get("buffers"):
        j["buffers"][0]["byteLength"] = len(out)
    _write(path, j, bytes(out))
    return before, len(out)


if __name__ == "__main__":
    args = sys.argv[1:]
    mx = int(args[args.index("--max") + 1]) if "--max" in args else 1024
    q = int(args[args.index("--quality") + 1]) if "--quality" in args else 82
    files = [a for i, a in enumerate(args) if a.endswith(".glb")]
    for f in files:
        a, b = shrink(f, mx, q)
        print(f"{f}: {a / 1e6:.1f} MB -> {b / 1e6:.1f} MB")
