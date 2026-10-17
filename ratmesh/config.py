"""Paths, presets and provider keys for RatLabs Mesh."""
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
JOBS = Path(os.environ.get("RATMESH_JOBS", ROOT / "jobs"))
KEYS_FILE = Path.home() / ".config/ratlabs-mesh/keys.env"
BLENDER = os.environ.get("RATMESH_BLENDER", "blender")
TRIPOSR = Path(os.environ.get("RATMESH_TRIPOSR", Path.home() / "tools/TripoSR"))
HUNYUAN = Path(os.environ.get("RATMESH_HUNYUAN", Path.home() / "tools/Hunyuan3D-2"))
# Quaternius Universal Animation Library 1 + 2 (CC0), fetched by local/get_animations.sh
ANIM = Path(os.environ.get("RATMESH_ANIM", Path.home() / "tools/anim"))
ANIM_LIBS = [
    ANIM / "universal_animation_librarystandard/Animation Library[Standard]/Godot/AnimationLibrary_Godot_Standard.glb",
    ANIM / "universal_animation_library_2standard/Universal Animation Library 2 [Standard]/Unreal-Godot/UAL2_Standard.glb",
]

# What a finished asset should look like, per kind. The three reference GLBs
# (rat 0.1 m, goblin 1.2 m, labrat 1.7 m) are ~30k triangles with one 2K texture.
PRESETS = {
    "character": {"height": 1.7, "polys": 30000, "texture": 2048, "origin": "bottom", "label": "Character (humanoid)"},
    "creature":  {"height": 1.2, "polys": 30000, "texture": 2048, "origin": "bottom", "label": "Creature"},
    "critter":   {"height": 0.1, "polys": 30000, "texture": 2048, "origin": "bottom", "label": "Small critter (rat-sized)"},
    "prop":      {"height": 0.5, "polys": 15000, "texture": 2048, "origin": "bottom", "label": "Prop / object"},
    "hero":      {"height": 1.7, "polys": 80000, "texture": 4096, "origin": "bottom", "label": "Hero asset (close-up)"},
}


def load_keys():
    """Read KEY=value lines from the keys file; real environment variables win."""
    keys = {}
    if KEYS_FILE.exists():
        for line in KEYS_FILE.read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, v = line.split("=", 1)
                keys[k.strip()] = v.strip().strip('"').strip("'")
    for k in ("FAL_KEY", "MESHY_API_KEY", "HF_TOKEN"):
        if os.environ.get(k):
            keys[k] = os.environ[k]
    return keys
