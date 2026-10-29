//! What a finished asset should look like, per kind. The three reference GLBs
//! (rat 0.1 m, goblin 1.2 m, labrat 1.7 m) are ~30k triangles with one 2K texture.
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Preset {
    #[serde(skip)]
    pub kind: &'static str,
    pub height: f64,
    pub polys: u32,
    pub texture: u32,
    pub origin: &'static str,
    pub label: &'static str,
}

pub const PRESETS: &[Preset] = &[
    Preset { kind: "character", height: 1.7, polys: 30000, texture: 2048, origin: "bottom", label: "Character (humanoid)" },
    Preset { kind: "creature", height: 1.2, polys: 30000, texture: 2048, origin: "bottom", label: "Creature" },
    Preset { kind: "critter", height: 0.1, polys: 30000, texture: 2048, origin: "bottom", label: "Small critter (rat-sized)" },
    Preset { kind: "prop", height: 0.5, polys: 15000, texture: 2048, origin: "bottom", label: "Prop / object" },
    Preset { kind: "hero", height: 1.7, polys: 80000, texture: 4096, origin: "bottom", label: "Hero asset (close-up)" },
];

pub fn preset(kind: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.kind == kind)
}

/// `{kind: preset}` in display order, as the studio and AI clients read it.
pub fn as_json() -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for p in PRESETS {
        m.insert(p.kind.into(), serde_json::to_value(p).unwrap());
    }
    serde_json::Value::Object(m)
}

/// Engine generation quality levels, lowest first.
pub const QUALITY: &[&str] = &["draft", "standard", "high"];
