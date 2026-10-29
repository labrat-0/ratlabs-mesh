//! Shrink a GLB's embedded textures for the web: cap the size, turn opaque PNGs into
//! JPEGs, re-encode JPEGs. Geometry is untouched.
use std::io::Cursor;
use std::path::Path;

use anyhow::Result;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::imageops::FilterType as Resize;
use image::{DynamicImage, GenericImageView, ImageEncoder};

use crate::Glb;

fn recode(data: &[u8], mime: &str, max_size: u32, quality: u8) -> (Vec<u8>, String) {
    let Ok(mut im) = image::load_from_memory(data) else { return (data.to_vec(), mime.into()) };
    let (w, h) = im.dimensions();
    if w.max(h) > max_size {
        let s = max_size as f64 / w.max(h) as f64;
        im = im.resize_exact(((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1), Resize::Lanczos3);
    }
    let has_alpha = im.color().has_alpha() && im.to_rgba8().pixels().any(|p| p[3] < 250);
    let mut buf = Vec::new();
    let new_mime = if has_alpha {
        let rgba = im.to_rgba8();
        let enc = PngEncoder::new_with_quality(Cursor::new(&mut buf), CompressionType::Best, FilterType::Adaptive);
        if enc.write_image(&rgba, rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8).is_err() {
            return (data.to_vec(), mime.into());
        }
        "image/png"
    } else {
        let rgb = DynamicImage::ImageRgb8(im.to_rgb8());
        if rgb.write_with_encoder(JpegEncoder::new_with_quality(Cursor::new(&mut buf), quality)).is_err() {
            return (data.to_vec(), mime.into());
        }
        "image/jpeg"
    };
    if buf.len() < data.len() { (buf, new_mime.into()) } else { (data.to_vec(), mime.into()) }
}

/// Returns (bytes before, bytes after) of the binary chunk.
pub fn shrink(path: &Path, max_size: u32, quality: u8) -> Result<(usize, usize)> {
    let mut g = Glb::read(path)?;
    let before = g.bin.len();
    let views = g.image_views();
    g.rewrite_views(|vi, data, json| {
        let &ii = views.get(&vi)?;
        let img = &mut json["images"][ii];
        let mime = img["mimeType"].as_str().unwrap_or("image/png").to_string();
        let (new, new_mime) = recode(data, &mime, max_size, quality);
        img["mimeType"] = new_mime.into();
        Some(new)
    })?;
    let after = g.bin.len();
    g.write(path)?;
    Ok((before, after))
}
