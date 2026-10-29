//! Concept stage: turn a description plus style references into a clean, single-object
//! picture that image-to-3D engines read well (one subject, whole, plain background).
//!
//! Uses Nano Banana edit on fal (~$0.04 an image) with the reference paintings as style
//! and material guides, so props come out in the art's own look.
use std::path::Path;

use anyhow::Result;
use serde_json::json;

use crate::fal::{Fal, err};
use crate::{Log, download};

const FRAME: &str = "Product reference photo for a 3D artist. Show ONLY {what}, alone, whole and uncropped, \
centred, three-quarter front view from slightly above, filling about 70% of a square frame. \
Plain flat mid-grey studio background, soft even light, no floor clutter, no other objects, \
no people or creatures unless they are part of the object, no text overlays. \
Match the materials, wear, colours and era of the attached artwork: {style}";

pub const DEFAULT_STYLE: &str = "grimy retro-futurist underground lab, scuffed olive and gunmetal steel, brass fittings, \
worn rubber cables, green phosphor glow, dark and gritty but clearly lit";

/// Generate one concept image at `out`. Without references, Nano Banana's text-to-image model is used.
pub fn concept(what: &str, out: &Path, refs: &[&Path], style: Option<&str>, seed: Option<i64>, log: Log) -> Result<()> {
    let fal = Fal::from_keys()?;
    let prompt = FRAME.replace("{what}", what).replace("{style}", style.unwrap_or(DEFAULT_STYLE));
    let mut payload = json!({"prompt": prompt, "aspect_ratio": "1:1", "output_format": "png", "num_images": 1});
    let endpoint = if refs.is_empty() {
        "fal-ai/nano-banana"
    } else {
        payload["image_urls"] = refs.iter().map(|r| fal.upload(r)).collect::<Result<Vec<_>>>()?.into();
        "fal-ai/nano-banana/edit"
    };
    if let Some(s) = seed {
        payload["seed"] = s.into();
    }
    let r = fal.run(endpoint, &payload, log, 4)?;
    let url = r["images"][0]["url"].as_str().ok_or_else(|| err(format!("no image returned: {}", r["description"].as_str().unwrap_or(""))))?;
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d)?;
    }
    download(url, out)
}
