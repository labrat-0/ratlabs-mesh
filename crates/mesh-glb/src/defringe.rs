//! Remove cut-out fringe colours (pure red / yellow halos from badly matted PNG art)
//! that image-to-3D engines paint into hair and edges. Edits the GLB's base-colour
//! texture in place: matching pixels are replaced by a blur of their neutral neighbours.
use std::collections::HashSet;
use std::io::Cursor;
use std::path::Path;

use anyhow::Result;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, GrayImage, ImageFormat, Luma, Rgb, Rgb32FImage, RgbImage};
use imageproc::distance_transform::Norm;
use imageproc::filter::gaussian_blur_f32;
use imageproc::morphology::dilate;

use crate::Glb;

fn fringe(p: &Rgb<u8>) -> bool {
    let (r, g, b) = (p[0] as f32, p[1] as f32, p[2] as f32);
    // saturated reds only; skin and ear pinks keep g and b above half of r
    let red = r > 95.0 && g < 0.5 * r && b < 0.5 * r && r - g.max(b) > 55.0;
    let yellow = r > 160.0 && g > 140.0 && b < 0.45 * g && r.min(g) - b > 100.0;
    red || yellow
}

/// Returns (re-encoded image, pixels replaced), or None when nothing matched.
fn clean(data: &[u8], jpeg: bool) -> Option<(Vec<u8>, usize)> {
    let a: RgbImage = image::load_from_memory(data).ok()?.to_rgb8();
    let (w, h) = a.dimensions();
    let mask = GrayImage::from_fn(w, h, |x, y| Luma([if fringe(a.get_pixel(x, y)) { 255 } else { 0 }]));
    let changed = mask.pixels().filter(|p| p[0] > 0).count();
    if changed == 0 {
        return None;
    }
    let grow = dilate(&mask, Norm::LInf, 2); // a 5x5 max filter
    // neutral fill: the image with fringe pixels replaced by their local grey level, blurred
    let grey_filled = Rgb32FImage::from_fn(w, h, |x, y| {
        let p = a.get_pixel(x, y);
        if grow.get_pixel(x, y)[0] > 0 {
            let m = (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0;
            Rgb([m, m, m])
        } else {
            Rgb([p[0] as f32, p[1] as f32, p[2] as f32])
        }
    });
    let fill = gaussian_blur_f32(&grey_filled, 3.0);
    let out = RgbImage::from_fn(w, h, |x, y| {
        if grow.get_pixel(x, y)[0] > 0 {
            let f = fill.get_pixel(x, y);
            Rgb([f[0].round().clamp(0.0, 255.0) as u8, f[1].round().clamp(0.0, 255.0) as u8, f[2].round().clamp(0.0, 255.0) as u8])
        } else {
            *a.get_pixel(x, y)
        }
    });
    let mut buf = Vec::new();
    let img = DynamicImage::ImageRgb8(out);
    if jpeg {
        img.write_with_encoder(JpegEncoder::new_with_quality(Cursor::new(&mut buf), 92)).ok()?;
    } else {
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png).ok()?;
    }
    Some((buf, changed))
}

/// Clean every base-colour texture in place. Returns the number of fringe pixels replaced.
pub fn defringe(path: &Path) -> Result<usize> {
    let mut g = Glb::read(path)?;
    let mut base = HashSet::new();
    for m in g.json["materials"].as_array().into_iter().flatten() {
        if let Some(t) = m["pbrMetallicRoughness"]["baseColorTexture"]["index"].as_u64()
            && let Some(src) = g.json["textures"][t as usize]["source"].as_u64()
        {
            base.insert(src as usize);
        }
    }
    let views = g.image_views();
    let mut changed = 0;
    g.rewrite_views(|vi, data, json| {
        let &ii = views.get(&vi)?;
        if !base.contains(&ii) {
            return None;
        }
        let jpeg = json["images"][ii]["mimeType"].as_str().unwrap_or("").contains("jpeg");
        let (new, n) = clean(data, jpeg)?;
        changed += n;
        Some(new)
    })?;
    g.write(path)?;
    Ok(changed)
}
