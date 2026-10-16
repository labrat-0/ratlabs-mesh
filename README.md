# RatLabs Mesh

Private. A RatLabs Studio by-product: drop in character art, get back a finished,
game-ready 3D model. Same job as meshy.ai, but RatLabs owns every stage except the
GPU step.

```sh
./ratmesh.sh serve                 # studio UI on http://127.0.0.1:8771
./ratmesh.sh engines               # which engines have keys
./ratmesh.sh make art.png --kind character --variants 2
./ratmesh.sh make lab-scene.png --crop 0.30,0.04,0.62,0.99   # one character out of a scene
./ratmesh.sh make front.png --back back.png --engine hunyuan3
./ratmesh.sh import old.glb --kind critter --height 0.1      # finish an existing model
./ratmesh.sh refinish <job-id> --polys 12000 --rotate 180    # free, no GPU
```

## How it works (and how Meshy does it)

Meshy's image-to-3D is a chain of stages. RatLabs Mesh runs the same chain:

| Stage | Meshy | RatLabs Mesh | Where |
|---|---|---|---|
| 1. Prep | image enhancement, implicit cut-out | BiRefNet cut-out, crop box for scene art, speck removal, centred 1024² square with 8% margin, quality notes | local CPU (`ratmesh/prep.py`) |
| 2. Generate | Meshy 6/7 diffusion model | TRELLIS.2 / Hunyuan3D v3 / Meshy itself, 1–4 seed variants in parallel | remote GPU (`ratmesh/engines.py`) |
| 3. Remesh | `should_remesh`, `target_polycount` | weld, drop floaters, collapse-decimate to the triangle budget, smooth-by-angle normals | Blender (`ratmesh/finish_blender.py`) |
| 4. Size | `auto_size`, `origin_at` | real-world height per kind, feet-at-origin or centred pivot | Blender |
| 5. Materials | PBR maps, `remove_lighting` | keeps PBR maps, fixes glTF's chrome default (metallic 1 → 0, roughness 1 → 0.8), caps texture size, JPEG-packed | Blender |
| 6. Review | 4 thumbnails | front / ¾ / side / back renders on near-black, transparent PNGs + sheet | Blender EEVEE |
| 7. Rig | auto-rig + animation library | not yet (see Next) | n/a |

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

This machine has Intel integrated graphics, and every model good enough to
match the references needs a 16–24 GB NVIDIA GPU. So stage 2 is remote:

| engine | model | key | notes |
|---|---|---|---|
| `trellis2` (default) | Microsoft TRELLIS.2, MIT license | `FAL_KEY` | best open model; bakes texture after decimating; ~$0.25–0.35 per model on fal.ai |
| `hunyuan3` | Tencent Hunyuan3D v3 | `FAL_KEY` | accepts back/left/right views: use for turnarounds |
| `meshy` | Meshy latest | `MESHY_API_KEY` | the reference quality; multi-image when views are given; `--pose a-pose` |
| `trellis2-hf` | TRELLIS.2 official HF Space | `HF_TOKEN` | needs HF PRO: the free tier caps a job at well under the 270 s it asks for |
| `triposr` | TripoSR on this CPU | none | free, offline, ~90 s. Coarse vertex-colour drafts for checking a cut-out and pose; never final |

Keys go in `~/.config/ratlabs-mesh/keys.env` (mode 600) or through **Keys** in the UI:

```
FAL_KEY=...
MESHY_API_KEY=...
HF_TOKEN=...
```

Engine requests send the prepared cut-out to that provider.

## Getting a perfect character

1. **One subject, whole body, plain pose.** Box the character if the art is a scene.
   Click **Preview cut-out** before generating; a fuzzy mask warning means box tighter.
2. **Front-facing, slightly A-posed arms** come out cleanest and rig best.
3. **Generate 2–4 variants** and keep the best: seeds differ a lot on backs and hands.
4. **Give it a back view** (`hunyuan3` or `meshy`) when the back matters.
5. Use **hero** for close-ups (80k tris, 4K texture), **character** for the game.

## Layout

```
ratmesh/        config, prep, engines, finish_blender, jobs, cli, server, triposr_cpu
web/            studio UI (three.js r160 vendored)
samples/input/  RatLabs art used for testing
jobs/<id>/      per-job output (git-ignored): source, prep.png, v1/raw.glb, v1/<name>.glb, v1/renders/
```

## Next

- Auto-rig stage: generalise `ratlabs-underground/tools/mesh-engine/rig-meshy.py`
  (humanoid) and `rig-rat.py` (quadruped), or call Meshy's rigging API.
- Self-hosted TRELLIS.2 worker (RunPod or Modal serverless) to drop per-model fees.
- A "Send to game" button that writes into `ratlabs-underground/assets/models/`.
