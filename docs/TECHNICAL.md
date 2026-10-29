# RatLabs Mesh: technical notes

The engineering reference. For the overview and quick start, see the [README](../README.md).

A RatLabs Studio by-product: drop in character art, get back a finished, game-ready 3D model.
Same job as meshy.ai, but RatLabs owns every stage except the GPU step. One Rust binary (`ratmesh`)
holds the studio, the pipeline, the CLI and an MCP server; Blender does the mesh work.

```sh
ratmesh serve                 # studio UI on http://127.0.0.1:8771, MCP at /mcp
ratmesh engines               # which engines are ready
ratmesh make art.png --kind character --variants 2
ratmesh make lab-scene.png --crop 0.30,0.04,0.62,0.99   # one character out of a scene
ratmesh make front.png --back back.png --engine hunyuan3
ratmesh import old.glb --kind critter --height 0.1      # finish an existing model
ratmesh refinish <job-id> --polys 12000 --rotate 180    # free, no GPU
```

## Architecture

```
crates/
  mesh-core      config + folders, presets, job store (job.json), event bus
  mesh-prep      cut-out: BiRefNet via ONNX Runtime, crop, speck removal, centred square
  mesh-glb       GLB read/write, triangle count, texture shrink, fringe cleanup, review sheet
  mesh-engines   Engine trait; fal.ai (Hunyuan3D v3, TRELLIS.2), Meshy; TOML engine plugins
  mesh-blender   finish / rig / LOD / glTF packing, running embedded Blender scripts
  mesh-market    Poly Haven, ambientCG, Sketchfab, Poly Pizza; material packer
  mesh-pipeline  run / refinish / rig, and the single work queue everything shares
  mesh-mcp       Model Context Protocol tools (transport-free JSON-RPC handler)
  mesh-server    axum: JSON API, /api/events (SSE), /mcp, embedded studio UI
  ratmesh        the binary: serve, mcp (stdio bridge), make, rig, doctor, setup, ...
web/js/          studio UI as ES modules: api, state, viewer, studio, job, market, keys, live, connect
local/           local Hunyuan3D-2 setup (embedded; run by `ratmesh setup hunyuan`)
plugins/         example engine plugins
```

What stays Python, and why: the Blender scripts (`mesh-blender/scripts/`) run inside Blender's own
Python, and the local engines (`mesh-engines/sidecars/`) are PyTorch. Both are embedded in the binary
and written to `~/.local/share/ratlabs-mesh/runtime/` before they run.

**One queue.** The studio, AI clients and the API all submit to one worker, so jobs never fight over the
CPU or RAM. `ratmesh mcp` doesn't run jobs itself: it forwards stdio JSON-RPC to the studio's `/mcp`
(starting the studio if needed), so everything an assistant does shows up live in the window. CLI
commands (`make`, `rig`, ...) run in their own process; the studio picks their jobs up within 30 s.

**Live updates.** Every job save emits an event on the bus; the UI listens on `/api/events`. AI tool
calls emit `agent` events (the header chip) and `show` events (`view_model`, `show_in_studio`, new
jobs), which bring the job up in the viewer.

**Local only.** The server binds `127.0.0.1` and rejects requests whose `Host` or `Origin` isn't this
machine, so a web page can't reach the API or MCP through DNS rebinding.

## How it works (and how Meshy does it)

Meshy's image-to-3D is a chain of stages. RatLabs Mesh runs the same chain:

| Stage | Meshy | RatLabs Mesh | Where |
|---|---|---|---|
| 1. Prep | image enhancement, implicit cut-out | BiRefNet cut-out, crop box for scene art, speck removal, centred 1024² square with 8% margin, quality notes | local CPU (`mesh-prep`) |
| 2. Generate | Meshy 6/7 diffusion model | Hunyuan3D-2 shape + multi-view paint locally, or TRELLIS.2 / Hunyuan3D v3 / Meshy remotely; 1–4 seed variants | this CPU or a remote GPU (`mesh-engines`) |
| 3. Remesh | `should_remesh`, `target_polycount` | weld, drop floaters, collapse-decimate to the triangle budget, smooth-by-angle normals | Blender (`finish_blender.py`) |
| 4. Size | `auto_size`, `origin_at` | real-world height per kind, feet-at-origin or centred pivot | Blender |
| 5. Materials | PBR maps, `remove_lighting` | keeps PBR maps, fixes glTF's chrome default (metallic 1 → 0, roughness 1 → 0.8), caps texture size, JPEG-packed | Blender |
| 6. Review | 4 thumbnails | front / ¾ / side / back renders on near-black, transparent PNGs + sheet | Blender EEVEE |
| 7. Rig | auto-rig + animation library | `ratmesh rig <job>`: auto 22-bone skeleton, voxel-proxy skinning, 88 CC0 Quaternius clips; needs an A/T-pose biped | Blender (`rig_blender.py`) |

The finished file matches the reference set (`rat-mesh.glb`, `goblin-mesh.glb`,
`model.glb`): one mesh, ~30k triangles, one 2K base-colour texture, metallic 0,
roughness 0.8, real height (0.1 / 1.2 / 1.7 m).

### Kinds (presets)

| kind | height | triangles | texture |
|---|---|---|---|
| character | 1.7 m | 30,000 | 2K |
| creature | 1.2 m | 30,000 | 2K |
| critter | 0.1 m | 30,000 | 2K |
| prop | 0.5 m | 15,000 | 2K |
| hero | 1.7 m | 80,000 | 4K |

All of them can be overridden per job, and **Refinish** re-runs stages 3 to 6 on
the stored raw GLB without spending GPU money.

## Engines

| engine | model | needs | notes |
|---|---|---|---|
| `hunyuan-local` | Tencent Hunyuan3D-2 shape + Paint, **on this CPU** | `ratmesh setup hunyuan` | free and offline. Shape ~6 min, paint ~20 min (about 27 min a model). Real painted backs, ~30k tris, 2K texture. Multi-view shape model when back/left/right art is given |
| `trellis2` | Microsoft TRELLIS.2, MIT license | `FAL_KEY` | best open model; bakes texture after decimating; ~$0.25–0.35 per model on fal.ai; about a minute |
| `hunyuan3` | Tencent Hunyuan3D v3 | `FAL_KEY` | accepts back/left/right views: use for turnarounds |
| `meshy` | Meshy latest | `MESHY_API_KEY` | the reference quality; multi-image when views are given; `--pose a-pose` |
| `trellis2-hf` | TRELLIS.2 official HF Space | `HF_TOKEN` | example plugin in `plugins/trellis2-hf/`; needs HF PRO: the free tier caps a job at well under the 270 s it asks for |
| `triposr` | TripoSR on this CPU | TripoSR checkout with a `.venv` | ~90 s coarse vertex-colour drafts for checking a cut-out and pose; never final |

### The local engine

This machine has Intel integrated graphics, no NVIDIA GPU. TRELLIS.2 needs NVIDIA-only kernels, but
Hunyuan3D-2 is plain PyTorch apart from one texture rasterizer that has an upstream CPU path:

```sh
ratmesh setup hunyuan        # clone, pinned venv, CPU rasterizer, patch (existing ~/tools installs are found)
ratmesh make art.png --engine hunyuan-local                  # ~25 min
ratmesh make art.png --engine hunyuan-local --quality draft  # 0.6B mini shape model
```

What the setup does, and why:
- `transformers<5`: version 5 renames DINOv2 weights and the shape checkpoint stops loading.
- `local/custom_rasterizer_cpu/`: builds the rasterizer as a C++ extension with the CUDA kernel stubbed out.
- `local/patch_hunyuan.py`: replaces hard-coded `'cuda'` and fp16 with `HY3D_DEVICE` / `HY3D_DTYPE`
  (default cpu / float32, because this CPU has no fast fp16). Idempotent; asserts every edit.
- The FlashVDM turbo VAE loads in fp16, so the driver casts the pipeline to float32 *after* enabling it.
- The mesh is reduced to the triangle budget *before* painting, so the texture is baked onto the final topology.
- Quality: `draft` = 0.6B mini model at octree 256; `standard` = 1.1B model at 320, which keeps fur strands
  and whiskers the mini loses (rat test) in about the same shape time (~7.5 min); `high` = 1.1B at 380,
  upstream's setting, not yet timed on this CPU.
- Variants of local engines run one at a time (each needs ~16 GB RAM).
- Delight (removing baked lighting from the art) runs 20 steps, not upstream's 50: about 2.5x faster, and
  on RatLabs art 50 steps oversaturated toward orange and invented coloured goggle lenses.

Where the ~27 minutes go (goblin, CPU): shape 6–7 min; paint 19 min = multi-view diffusion 13 min, delight
5 min (2 min at 20 steps), bake + inpaint 1 min, UV + renders 12 s.

**Remove baked lighting** (UI checkbox, `--keep-lighting` to turn off) mirrors Meshy's `remove_lighting`.
On gives an evenly lit texture that relights well in a game. Off keeps the art's own dark, moody colour,
which is often closer to RatLabs art, and skips delight.

Weights (~22 GB) live in `~/.cache/huggingface`. License: Tencent Hunyuan 3D 2.0 Community License:
commercial use allowed under 1M monthly users, **not licensed in the EU, UK or South Korea**.

Speed-up not yet taken: `sudo pacman -S intel-compute-runtime` enables PyTorch on the Arc iGPU (XPU).

Keys for the remote engines go in `~/.config/ratlabs-mesh/keys.env` (mode 600) or through **Keys** in the UI:

```
FAL_KEY=...
MESHY_API_KEY=...
HF_TOKEN=...
```

Remote engine requests send the prepared cut-out to that provider. `hunyuan-local` sends nothing anywhere.

## Getting a perfect character

1. **One subject, whole body, plain pose.** Box the character if the art is a scene.
   Click **Preview cut-out** before generating; a fuzzy mask warning means box tighter.
2. **Front-facing, slightly A-posed arms** come out cleanest and rig best.
3. **Generate 2–4 variants** and keep the best: seeds differ a lot on backs and hands.
4. **Give it a back view** (`hunyuan3` or `meshy`) when the back matters.
5. Use **hero** for close-ups (80k tris, 4K texture), **character** for the game.

## Starter pack

`tools/build-starter-pack.py` packs the 30 CC0 Poly Haven props (from the free-props package's finished
jobs) into `starter-pack.zip`: finished GLB, stats, a thumbnail and the review sheet per item, plus
licence/author/source. Upload it once to a GitHub release tagged `starter-pack-v1`; the app downloads it
from there when someone presses **Load starter pack** (`RATMESH_STARTER` overrides the URL or points at
a local zip). The script refuses anything that isn't CC0.

## Job folders

```
jobs/<id>/      job.json, job.log, source.*, views/, prep.png, prep_hi.png,
                v1/raw.glb, v1/<name>.glb, v1/<name>-rigged.glb, v1/renders/{front,three_quarter,side,back}.png + sheet.jpg
```

job.json is the same layout the Python version wrote, so older jobs open unchanged (point `jobs` in
config.toml at the old folder).

## Next

- Intel XPU (Arc iGPU) for the local engine, once `intel-compute-runtime` is installed.
- A persistent local worker so the ~1 min model load happens once, not per job.
- MCP progress notifications while a job runs (today clients poll with `get_job` + `wait_seconds`).
- A "Send to game" button that writes into `ratlabs-underground/assets/models/`.
