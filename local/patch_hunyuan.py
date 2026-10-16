"""Make a Hunyuan3D-2 checkout run without CUDA.

Upstream hard-codes 'cuda' and fp16 in the texture pipeline. This swaps them for
HY3D_DEVICE (default cpu) and HY3D_DTYPE (default float32: this CPU has no fast
fp16). Run once per checkout; it is idempotent and asserts every edit applies.

    python local/patch_hunyuan.py ~/tools/Hunyuan3D-2
"""
import sys
from pathlib import Path

root = Path(sys.argv[1]).expanduser() / "hy3dgen" / "texgen"
DEV = "os.environ.get('HY3D_DEVICE', 'cpu')"
DT = "getattr(torch, os.environ.get('HY3D_DTYPE', 'float32'))"

EDITS = {
    "pipelines.py": [
        ("self.device = 'cuda'", f"self.device = {DEV}"),
        ("        torch.cuda.empty_cache()\n        # Load model", "        if torch.cuda.is_available():\n            torch.cuda.empty_cache()\n        # Load model"),
        ("            texture_size=self.config.texture_size)", "            texture_size=self.config.texture_size, device=self.config.device)"),
    ],
    "utils/multiview_utils.py": [
        ("custom_pipeline=custom_pipeline_path, torch_dtype=torch.float16)", f"custom_pipeline=custom_pipeline_path, torch_dtype={DT})"),
    ],
    "utils/dehighlight_utils.py": [
        ("            torch_dtype=torch.float16,\n            safety_checker=None,", f"            torch_dtype={DT},\n            safety_checker=None,"),
        ("pipeline.to(self.device, torch.float16)", f"pipeline.to(self.device, {DT})"),
    ],
    "hunyuanpaint/pipeline.py": [
        ("            ddim_timesteps=30,\n        ).to('cuda')", f"            ddim_timesteps=30,\n        ).to({DEV})"),
        ('.contiguous().half().to("cuda")', ".contiguous().to(dtype=self.unet.dtype, device=self._execution_device)"),
        ("index = torch.range(29, 0, -bsz, device='cuda').long()", "index = torch.arange(29, -1, -bsz, device=self.solver.ddim_timesteps.device).long()"),
        ("self.scheduler.set_timesteps(timesteps=timesteps.cpu(), device='cuda')", "self.scheduler.set_timesteps(timesteps=timesteps.cpu(), device=self._execution_device)"),
    ],
    "differentiable_renderer/mesh_render.py": [
        ("bake_mode='linear', raster_mode='cr', device='cuda'):", f"bake_mode='linear', raster_mode='cr', device={DEV}):"),
    ],
}

for rel, edits in EDITS.items():
    p = root / rel
    s = p.read_text()
    for old, new in edits:
        if new in s:
            continue
        assert s.count(old) == 1, f"{rel}: expected one match for {old!r}"
        s = s.replace(old, new)
    if "import os\n" not in s:
        s = "import os\n" + s
    p.write_text(s)
    print("patched", rel)
