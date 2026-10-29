# Engine plugins

Any image-to-3D model can be added to RatLabs Mesh with a TOML manifest in
`~/.config/ratlabs-mesh/engines/`. Plugins appear in the studio's engine list, in
`ratmesh engines`, and to every AI client through `mesh_status` / `generate_model`.

- **`type = "command"`**: run any program (a local model, a script calling some API). It gets the
  prepared cut-out and an output path, prints `{"stage": "..."}` lines for progress, and writes a GLB.
  See [`trellis2-hf/`](trellis2-hf/).
- **`type = "fal"`**: call any model hosted on fal.ai by filling in its payload. See
  [`fal-template/`](fal-template/).

The bundled local engines are plugins too:
[`hunyuan-local.toml`](../crates/mesh-engines/manifests/hunyuan-local.toml),
[`triposr.toml`](../crates/mesh-engines/manifests/triposr.toml).
The full field list is in [`plugin.rs`](../crates/mesh-engines/src/plugin.rs).
A plugin whose `id` matches a built-in replaces it.
