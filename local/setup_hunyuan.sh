#!/usr/bin/env bash
# Install the local Hunyuan3D-2 engine (CPU) for RatLabs Mesh.
#   ./local/setup_hunyuan.sh [checkout dir, default ~/tools/Hunyuan3D-2]
# Weights (~15 GB) download on first use into ~/.cache/huggingface.
set -euo pipefail
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
DIR="${1:-$HOME/tools/Hunyuan3D-2}"
COMMIT=f8db630
[[ -d $DIR/.git ]] || git clone -q https://github.com/Tencent-Hunyuan/Hunyuan3D-2.git "$DIR"
git -C "$DIR" fetch -q --depth 50 origin 2>/dev/null || true
git -C "$DIR" checkout -q "$COMMIT" -- . 2>/dev/null || echo "note: could not pin $COMMIT, using checkout as is"
[[ -x $DIR/.venv/bin/python ]] || uv venv -q --python 3.12 "$DIR/.venv"
PY="$DIR/.venv/bin/python"
uv pip install -q --python "$PY" --index-url https://download.pytorch.org/whl/cpu --extra-index-url https://pypi.org/simple -r "$HERE/requirements-hunyuan.txt"

# The texture rasterizer has a CPU path upstream; only its build assumes CUDA.
B="$DIR/cpu_build/custom_rasterizer"
SRC="$DIR/hy3dgen/texgen/custom_rasterizer"
mkdir -p "$B/src"
cp "$SRC"/lib/custom_rasterizer_kernel/{rasterizer.cpp,grid_neighbor.cpp,rasterizer.h} "$B/src/"
cp -r "$SRC/custom_rasterizer" "$B/"
cp "$HERE/custom_rasterizer_cpu/setup.py" "$HERE/custom_rasterizer_cpu/gpu_stub.cpp" "$B/"
mv "$B/gpu_stub.cpp" "$B/src/"
sed -i 's|#include <ATen/cuda/CUDAContext.h> // For CUDA context|#ifndef __host__\n#define __host__\n#endif\n#ifndef __device__\n#define __device__\n#endif|' "$B/src/rasterizer.h"
uv pip install -q --python "$PY" --no-build-isolation "$B"
uv pip install -q --python "$PY" --no-build-isolation "$DIR/hy3dgen/texgen/differentiable_renderer"

"$PY" "$HERE/patch_hunyuan.py" "$DIR"
(cd / && "$PY" -c "import custom_rasterizer_kernel, mesh_processor; print('ok: Hunyuan3D-2 CPU engine ready at $DIR')")
