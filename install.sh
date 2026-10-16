#!/usr/bin/env bash
# Install RatLabs Mesh as a desktop app, the same way RatLabs Studio installs:
#   - .venv with the Python deps (uv)
#   - systemd user service ratlabs-mesh.service on 127.0.0.1:$PORT
#   - ~/.local/bin/ratlabs-mesh launcher (omarchy-launch-webapp window)
#   - desktop entry so it shows in the launcher (Super+Space)
# ./install.sh [--port 8771] | --uninstall
set -euo pipefail
ROOT="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
PORT=8771
UNIT="$HOME/.config/systemd/user/ratlabs-mesh.service"
BIN="$HOME/.local/bin/ratlabs-mesh"
DESKTOP="$HOME/.local/share/applications/ratlabs-mesh.desktop"

case "${1:-}" in
  --port) PORT="$2" ;;
  --uninstall)
    systemctl --user disable --now ratlabs-mesh.service 2>/dev/null || true
    rm -f "$UNIT" "$BIN" "$DESKTOP"
    systemctl --user daemon-reload
    echo "Removed the service and launcher. The repo and jobs/ are untouched."
    exit 0 ;;
esac

for dep in uv blender magick; do
  command -v "$dep" >/dev/null || { echo "missing: $dep"; exit 1; }
done
[[ -x $ROOT/.venv/bin/python ]] || uv venv -q --python 3.12 "$ROOT/.venv"
uv pip install -q --python "$ROOT/.venv/bin/python" -r "$ROOT/requirements.txt"

mkdir -p "$(dirname "$UNIT")" "$(dirname "$BIN")" "$(dirname "$DESKTOP")"
cat > "$UNIT" <<EOF
[Unit]
Description=RatLabs Mesh (image to 3D)

[Service]
Environment=RATMESH_PORT=$PORT
WorkingDirectory=$ROOT
ExecStart=$ROOT/.venv/bin/python -m ratmesh serve --port $PORT
Restart=on-failure

[Install]
WantedBy=default.target
EOF

cat > "$BIN" <<EOF
#!/usr/bin/env bash
# Opens RatLabs Mesh as an app window, starting the local server if needed.
set -euo pipefail
URL="http://127.0.0.1:$PORT/"
up() { curl -fsS -o /dev/null --max-time 1 "\$URL/api/config"; }
if ! up; then
  systemctl --user start ratlabs-mesh.service
  for _ in \$(seq 1 80); do up && break; sleep 0.1; done
  up || { notify-send "RatLabs Mesh" "Server failed to start: journalctl --user -u ratlabs-mesh" 2>/dev/null; exit 1; }
fi
if command -v omarchy-launch-webapp >/dev/null; then exec omarchy-launch-webapp "\$URL"; else exec chromium --app="\$URL"; fi
EOF
chmod +x "$BIN"

cat > "$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Name=RatLabs Mesh
Comment=Turn character art into finished 3D models
Exec=$BIN
Icon=applications-graphics
Terminal=false
Categories=Graphics;3DGraphics;
EOF

systemctl --user daemon-reload
systemctl --user enable ratlabs-mesh.service >/dev/null
systemctl --user restart ratlabs-mesh.service
for _ in $(seq 1 80); do curl -fs -o /dev/null --max-time 1 "http://127.0.0.1:$PORT/api/config" && break; sleep 0.1; done
curl -fsS -o /dev/null "http://127.0.0.1:$PORT/api/config" && echo "✓ RatLabs Mesh is running on http://127.0.0.1:$PORT" || { echo "service did not come up: journalctl --user -u ratlabs-mesh"; exit 1; }
