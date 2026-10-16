"""Hunyuan3D-2 on this machine: shape, then reduce, then paint.

Runs under the Hunyuan3D-2 venv (see local/setup_hunyuan.sh), not this project's.
Prints one JSON line per stage so the job log can follow along.

  python hunyuan_local.py prep.png out.glb --engine ~/tools/Hunyuan3D-2 \
      [--model mini|full] [--faces 30000] [--octree 256] [--no-texture] \
      [--back b.png --left l.png --right r.png] [--detail-source art_hi.png]

Keeps its working files beside the output in work/: the full-detail mesh (for the
normal-map bake), the game mesh, and the six painted views. --reuse skips shape and
the multi-view diffusion and only re-bakes from work/, which takes about a minute.
"""
import argparse, json, os, sys, time
from pathlib import Path

ap = argparse.ArgumentParser()
ap.add_argument("image", type=Path)
ap.add_argument("out", type=Path)
ap.add_argument("--engine", type=Path, required=True)
ap.add_argument("--model", choices=["mini", "full"], default="mini")
ap.add_argument("--steps", type=int, default=5)
ap.add_argument("--base", action="store_true", help="non-turbo shape model: slower, follows the art more closely")
ap.add_argument("--guidance", type=float, default=5.0)
ap.add_argument("--octree", type=int, default=256)
ap.add_argument("--faces", type=int, default=30000)
ap.add_argument("--seed", type=int, default=1234)
ap.add_argument("--no-texture", action="store_true")
ap.add_argument("--keep-lighting", action="store_true", help="paint from the art as lit, skip delighting")
ap.add_argument("--delight-steps", type=int, default=20)
ap.add_argument("--detail-source", type=Path, help="high-res cut-out to transfer front detail from")
ap.add_argument("--detail", type=float, default=0.9, help="front detail strength, 0 turns it off")
ap.add_argument("--reuse", action="store_true", help="re-bake from work/ without regenerating")
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

work = a.out.parent / "work"
work.mkdir(parents=True, exist_ok=True)

if a.reuse:
    import trimesh
    mesh = trimesh.load(work / "lowpoly.glb", force="mesh")
else:
    if views:
        repo, sub = "tencent/Hunyuan3D-2mv", "hunyuan3d-dit-v2-mv-turbo"
        prompt = {"front": front, **views}
    elif a.model == "full":
        repo, sub = "tencent/Hunyuan3D-2", "hunyuan3d-dit-v2-0" if a.base else "hunyuan3d-dit-v2-0-turbo"
        prompt = front
    else:
        repo, sub = "tencent/Hunyuan3D-2mini", "hunyuan3d-dit-v2-mini-turbo"
        prompt = front
    shape = Hunyuan3DDiTFlowMatchingPipeline.from_pretrained(repo, subfolder=sub, use_safetensors=True, device="cpu", dtype=torch.float32)
    shape.enable_flashvdm(topk_mode="merge")
    # The turbo VAE that enable_flashvdm swaps in loads as fp16; cast afterwards.
    shape.to(device="cpu", dtype=torch.float32)
    stage("shape", model=sub)
    mesh = shape(image=prompt, num_inference_steps=a.steps, octree_resolution=a.octree, num_chunks=20000, guidance_scale=a.guidance,
                 generator=torch.manual_seed(a.seed), output_type="trimesh")[0]
    del shape
    stage("cleanup", faces=len(mesh.faces))
    mesh = FloaterRemover()(mesh)
    mesh = DegenerateFaceRemover()(mesh)
    # The full-detail surface is kept for the normal-map bake.
    mesh.export(work / "hipoly.glb")
    # Reduce before painting so the texture is baked onto the final topology.
    mesh = FaceReducer()(mesh, max_facenum=a.faces)
    mesh.export(work / "lowpoly.glb")


def paint_mesh(paint, mesh, prompts):
    """Upstream Hunyuan3DPaintPipeline.__call__, plus saved views and front detail transfer."""
    from hy3dgen.texgen.utils.uv_warp_utils import mesh_uv_wrap
    cfg = paint.config
    elevs, azims, weights = cfg.candidate_camera_elevs, cfg.candidate_camera_azims, cfg.candidate_view_weights
    mesh = mesh_uv_wrap(mesh)
    paint.render.load_mesh(mesh)
    saved = [work / f"view_{i}.png" for i in range(len(azims))]
    if a.reuse and all(p.exists() for p in saved):
        views_out = [Image.open(p).convert("RGB") for p in saved]
    else:
        prompts = [paint.recenter_image(p) for p in prompts]
        stage("delight" if not a.keep_lighting else "reference")
        prompts = [paint.models["delight_model"](p) for p in prompts]
        normal_maps = paint.render_normal_multiview(elevs, azims, use_abs_coor=True)
        position_maps = paint.render_position_multiview(elevs, azims)
        camera_info = [(((azim // 30) + 9) % 12) // {-20: 1, 0: 1, 20: 1, -90: 3, 90: 3}[elev]
                       + {-20: 0, 0: 12, 20: 24, -90: 36, 90: 40}[elev] for azim, elev in zip(azims, elevs)]
        stage("multiview diffusion")
        views_out = paint.models["multiview_model"](prompts, normal_maps + position_maps, camera_info)
        for im, p in zip(views_out, saved):
            im.save(p)
    views_out = [v.resize((cfg.render_size, cfg.render_size)) for v in views_out]
    if a.detail > 0 and a.detail_source:
        from detail import transfer
        _, clip = paint.render.get_pos_from_mvp(elevs[0], azims[0], None, None)
        rast, _ = paint.render.raster_rasterize(clip, paint.render.pos_idx, resolution=[cfg.render_size] * 2)
        sil = (rast[0, ..., -1] > 0).cpu().numpy()
        views_out[0], cover = transfer(views_out[0], Image.open(a.detail_source), sil, a.detail, debug=work)
        stage("front detail", coverage=round(cover, 3))
    stage("baking")
    texture, mask = paint.bake_from_multiview(views_out, elevs, azims, weights, method=cfg.merge_method)
    texture = paint.texture_inpaint(texture, (mask.squeeze(-1).cpu().numpy() * 255).astype("uint8"))
    paint.render.set_texture(texture)
    return paint.render.save_mesh()


if not a.no_texture:
    from hy3dgen.texgen import Hunyuan3DPaintPipeline
    sys.path.append(str(Path(__file__).resolve().parent))  # for detail.py; appended so nothing is shadowed
    stage("loading paint")
    paint = Hunyuan3DPaintPipeline.from_pretrained("tencent/Hunyuan3D-2", subfolder="hunyuan3d-paint-v2-0-turbo")
    if a.keep_lighting:
        # Same output format as the delight model: RGB flattened on white.
        def flat(im):
            bg = Image.new("RGBA", im.size, (255, 255, 255, 255))
            return Image.alpha_composite(bg, im.convert("RGBA")).convert("RGB")
        paint.models["delight_model"] = flat
    stage("painting", faces=len(mesh.faces))
    mesh = paint_mesh(paint, mesh, [front, *views.values()])

mesh.export(a.out)
stage("done", faces=len(mesh.faces))
