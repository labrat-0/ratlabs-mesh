//! GLB files without Blender: read and write the container, count triangles, shrink
//! embedded textures for the web, scrub cut-out fringe colours, and build review sheets.
use std::path::Path;

use anyhow::{Result, bail};
use serde_json::Value;

mod defringe;
mod montage;
mod shrink;

pub use defringe::defringe;
pub use montage::review_sheet;
pub use shrink::shrink;

/// A GLB v2 split into its JSON document and binary chunk.
pub struct Glb {
    pub json: Value,
    pub bin: Vec<u8>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()))
}

impl Glb {
    pub fn read(path: &Path) -> Result<Glb> {
        Self::parse(&std::fs::read(path)?).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
    }

    pub fn parse(b: &[u8]) -> Result<Glb> {
        if b.len() < 20 || &b[..4] != b"glTF" || u32_at(b, 4) != Some(2) {
            bail!("not a GLB v2");
        }
        let jlen = u32_at(b, 12).unwrap() as usize;
        let json = serde_json::from_slice(b.get(20..20 + jlen).ok_or_else(|| anyhow::anyhow!("truncated JSON chunk"))?)?;
        let off = 20 + jlen;
        let bin = match u32_at(b, off) {
            Some(blen) => b.get(off + 8..off + 8 + blen as usize).unwrap_or_default().to_vec(),
            None => vec![],
        };
        Ok(Glb { json, bin })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut jb = serde_json::to_vec(&self.json).unwrap();
        while !jb.len().is_multiple_of(4) {
            jb.push(b' ');
        }
        let mut bin = self.bin.clone();
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let total = 12 + 8 + jb.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(jb.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&jb);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        Ok(std::fs::write(path, self.to_bytes())?)
    }

    /// Rebuild the binary chunk, letting `edit` replace any bufferView's bytes
    /// (re-encoded textures). Offsets are re-packed with 4-byte alignment.
    pub fn rewrite_views(&mut self, mut edit: impl FnMut(usize, &[u8], &mut Value) -> Option<Vec<u8>>) -> Result<()> {
        let views = self.json.get("bufferViews").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let mut out = Vec::with_capacity(self.bin.len());
        let mut new_views = Vec::with_capacity(views.len());
        for (i, mut bv) in views.into_iter().enumerate() {
            let start = bv.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
            let len = bv.get("byteLength").and_then(Value::as_u64).unwrap_or(0) as usize;
            let Some(data) = self.bin.get(start..start + len) else { bail!("bufferView {i} is out of range") };
            let data = edit(i, data, &mut self.json).unwrap_or_else(|| data.to_vec());
            while out.len() % 4 != 0 {
                out.push(0);
            }
            bv["byteOffset"] = out.len().into();
            bv["byteLength"] = data.len().into();
            out.extend_from_slice(&data);
            new_views.push(bv);
        }
        self.json["bufferViews"] = Value::Array(new_views);
        if let Some(b) = self.json.get_mut("buffers").and_then(|b| b.get_mut(0)) {
            b["byteLength"] = out.len().into();
        }
        self.bin = out;
        Ok(())
    }

    /// `{bufferView index: image index}` for images embedded in the binary chunk.
    pub fn image_views(&self) -> std::collections::HashMap<usize, usize> {
        self.json["images"]
            .as_array()
            .map(|imgs| imgs.iter().enumerate().filter_map(|(i, im)| im["bufferView"].as_u64().map(|v| (v as usize, i))).collect())
            .unwrap_or_default()
    }
}

/// Triangle count from a GLB's JSON chunk, without loading the mesh. 0 when unreadable.
pub fn tris(path: &Path) -> u64 {
    let Ok(b) = std::fs::read(path) else { return 0 };
    let Some(jlen) = u32_at(&b, 12) else { return 0 };
    let Some(j) = b.get(20..20 + jlen as usize).and_then(|s| serde_json::from_slice::<Value>(s).ok()) else { return 0 };
    let acc = &j["accessors"];
    let mut n = 0;
    for m in j["meshes"].as_array().into_iter().flatten() {
        for p in m["primitives"].as_array().into_iter().flatten() {
            let a = p.get("indices").or_else(|| p["attributes"].get("POSITION")).and_then(Value::as_u64);
            n += a.and_then(|a| acc[a as usize]["count"].as_u64()).unwrap_or(0) / 3;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Glb {
        Glb {
            json: json!({
                "asset": {"version": "2.0"},
                "buffers": [{"byteLength": 7}],
                "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 3}, {"buffer": 0, "byteOffset": 3, "byteLength": 4}],
                "accessors": [{"count": 36}, {"count": 9}],
                "meshes": [{"primitives": [{"indices": 0, "attributes": {"POSITION": 1}}, {"attributes": {"POSITION": 1}}]}],
            }),
            bin: vec![1, 2, 3, 4, 5, 6, 7],
        }
    }

    #[test]
    fn round_trip_and_tris() {
        let dir = std::env::temp_dir().join(format!("mesh-glb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.glb");
        sample().write(&p).unwrap();
        let g = Glb::read(&p).unwrap();
        assert_eq!(&g.bin[..7], &[1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(tris(&p), 12 + 3);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rewrite_realigns() {
        let mut g = sample();
        g.rewrite_views(|i, _, _| (i == 0).then(|| vec![9; 5])).unwrap();
        assert_eq!(g.json["bufferViews"][1]["byteOffset"], 8);
        assert_eq!(&g.bin[8..12], &[4, 5, 6, 7]);
        assert_eq!(g.json["buffers"][0]["byteLength"], 12);
    }
}
