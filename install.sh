#!/bin/sh
# Install RatLabs Mesh on Linux or macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/labrat-0/ratlabs-mesh/main/install.sh | sh
#
# Downloads the latest release for this machine, checks it against SHA256SUMS, installs
# `ratmesh` to ~/.local/bin, adds the app-launcher entry on Linux, and runs `ratmesh doctor`.
# Nothing else is installed: missing pieces (Blender) are reported with the command to get them.
#
# RATMESH_VERSION=v0.2.0   install that release instead of the latest
# RATMESH_ARCHIVE=path     install from a local archive (offline / testing)
# RATMESH_NO_DESKTOP=1     skip the Linux launcher entry and background service
# RATMESH_DOWNLOAD_BASE=url download from a mirror instead of GitHub Releases
set -eu

REPO="labrat-0/ratlabs-mesh"
BIN_DIR="${HOME}/.local/bin"
say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

case "$(uname -s)" in
  Linux) os=unknown-linux-gnu ;;
  Darwin) os=apple-darwin ;;
  *) die "this installer is for Linux and macOS; on Windows use install.ps1" ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=aarch64 ;;
  *) die "unsupported CPU $(uname -m)" ;;
esac
[ "$os" = apple-darwin ] && [ "$arch" = x86_64 ] && die "Intel Macs: build from source with \`cargo install --git https://github.com/$REPO ratmesh\`"
target="$arch-$os"
name="ratmesh-$target"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if [ -n "${RATMESH_ARCHIVE:-}" ]; then
  say "Installing from $RATMESH_ARCHIVE"
  cp "$RATMESH_ARCHIVE" "$tmp/$name.tar.gz"
else
  if [ -n "${RATMESH_DOWNLOAD_BASE:-}" ]; then base="$RATMESH_DOWNLOAD_BASE"
  elif [ -n "${RATMESH_VERSION:-}" ]; then base="https://github.com/$REPO/releases/download/$RATMESH_VERSION"; else base="https://github.com/$REPO/releases/latest/download"; fi
  say "Downloading RatLabs Mesh for $target"
  curl -fL --progress-bar -o "$tmp/$name.tar.gz" "$base/$name.tar.gz" || die "download failed: $base/$name.tar.gz"
  curl -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS" || die "could not fetch SHA256SUMS"
  want="$(grep " $name.tar.gz\$" "$tmp/SHA256SUMS" | cut -d' ' -f1)"
  if command -v sha256sum >/dev/null; then got="$(sha256sum "$tmp/$name.tar.gz" | cut -d' ' -f1)"; else got="$(shasum -a 256 "$tmp/$name.tar.gz" | cut -d' ' -f1)"; fi
  [ -n "$want" ] && [ "$want" = "$got" ] || die "checksum mismatch for $name.tar.gz"
  say "Checksum OK"
fi

tar xzf "$tmp/$name.tar.gz" -C "$tmp"
mkdir -p "$BIN_DIR"
install -m 755 "$tmp/$name/ratmesh" "$BIN_DIR/ratmesh"
[ "$os" = apple-darwin ] && xattr -d com.apple.quarantine "$BIN_DIR/ratmesh" 2>/dev/null || true
say "Installed $BIN_DIR/ratmesh ($("$BIN_DIR/ratmesh" --version))"

if [ "$os" = unknown-linux-gnu ] && [ -z "${RATMESH_NO_DESKTOP:-}" ] && systemctl --user show-environment >/dev/null 2>&1; then
  say "Adding RatLabs Mesh to your app launcher"
  "$BIN_DIR/ratmesh" install-desktop || say "launcher setup failed; start it any time with: ratmesh open"
fi

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) say "Add ~/.local/bin to your PATH to type \`ratmesh\`:  echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.profile" ;;
esac

echo
"$BIN_DIR/ratmesh" doctor || true
echo
if ! "$BIN_DIR/ratmesh" doctor 2>/dev/null | grep -q "✓ Blender"; then
  if [ "$os" = apple-darwin ]; then
    say "RatLabs Mesh needs Blender 4.2+: brew install --cask blender   (or blender.org/download)"
  else
    say "RatLabs Mesh needs Blender 4.2+: sudo pacman -S blender | sudo apt install blender | sudo dnf install blender | flatpak/snap, or blender.org/download"
  fi
fi
say "Open the studio:  ratmesh open"
say "Connect your AI:  ratmesh connect"
