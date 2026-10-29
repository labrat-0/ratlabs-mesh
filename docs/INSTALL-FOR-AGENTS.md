# Installing RatLabs Mesh: instructions for AI agents

You are an AI agent helping a person install **RatLabs Mesh**, a local studio that turns character art
into finished, game-ready 3D models, and connect yourself to it over MCP. Follow the section for their
operating system. Rules:

- Install only RatLabs Mesh itself without asking. **Ask before** installing anything else (Blender,
  package managers) and before any command that needs administrator rights.
- Generating models with paid engines costs the person money (about $0.25–0.38 per model). Never start
  one in this setup.
- Report what you did and what is still missing at the end.

## Linux

```sh
curl -fsSL https://raw.githubusercontent.com/labrat-0/ratlabs-mesh/main/install.sh | sh
```

Installs `~/.local/bin/ratmesh` and a **RatLabs Mesh** entry in the app launcher (with a background
service). The prebuilt binary needs glibc 2.39+ (Ubuntu 24.04, Debian 13, Fedora 40 or newer); on older
distros the installer says so: build from source with
`cargo install --git https://github.com/labrat-0/ratlabs-mesh ratmesh` (needs Rust and a C++ compiler). If Blender is missing, offer the command for their distro: `sudo apt install blender`,
`sudo dnf install blender`, `sudo pacman -S blender`, or `flatpak install flathub org.blender.Blender`.
If `~/.local/bin` isn't on PATH, call it by full path or offer to add it.

## macOS (Apple Silicon)

```sh
curl -fsSL https://raw.githubusercontent.com/labrat-0/ratlabs-mesh/main/install.sh | sh
```

Installs `~/.local/bin/ratmesh`. If Blender is missing, offer `brew install --cask blender` (needs
Homebrew) or a download from blender.org. Intel Macs: `cargo install --git https://github.com/labrat-0/ratlabs-mesh ratmesh`.

## Windows

```powershell
irm https://raw.githubusercontent.com/labrat-0/ratlabs-mesh/main/install.ps1 | iex
```

Installs `%LOCALAPPDATA%\Programs\RatLabs Mesh\ratmesh.exe`, adds it to the user PATH and the Start
menu. A terminal opened before the install won't see the new PATH: use the full path or a new
terminal. If Blender is missing, offer `winget install BlenderFoundation.Blender`.

## Then, on every system

1. `ratmesh doctor` checks Blender, the engines, models and folders. Tell the person what's missing,
   and that the first cut-out downloads a ~970 MB model once.
2. Connect yourself. `ratmesh connect` prints the setup for every client. The endpoint is
   `http://127.0.0.1:8771/mcp`, or run `ratmesh mcp` as a stdio server. For example:
   - Claude Code: `claude mcp add -s user --transport http ratlabs-mesh http://127.0.0.1:8771/mcp`
   - Grok Build: `grok mcp add ratlabs-mesh http://127.0.0.1:8771/mcp`
   - Codex CLI, Cursor, VS Code, Gemini CLI, Claude Desktop: see `ratmesh connect`

   Many clients load new MCP servers only at start: tell the person to restart you if the tools don't appear.
3. `ratmesh open` starts the studio and opens it in their browser.
4. Once connected, call the `mesh_status` tool and summarise which engines are ready. To try
   everything for free, offer `load_starter_pack` (30 CC0 props and a rigged character). For paid
   generation they need a fal.ai or Meshy key: they paste it under **Keys** in the studio. Don't ask
   them to paste keys into the chat.
