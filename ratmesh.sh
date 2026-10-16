#!/usr/bin/env bash
# RatLabs Mesh launcher: ./ratmesh.sh serve | make art.png | engines | ...
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"
if [ ! -x .venv/bin/python ]; then
  uv venv -q --python 3.12 .venv
  uv pip install -q --python .venv/bin/python -r requirements.txt
fi
exec .venv/bin/python -m ratmesh "$@"
