//! Stage 1: turn any picture into a clean, centred subject cut-out.
//!
//! Image-to-3D models are only as good as their input. Meshy quietly does this step for
//! you; here it is explicit so it can be checked before paying for a GPU.
use std::collections::VecDeque;
use std::io::BufReader;
use std::path::Path;

use anyhow::{Result, bail};
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageReader, Rgba, RgbaImage};
use serde::Serialize;

#[cfg(feature = "cutout")]
pub mod cutout;

/// What TRELLIS.2 / Hunyuan / Meshy expect.
pub const SIZE: u32 = 1024;
/// Detail source for the local painter.
pub const HI_SIZE: u32 = 2048;
/// Empty border around the subject, as a fraction of the square.
const MARGIN: f64 = 0.08;

#[derive(Debug, Clone, Serialize)]
pub struct PrepInfo {
    pub subject_px: [u32; 2],
    pub aspect: f64,
    pub coverage: f64,
    pub edge_fuzz: f64,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PrepOptions {
    /// (x0, y0, x1, y1) as 0..1 fractions of the source, to pick one character out of a scene.
    pub crop: Option<[f64; 4]>,
    /// Skip background removal (the art is already isolated).
    pub keep_background: bool,
}

/// Open an image the right way up (EXIF orientation applied).
pub fn open(src: &Path) -> Result<DynamicImage> {
    let mut dec = ImageReader::new(BufReader::new(std::fs::File::open(src)?)).with_guessed_format()?.into_decoder()?;
    let orient = dec.orientation()?;
    let mut im = DynamicImage::from_decoder(dec)?;
    im.apply_orientation(orient);
    Ok(im)
}

fn round(x: f64, places: i32) -> f64 {
    let m = 10f64.powi(places);
    (x * m).round() / m
}

/// Scale the subject into a transparent square with an even margin.
/// Pasted with its own alpha as the mask, so edges match what Pillow produced.
fn square(subject: &RgbaImage, size: u32) -> RgbaImage {
    let inner = (size as f64 * (1.0 - 2.0 * MARGIN)) as u32;
    let scale = inner as f64 / subject.width().max(subject.height()) as f64;
    let w = ((subject.width() as f64 * scale).round() as u32).max(1);
    let h = ((subject.height() as f64 * scale).round() as u32).max(1);
    let s = image::imageops::resize(subject, w, h, FilterType::Lanczos3);
    let mut canvas = RgbaImage::new(size, size);
    let (ox, oy) = ((size - w) / 2, (size - h) / 2);
    for (x, y, p) in s.enumerate_pixels() {
        let a = p[3] as u32;
        let m = |v: u8| ((v as u32 * a + 127) / 255) as u8;
        canvas.put_pixel(ox + x, oy + y, Rgba([m(p[0]), m(p[1]), m(p[2]), m(p[3])]));
    }
    canvas
}

/// Label 4-connected regions of `solid`; returns (labels, sizes) with label 0 = background.
fn components(solid: &[bool], w: usize, h: usize) -> (Vec<u32>, Vec<usize>) {
    let mut labels = vec![0u32; solid.len()];
    let mut sizes = vec![0usize];
    let mut q = VecDeque::new();
    for start in 0..solid.len() {
        if !solid[start] || labels[start] != 0 {
            continue;
        }
        let l = sizes.len() as u32;
        sizes.push(0);
        labels[start] = l;
        q.push_back(start);
        while let Some(i) = q.pop_front() {
            sizes[l as usize] += 1;
            let (x, y) = (i % w, i / w);
            let mut visit = |j: usize| {
                if solid[j] && labels[j] == 0 {
                    labels[j] = l;
                    q.push_back(j);
                }
            };
            if x > 0 {
                visit(i - 1)
            }
            if x + 1 < w {
                visit(i + 1)
            }
            if y > 0 {
                visit(i - w)
            }
            if y + 1 < h {
                visit(i + w)
            }
        }
    }
    (labels, sizes)
}

/// Pixels within `r` 4-connected steps of `seed` (scipy's binary_dilation, `r` iterations).
fn dilate(seed: &[bool], w: usize, h: usize, r: u32) -> Vec<bool> {
    let mut dist = vec![u32::MAX; seed.len()];
    let mut q = VecDeque::new();
    for (i, &s) in seed.iter().enumerate() {
        if s {
            dist[i] = 0;
            q.push_back(i);
        }
    }
    while let Some(i) = q.pop_front() {
        let d = dist[i];
        if d == r {
            continue;
        }
        let (x, y) = (i % w, i / w);
        for j in [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (y + 1 < h).then(|| i + w)].into_iter().flatten() {
            if dist[j] == u32::MAX {
                dist[j] = d + 1;
                q.push_back(j);
            }
        }
    }
    dist.into_iter().map(|d| d <= r).collect()
}

/// Write a square RGBA cut-out to `out` (and a full-resolution copy to `hi_out`) and
/// return quality notes for the user.
pub fn prepare(src: &Path, out: &Path, opt: PrepOptions, hi_out: Option<&Path>) -> Result<PrepInfo> {
    let mut im = open(src)?.to_rgba8();
    if let Some([x0, y0, x1, y1]) = opt.crop {
        let (w, h) = (im.width() as f64, im.height() as f64);
        let (cx, cy) = ((x0 * w) as u32, (y0 * h) as u32);
        let (cw, ch) = (((x1 * w) as u32).saturating_sub(cx).max(1), ((y1 * h) as u32).saturating_sub(cy).max(1));
        im = image::imageops::crop_imm(&im, cx, cy, cw, ch).to_image();
    }
    let rgba = if opt.keep_background { im } else { remove_background(&im)? };

    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let mut a: Vec<f32> = rgba.pixels().map(|p| p[3] as f32 / 255.0).collect();
    let mut solid: Vec<bool> = a.iter().map(|&v| v > 0.5).collect();
    // Keep the main subject plus any sizeable attached pieces; drop specks.
    let (labels, sizes) = components(&solid, w, h);
    if sizes.len() > 1 {
        let biggest = *sizes[1..].iter().max().unwrap() as f64;
        let min = (biggest * 0.02).max(64.0);
        let keep: Vec<bool> = labels.iter().map(|&l| l > 0 && sizes[l as usize] as f64 >= min).collect();
        let near = dilate(&keep, w, h, 6);
        for i in 0..a.len() {
            if !near[i] {
                a[i] = 0.0;
            }
        }
        solid = a.iter().map(|&v| v > 0.5).collect();
    }

    let n_solid = solid.iter().filter(|&&s| s).count();
    let coverage = n_solid as f64 / a.len() as f64;
    let fuzzy = a.iter().filter(|&&v| v > 0.08 && v < 0.92).count() as f64 / n_solid.max(1) as f64;
    if coverage < 0.002 {
        bail!("No subject found. Draw a crop box around the character.");
    }
    let mut notes = vec![];
    if fuzzy > 0.6 {
        notes.push("Mask is mostly semi-transparent: this looks like a scene, not one subject. Draw a crop box around the character.".into());
    }

    let mut arr = rgba;
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (i, p) in arr.pixels_mut().enumerate() {
        p[3] = (a[i] * 255.0) as u8;
        if p[3] > 8 {
            let (x, y) = ((i % w) as u32, (i / w) as u32);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + 1);
            y1 = y1.max(y + 1);
        }
    }
    let subject = image::imageops::crop_imm(&arr, x0, y0, x1 - x0, y1 - y0).to_image();
    let (sw, sh) = subject.dimensions();
    if sw.max(sh) < 512 {
        notes.push(format!("Subject is only {}px tall/wide; a larger source gives sharper textures.", sw.max(sh)));
    }
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    square(&subject, SIZE).save(out)?;
    if let Some(hi) = hi_out {
        // Full-resolution copy for the detail pass: never smaller than the engine input.
        let size = SIZE.max(HI_SIZE.min((sw.max(sh) as f64 / (1.0 - 2.0 * MARGIN)) as u32));
        square(&subject, size).save(hi)?;
    }
    Ok(PrepInfo { subject_px: [sw, sh], aspect: round(sh as f64 / sw as f64, 3), coverage: round(coverage, 4), edge_fuzz: round(fuzzy, 3), notes })
}

#[cfg(feature = "cutout")]
fn remove_background(im: &RgbaImage) -> Result<RgbaImage> {
    cutout::cutout(im)
}

#[cfg(not(feature = "cutout"))]
fn remove_background(_: &RgbaImage) -> Result<RgbaImage> {
    bail!("this build has no background removal; tick \"art is already isolated\" or rebuild with the `cutout` feature")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_specks_keeps_subject() {
        let dir = std::env::temp_dir().join(format!("mesh-prep-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 200x200 transparent canvas: a 60x120 opaque body and a 3x3 speck far away.
        let mut im = RgbaImage::new(200, 200);
        for y in 40..160 {
            for x in 70..130 {
                im.put_pixel(x, y, Rgba([200, 50, 50, 255]));
            }
        }
        for y in 5..8 {
            for x in 5..8 {
                im.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
        let src = dir.join("in.png");
        im.save(&src).unwrap();
        let out = dir.join("out.png");
        let info = prepare(&src, &out, PrepOptions { keep_background: true, ..Default::default() }, None).unwrap();
        assert_eq!(info.subject_px, [60, 120]);
        assert_eq!(info.aspect, 2.0);
        assert!(info.notes.iter().any(|n| n.contains("only 120px")));
        let o = image::open(&out).unwrap().to_rgba8();
        assert_eq!(o.dimensions(), (SIZE, SIZE));
        assert_eq!(o.get_pixel(512, 512)[3], 255);
        assert_eq!(o.get_pixel(10, 10)[3], 0);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn empty_is_an_error() {
        let dir = std::env::temp_dir().join(format!("mesh-prep-e-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("in.png");
        RgbaImage::new(50, 50).save(&src).unwrap();
        let err = prepare(&src, &dir.join("o.png"), PrepOptions { keep_background: true, ..Default::default() }, None).unwrap_err();
        assert!(err.to_string().contains("No subject"));
        std::fs::remove_dir_all(dir).ok();
    }
}
