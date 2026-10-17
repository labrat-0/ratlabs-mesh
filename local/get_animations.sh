#!/usr/bin/env bash
# Fetch the Quaternius Universal Animation Library 1 + 2 (Standard editions, CC0) for `ratmesh rig`.
set -euo pipefail
dest="${RATMESH_ANIM:-$HOME/tools/anim}"
mkdir -p "$dest"
for z in universal_animation_librarystandard universal_animation_library_2standard; do
  [ -d "$dest/$z" ] && continue
  curl -fsSLo "$dest/$z.zip" "https://opengameart.org/sites/default/files/$z.zip"
  unzip -qo "$dest/$z.zip" -d "$dest/$z"
done
echo "animation libraries in $dest"
