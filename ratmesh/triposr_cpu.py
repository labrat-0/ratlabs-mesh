"""TripoSR on CPU: the free offline draft engine.

Runs under the TripoSR venv (not this project's). scikit-image marching cubes
stands in for the CUDA-only torchmcubes extension; everything else is upstream.
Prints one JSON line per stage so the job log can follow along.
"""
import argparse, json, os, sys, time, types
from pathlib import Path

ap = argparse.ArgumentParser()
ap.add_argument("image", type=Path)
ap.add_argument("out", type=Path)
ap.add_argument("--engine", type=Path, required=True)
ap.add_argument("--resolution", type=int, default=224)
ap.add_argument("--threads", type=int, default=max(2, (os.cpu_count() or 4) - 2))
a = ap.parse_args()
t0 = time.monotonic()


def stage(s, **kw):
    print(json.dumps({"stage": s, "t": round(time.monotonic() - t0, 1), **kw}), flush=True)


stage("loading")
os.environ.setdefault("HF_HUB_DISABLE_PROGRESS_BARS", "1")
import numpy as np
import torch
from PIL import Image
from skimage.measure import marching_cubes as mc

torch.set_num_threads(a.threads)


def marching_cubes(volume, threshold):
    v, f, _, _ = mc(volume.cpu().numpy(), level=float(threshold))
    return torch.from_numpy(v[:, [2, 1, 0]].copy()), torch.from_numpy(f.astype(np.int64).copy())


sys.modules["torchmcubes"] = types.SimpleNamespace(marching_cubes=marching_cubes)
sys.path.insert(0, str(a.engine))
from tsr.system import TSR
from tsr.utils import resize_foreground

model = TSR.from_pretrained("stabilityai/TripoSR", config_name="config.yaml", weight_name="model.ckpt")
model.eval().to("cpu")
model.renderer.set_chunk_size(16384)
stage("encoding")
rgba = resize_foreground(Image.open(a.image).convert("RGBA"), 0.85)
t = np.asarray(rgba).astype(np.float32) / 255
rgb = t[:, :, :3] * t[:, :, 3:4] + 0.5 * (1 - t[:, :, 3:4])
with torch.inference_mode():
    codes = model([Image.fromarray(np.uint8(rgb * 255))], device="cpu")
stage("surface")
with torch.inference_mode():
    mesh = model.extract_mesh(codes, has_vertex_color=True, resolution=a.resolution)[0]
mesh.update_faces(mesh.nondegenerate_faces())
mesh.remove_unreferenced_vertices()
if mesh.volume < 0:
    mesh.invert()
# TripoSR is z-up with the camera on +x; glTF is y-up facing +z.
R = np.array([[0, 1, 0], [0, 0, 1], [1, 0, 0]], dtype=np.float64)
mesh.vertices = mesh.vertices @ R.T
# Its colours are sRGB; glTF vertex colours are linear.
c = mesh.visual.vertex_colors.astype(np.float64) / 255
c[:, :3] = np.where(c[:, :3] <= 0.04045, c[:, :3] / 12.92, ((c[:, :3] + 0.055) / 1.055) ** 2.4)
mesh.visual.vertex_colors = np.uint8(np.clip(c, 0, 1) * 255)
a.out.write_bytes(mesh.export(file_type="glb"))
stage("done", faces=len(mesh.faces))
