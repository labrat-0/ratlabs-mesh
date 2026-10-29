//! Pack a grabbed Market material into a game-ready folder:
//!
//!   color.jpg   base colour
//!   normal.jpg  OpenGL normal (+Y), as three.js expects
//!   arm.jpg     R = ambient occlusion, G = roughness, B = metalness (glTF convention)
//!   alpha.jpg   only when the source has an opacity map (grates, catwalks)
//!   preview.png the source's thumbnail
//!
//! Height maps and duplicate single-channel maps are dropped: three.js reads ao, roughness
//! and metalness from one ARM texture, and displacement needs tessellation games rarely use.
use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage};
use serde_json::{Value, json};

const ROLES: &[(&str, &[&str])] = &[
    ("color", &["_diff_", "_Color"]),
    ("normal", &["_nor_gl_", "_NormalGL"]),
    ("roughness", &["_rough_", "_Roughness"]),
    ("ao", &["_ao_", "_AmbientOcclusion"]),
    ("metalness", &["_metal_", "_Metalness"]),
    ("arm", &["_arm_"]),
    ("alpha", &["_Opacity"]),
];

fn jpeg(img: DynamicImage, quality: u8, out: &Path) -> Result<()> {
    let mut buf = Vec::new();
    img.write_with_encoder(JpegEncoder::new_with_quality(Cursor::new(&mut buf), quality))?;
    std::fs::write(out, buf)?;
    Ok(())
}

pub fn pack(source: &str, id: &str, out: &Path) -> Result<Value> {
    let src = crate::lib_dir().join(source).join(id);
    let meta: Value = serde_json::from_slice(&std::fs::read(src.join("meta.json"))?)?;
    std::fs::create_dir_all(out)?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(&src)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("png")))
        .collect();
    files.sort();
    let mut found: BTreeMap<&str, PathBuf> = BTreeMap::new();
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy();
        for (role, pats) in ROLES {
            if pats.iter().any(|p| name.contains(p)) && !found.contains_key(role) {
                found.insert(role, f.clone());
            }
        }
    }
    let Some(color_path) = found.get("color") else { bail!("{source}/{id}: no colour map") };
    let color = image::open(color_path)?;
    let (w, h) = (color.width(), color.height());
    jpeg(DynamicImage::ImageRgb8(color.to_rgb8()), 82, &out.join("color.jpg"))?;
    let mut maps = json!({"color": "color.jpg"});
    if let Some(n) = found.get("normal") {
        jpeg(DynamicImage::ImageRgb8(image::open(n)?.to_rgb8()), 90, &out.join("normal.jpg"))?;
        maps["normal"] = "normal.jpg".into();
    }
    let arm: RgbImage = if let Some(a) = found.get("arm") {
        image::open(a)?.to_rgb8()
    } else {
        let ch = |role: &str, default: u8| -> Result<GrayImage> {
            Ok(match found.get(role) {
                Some(p) => image::imageops::resize(&image::open(p)?.to_luma8(), w, h, FilterType::Triangle),
                None => GrayImage::from_pixel(w, h, Luma([default])),
            })
        };
        let (ao, rough, metal) = (ch("ao", 255)?, ch("roughness", 204)?, ch("metalness", 0)?);
        RgbImage::from_fn(w, h, |x, y| Rgb([ao.get_pixel(x, y)[0], rough.get_pixel(x, y)[0], metal.get_pixel(x, y)[0]]))
    };
    jpeg(DynamicImage::ImageRgb8(arm), 85, &out.join("arm.jpg"))?;
    maps["arm"] = "arm.jpg".into();
    if let Some(a) = found.get("alpha") {
        jpeg(DynamicImage::ImageLuma8(image::open(a)?.to_luma8()), 90, &out.join("alpha.jpg"))?;
        maps["alpha"] = "alpha.jpg".into();
    }
    if let Some(thumb) = meta["thumb"].as_str().filter(|t| !t.is_empty())
        && let Ok(r) = crate::client(30).get(thumb).send()
        && let Ok(b) = r.bytes()
    {
        std::fs::write(out.join("preview.png"), b).ok();
    }
    Ok(json!({"id": id, "name": meta["name"], "maps": maps, "licence": meta["licence"], "author": meta["author"], "source": meta["page"]}))
}
