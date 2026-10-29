//! The review sheet: front, three-quarter, side and back renders in one 2048x512 strip.
use std::io::Cursor;
use std::path::Path;

use anyhow::Result;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::{FilterType, overlay};
use image::{DynamicImage, Rgba, RgbaImage};

pub const VIEWS: [&str; 4] = ["front", "three_quarter", "side", "back"];
const TILE: u32 = 512;
const BG: Rgba<u8> = Rgba([0x06, 0x07, 0x0a, 0xff]);

/// Writes renders/sheet.jpg from renders/{front,three_quarter,side,back}.png.
pub fn review_sheet(renders: &Path) -> Result<()> {
    let mut sheet = RgbaImage::from_pixel(TILE * 4, TILE, BG);
    for (i, v) in VIEWS.iter().enumerate() {
        let Ok(im) = image::open(renders.join(format!("{v}.png"))) else { continue };
        let im = im.resize(TILE, TILE, FilterType::Lanczos3).to_rgba8();
        let x = i as u32 * TILE + (TILE - im.width()) / 2;
        let y = (TILE - im.height()) / 2;
        overlay(&mut sheet, &im, x as i64, y as i64);
    }
    let mut buf = Vec::new();
    DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(sheet).to_rgb8()).write_with_encoder(JpegEncoder::new_with_quality(Cursor::new(&mut buf), 92))?;
    std::fs::write(renders.join("sheet.jpg"), buf)?;
    Ok(())
}
