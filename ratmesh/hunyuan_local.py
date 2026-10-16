"""Hunyuan3D-2 on this machine: shape, then reduce, then paint.

Runs under the Hunyuan3D-2 venv (see local/setup_hunyuan.sh), not this project's.
Prints one JSON line per stage so the job log can follow along.

  python hunyuan_local.py prep.png out.glb --engine ~/tools/Hunyuan3D-2 \
      [--model mini|full] [--faces 30000] [--octree 256] [--no-texture] \
      [--back b.png --left l.png --right r.png]
"""
import argparse, json, os, sys, time
from pathlib import Path

ap = argparse.ArgumentParser()
ap.add_argument("image", type=Path)
ap.add_argument("out", type=Path)
ap.add_argument("--engine", type=Path, required=True)
ap.add_argument("--model", choices=["mini", "full"], default="mini")
ap.add_argument("--steps", type=int, default=5)
ap.add_argument("--octree", type=int, default=256)
ap.add_argument("--faces", type=int, default=30000)
ap.add_argument("--seed", type=int, default=1234)
ap.add_argument("--no-texture", action="store_true")
ap.add_argument("--keep-lighting", action="store_true", help="paint from the art as lit, skip delighting")
ap.add_argument("--delight-steps", type=int, default=20)
ap.add_argument("--threads", type=int, default=max(2, (os.cpu_count() or 4) - 2))
for side in ("back", "left", "right"):
    ap.add_argument("--" + side, type=Path)
a = ap.parse_args()
t0 = time.monotonic()


def stage(s, **kw):
    print(json.dumps({"stage": s, "t": round(time.monotonic() - t0, 1), **kw}), flush=True)


stage("loading")
os.environ.setdefault("HF_HUB_DISABLE_PROGRESS_BARS", "1")
os.environ.setdefault("HY3D_DEVICE", "cpu")
os.environ.setdefault("HY3D_DTYPE", "float32")
# Upstream runs 50; 20 is ~2.5x faster and stays closer to the art (50 oversaturates).
os.environ["HY3D_DELIGHT_STEPS"] = str(a.delight_steps)
sys.path.insert(0, str(a.engine))
import torch
from PIL import Image
from hy3dgen.shapegen import DegenerateFaceRemover, FaceReducer, FloaterRemover, Hunyuan3DDiTFlowMatchingPipeline

torch.set_num_threads(a.threads)
front = Image.open(a.image).convert("RGBA")
views = {s: Image.open(getattr(a, s)).convert("RGBA") for s in ("back", "left", "right") if getattr(a, s)}

if views:
    repo, sub = "tencent/Hunyuan3D-2mv", "hunyuan3d-dit-v2-mv-turbo"
    prompt = {"front": front, **views}
elif a.model == "full":
    repo, sub = "tencent/Hunyuan3D-2", "hunyuan3d-dit-v2-0-turbo"
    prompt = front
else:
    repo, sub = "tencent/Hunyuan3D-2mini", "hunyuan3d-dit-v2-mini-turbo"
    prompt = front

shape = Hunyuan3DDiTFlowMatchingPipeline.from_pretrained(repo, subfolder=sub, use_safetensors=True, device="cpu", dtype=torch.float32)
shape.enable_flashvdm(topk_mode="merge")
# The turbo VAE that enable_flashvdm swaps in loads as fp16; cast afterwards.
shape.to(device="cpu", dtype=torch.float32)
stage("shape", model=sub)
mesh = shape(image=prompt, num_inference_steps=a.steps, octree_resolution=a.octree, num_chunks=20000,
             generator=torch.manual_seed(a.seed), output_type="trimesh")[0]
del shape
stage("cleanup", faces=len(mesh.faces))
mesh = FloaterRemover()(mesh)
mesh = DegenerateFaceRemover()(mesh)
# Reduce before painting so the texture is baked onto the final topology.
mesh = FaceReducer()(mesh, max_facenum=a.faces)

if not a.no_texture:
    from hy3dgen.texgen import Hunyuan3DPaintPipeline
    stage("loading paint")
    paint = Hunyuan3DPaintPipeline.from_pretrained("tencent/Hunyuan3D-2", subfolder="hunyuan3d-paint-v2-0-turbo")
    if a.keep_lighting:
        # Same output format as the delight model: RGB flattened on white.
        def flat(im):
            bg = Image.new("RGBA", im.size, (255, 255, 255, 255))
            return Image.alpha_composite(bg, im.convert("RGBA")).convert("RGB")
        paint.models["delight_model"] = flat
    stage("painting", faces=len(mesh.faces))
    mesh = paint(mesh, image=[front, *views.values()])

mesh.export(a.out)
stage("done", faces=len(mesh.faces))
