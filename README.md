<p align="center"><img src="assets/icon-128.png" width="96" alt="RatLabs Mesh icon"></p>

<h1 align="center">RatLabs Mesh</h1>

<p align="center"><b>Turn character art into finished, game-ready 3D models: by hand, or by asking any AI.</b><br>
Cut-out, 3D generation, cleanup, real-world sizing, review renders and auto-rigging in one local studio.</p>

<p align="center">
<a href="LICENSE"><img alt="Code: MIT" src="https://img.shields.io/badge/code-MIT-39ff88?style=flat-square&labelColor=0e0e0e"></a>
<img alt="Linux, macOS, Windows" src="https://img.shields.io/badge/runs_on-Linux_·_macOS_·_Windows-e8e8e8?style=flat-square&labelColor=0e0e0e">
<a href="https://modelcontextprotocol.io"><img alt="MCP server" src="https://img.shields.io/badge/MCP-any_AI_client-39ff88?style=flat-square&labelColor=0e0e0e"></a>
<img alt="One binary" src="https://img.shields.io/badge/one_binary-Rust-e8e8e8?style=flat-square&labelColor=0e0e0e">
</p>

<p align="center"><img src="docs/media/ai-demo.gif" alt="Claude Code drives RatLabs Mesh: it checks the cut-out of a labrat drawing, generates a 3D model with Hunyuan3D, and the studio shows it in clay mode under neon light while Claude critiques the result"></p>
<p align="center"><sub>Claude Code turning one drawing into a 30k-triangle, 1.7 m character in about four minutes, while the studio follows along. Real time, the generation wait sped up.</sub></p>

```sh
tar xzf ratmesh-*-linux-gnu.tar.gz          # from the Releases page
./ratmesh-*/ratmesh install-desktop         # then open "RatLabs Mesh" from your launcher
```

<p align="center"><a href="#get-started">Get started</a> · <a href="#ask-any-ai-watch-it-build">Connect your AI</a> · <a href="docs/TECHNICAL.md">How it works</a></p>

---

## Ask any AI. Watch it build.

RatLabs Mesh speaks the [Model Context Protocol](https://modelcontextprotocol.io), so any assistant that
supports it can drive the studio: Claude, ChatGPT / Codex, Grok, Cursor, VS Code Copilot, Gemini CLI,
LM Studio or a local model. Ask for *"a rusty oil drum prop at 15k tris, then rig the goblin"*. The
assistant checks the cut-out, generates, **looks at the review renders** (they come back to it as
images), refinishes and rigs. Every job pops up in your studio as it starts, and the header shows which
assistant is working and what it's doing.

<table><tr>
<td width="50%"><img src="docs/screenshots/mcp-tools.png" alt="Claude Code listing the RatLabs Mesh tools"></td>
<td width="50%"><img src="docs/screenshots/connect-ai.png" alt="The Connect AI dialog with setup for Claude Code, Claude Desktop, Cursor, VS Code, Codex, Gemini, Grok and others"></td>
</tr></table>

Click **Connect AI** in the studio, or run `ratmesh connect`, for ready-to-paste setup for each client:

```sh
claude mcp add --transport http ratlabs-mesh http://127.0.0.1:8771/mcp    # Claude Code
grok mcp add ratlabs-mesh http://127.0.0.1:8771/mcp                         # Grok Build
```

```json
{ "mcpServers": { "ratlabs-mesh": { "command": "ratmesh", "args": ["mcp"] } } }
```

The `ratmesh mcp` command starts the studio if it isn't running. Assistants are told which engines cost
money and to ask before spending. Everything stays on your machine; the studio only answers `127.0.0.1`.

**Tools:** `mesh_status`, `preview_cutout`, `generate_model`, `import_model`, `get_job`, `list_jobs`,
`view_model`, `show_in_studio`, `refinish_model`, `rig_model`, `rerun_job`, `concept_art`,
`load_starter_pack`, `market_search`, `market_grab`, `market_library`, `market_finish`.

## From one picture to a model you can ship

<table>
<tr>
<td width="30%" align="center"><img src="docs/screenshots/source-art.jpg" alt="Source art: a goblin scientist in a torn lab coat"><br><sub><b>In:</b> one piece of 2D art</sub></td>
<td width="70%" align="center"><img src="docs/screenshots/review-sheet.jpg" alt="Front, three-quarter, side and back renders of the finished 3D model"><br><sub><b>Out:</b> a textured 3D model, shown front, ¾, side and back</sub></td>
</tr>
</table>

Image-to-3D models give you a raw mesh. That's maybe half the work. RatLabs Mesh does the rest of the
production chain for you, so the file you download can go straight into a game engine:

- **One clean mesh** with stray floaters removed and welded seams
- **The right triangle count**: ~30k for a character, 15k for a prop, 80k for a hero close-up
- **One 2K texture** with sensible PBR settings (no accidental chrome)
- **Real-world size**, with feet on the ground: a character is 1.7 m and a rat is 10 cm
- **Front / ¾ / side / back review renders**, so you can check a model without opening Blender
- **One-click rig + animate**: a 22-bone skeleton and 88 free CC0 animations (idle, walk, run, attack...)

## The studio

<p align="center"><img src="docs/screenshots/studio.png" alt="The RatLabs Mesh studio: new-asset panel, the generated Labrat in the 3D viewer, stats, log and library"></p>

Drop art on the left, pick what it is (**character, creature, critter, prop, hero**), and press **Generate**.
Everything you've made stays in the library along the bottom. Each model comes with its stats, downloads,
and a **Refinish** panel that re-sizes, re-budgets or turns the model around for free, with no GPU.

### Inspect every model three ways

| Texture | Clay | Wire |
|---|---|---|
| ![The textured Labrat](docs/screenshots/labrat-texture.png) | ![The same Labrat in clay shading](docs/screenshots/labrat-clay.png) | ![The same Labrat as a green wireframe](docs/screenshots/labrat-wire.png) |

Clay mode shows the real shape without the paint. Wire mode shows the topology you're shipping. Flip
between **Studio** and **Neon** lighting to see how it holds up in a dark scene.

### Rigged and animated in the browser

<p align="center"><img src="docs/screenshots/labrat-rigged-walk.png" width="480" alt="The rigged Labrat mid-walk under neon lighting"></p>

Press **Rig + animate** on any standing biped: a 22-bone skeleton and 88 free CC0 animations. Pick a clip
from the dropdown to watch it move, then download the rigged GLB. New library? **Load starter pack** adds
30 finished CC0 props and the Labrat above, rigged with its game animations, to try everything for free.

### Market: free assets to fill the rest of the scene

<p align="center"><img src="docs/screenshots/market.png" alt="The Market tab searching Poly Haven for lamps, every result showing its licence and author"></p>

One search across **Poly Haven**, **ambientCG**, **Sketchfab** and **Poly Pizza** for models, materials and
HDRI skies. Every download keeps its licence, author and source page next to it. Any model you grab can go
through the same finishing pipeline, so a scene full of mixed sources ends up at one consistent scale.

---

## Pick your engine

| Engine | Cost | Speed | Best for |
|---|---|---|---|
| **Hunyuan3D v3** (fal.ai) | ~$0.38 a model | a few minutes | Best faces. Takes back / left / right views for turnarounds |
| **TRELLIS.2** (fal.ai) | ~$0.25–0.35 | about a minute | Fast open-model drafts |
| **Meshy** | your Meshy plan | minutes | Reference quality; can force an A- or T-pose |
| **Hunyuan3D-2 local** | **free, fully offline** | ~25 min on a CPU | Nothing leaves your machine. No GPU needed |
| **TripoSR local** | free | ~90 s | Rough previews to check a cut-out and pose |

The local engine runs on a plain CPU (tested on Intel integrated graphics, no NVIDIA card). Remote
engines get only the prepared cut-out of your art; the local engine sends nothing anywhere.

---

## Get started

**You need:** [Blender](https://www.blender.org) 4.2 or newer. That's it: RatLabs Mesh is one binary with
the studio built in.

**Linux:** download `ratmesh-<version>-x86_64-unknown-linux-gnu.tar.gz` from the Releases page, then

```sh
tar xzf ratmesh-*-linux-gnu.tar.gz
./ratmesh-*/ratmesh install-desktop     # installs to ~/.local/bin, adds the launcher entry + background service
```

Open **RatLabs Mesh** from your app launcher. `ratmesh doctor` checks everything and says how to fix
what's missing; `ratmesh install-desktop --uninstall` removes the app (your models stay).

**macOS / Windows:** download the archive for your system and run `ratmesh serve`, then open
http://127.0.0.1:8771.

**From source** (Rust 1.85+): `cargo install --git https://github.com/labrat-0/ratlabs-mesh ratmesh`.

New library? Press **Load starter pack** to add 30 free CC0 props (lab gear, tools, lamps, pipes, crates)
and the **Labrat** from *Rat Underground*, rigged with its game animations. The props are free for
anything; the Labrat is ours, for trying in the studio only.

The first cut-out downloads the BiRefNet background-removal model (~970 MB, once; `ratmesh setup cutout`
fetches it ahead of time).

### Add an engine

- **Paid, fast:** click **Keys** in the top-right corner and paste a [fal.ai](https://fal.ai) key (and/or a
  Meshy key). Keys stay on your machine in `~/.config/ratlabs-mesh/keys.env`.
- **Free, offline:** `ratmesh setup hunyuan` (Linux, needs `uv` and `git`). It downloads ~22 GB of model
  weights and needs ~16 GB of RAM while generating.
- **Any other model:** drop a small TOML manifest in `~/.config/ratlabs-mesh/engines/`. A `command` engine
  runs any program that writes a GLB; a `fal` engine calls any image-to-3D model on fal.ai with no code.
  See [plugins/](plugins/).

For rigging, `ratmesh setup animations` fetches the 88 CC0 Quaternius animations.

### Make your first model

1. **Drop your art** onto the *New asset* panel. If the art is a scene, drag a box around one character.
2. Click **Preview cut-out** to check the background was removed cleanly.
3. Choose a **Kind** (it sets height, triangle budget and texture size) and an **Engine**.
4. Press **Generate**. Try 2 variants when it matters: seeds differ a lot on backs and hands, so you can keep the better one (each variant is a separate generation).
5. **Download GLB**, or press **Rig + animate** first and take the rigged GLB.

### No art yet? Make it with any free AI

Any AI image generator works: ChatGPT, Gemini, Copilot, Grok, Meta AI, Leonardo and the rest, most
with a free tier. For the best models, make **four views of the same character: front, back, left and
right**. Engines that take extra views (Hunyuan3D v3, Meshy, Hunyuan3D-2 local) use them to build real
backs and sides instead of guessing.

Start with the front:

```text
Full-body character design of [describe your character], standing in a relaxed A-pose with arms
slightly away from the body, facing the camera. Whole body in frame, head to feet. Plain light-grey
background, soft even lighting, no floor shadow, no text.
```

Then, in the same chat so the character stays consistent:

```text
Now the exact same character, same outfit and proportions, same A-pose and framing,
seen from the back.        (then again: from the left side, then from the right side)
```

Drop the front into **New asset** and the other three into **Back / Left / Right** under *Extra views*.
Save each view as its own image. If the AI gives you one sheet with all four, crop it into four files.



- **One subject, whole body, facing front.** Arms slightly away from the body rig best.
- **Give it a back view** (with Hunyuan3D v3 or Meshy) when the back matters.
- **Use "hero"** for close-ups (80k triangles, 4K texture) and **"character"** for in-game models.
- Untick **Remove baked lighting** to keep the art's own dark, moody colour in the texture.
- Already have a GLB from somewhere else? Drop it in and RatLabs Mesh will just finish it.

### Prefer the command line?

```sh
ratmesh make art.png --kind character --engine hunyuan3 --variants 2
ratmesh make scene.png --crop 0.30,0.04,0.62,0.99         # one character out of a scene
ratmesh make front.png --back back.png --engine hunyuan3   # sharper backs from a turnaround
ratmesh import old.glb --kind prop                         # finish an existing model
ratmesh refinish <job-id> --polys 12000 --rotate 180       # free re-finish, no GPU
ratmesh rig <job-id>                                       # skeleton + 88 animations
ratmesh concept "a rusty oil drum" --ref style.png         # source art from a description
ratmesh glb shrink model.glb --max 1024                    # web-sized textures
ratmesh engines                                            # which engines are ready
```

### Where things live

| | Linux | override |
|---|---|---|
| Jobs (your models) | `~/.local/share/ratlabs-mesh/jobs` | `RATMESH_JOBS` or `jobs = "..."` |
| Market downloads | `~/.local/share/ratlabs-mesh/library` | `RATMESH_LIBRARY` or `library = "..."` |
| Keys, settings, engine plugins | `~/.config/ratlabs-mesh/` | |

Overrides go in `~/.config/ratlabs-mesh/config.toml` (also `blender`, `hunyuan`, `triposr`, `anim`, `port`).
macOS and Windows use their standard app-data folders.

---

## Under the hood

How each stage works, how it lines up with Meshy's pipeline, the crate layout, presets, engines and timings:
**[docs/TECHNICAL.md](docs/TECHNICAL.md)**.

**Licences.** The code is [MIT](LICENSE). The art in this repo (the Rat Underground characters,
samples, renders and the RatLabs logo) is © RatLabs, all rights reserved: see [ART-LICENSE.md](ART-LICENSE.md).
Models you make with RatLabs Mesh are yours. The local engine uses Tencent's Hunyuan3D-2, under the Hunyuan 3D
2.0 Community License, which allows commercial use under 1M monthly users but **does not cover the EU, UK or
South Korea**. Animations are CC0 (Quaternius). Market assets keep their own licences, saved with each download.

<p align="center"><sub>Built by <b>RatLabs</b> · <a href="https://ratlabs.tech">ratlabs.tech</a></sub></p>
