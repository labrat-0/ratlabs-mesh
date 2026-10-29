//! Background removal with BiRefNet-general through ONNX Runtime, matching rembg's
//! preprocessing so cut-outs come out the same as the Python version's.
//!
//! The model (~970 MB) is reused from rembg's cache when present, otherwise downloaded
//! once into the data folder. A loaded session holds several GB, so it is dropped again
//! after ten idle minutes.
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use image::imageops::FilterType;
use image::{GrayImage, Rgba, RgbaImage};
use ort::session::Session;
use ort::value::Tensor;

const MODEL_URL: &str = "https://github.com/danielgatis/rembg/releases/download/v0.0.0/BiRefNet-general-epoch_244.onnx";
const MODEL_BYTES_MIN: u64 = 900_000_000;
const IDLE_UNLOAD: Duration = Duration::from_secs(600);
const INPUT: u32 = 1024;
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

struct Loaded {
    session: Session,
    last_used: Instant,
}

static SESSION: Mutex<Option<Loaded>> = Mutex::new(None);
static ENV: std::sync::OnceLock<ort::environment::Environment> = std::sync::OnceLock::new();

/// Where the model is, or will be once downloaded.
pub fn model_path() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let candidates = [home.join(".rembg/models/birefnet-general/birefnet-general.onnx"), home.join(".u2net/birefnet-general.onnx")];
    candidates.into_iter().find(|p| p.exists()).unwrap_or_else(|| mesh_core::config().data_dir.join("models/birefnet-general.onnx"))
}

pub fn model_ready() -> bool {
    std::fs::metadata(model_path()).map(|m| m.len() >= MODEL_BYTES_MIN).unwrap_or(false)
}

/// Download the model if it is missing. `log` gets progress lines.
pub fn ensure_model(log: &dyn Fn(&str)) -> Result<PathBuf> {
    let path = model_path();
    if model_ready() {
        return Ok(path);
    }
    log("downloading the BiRefNet cut-out model (970 MB, once)");
    std::fs::create_dir_all(path.parent().unwrap())?;
    let part = path.with_extension("onnx.part");
    let mut resp = reqwest::blocking::Client::builder().timeout(None).build()?.get(MODEL_URL).send()?.error_for_status()?;
    let mut f = std::fs::File::create(&part)?;
    std::io::copy(&mut resp, &mut f)?;
    drop(f);
    if std::fs::metadata(&part)?.len() < MODEL_BYTES_MIN {
        std::fs::remove_file(&part).ok();
        return Err(anyhow!("BiRefNet download was incomplete; try again"));
    }
    std::fs::rename(&part, &path)?;
    Ok(path)
}

fn reaper() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        std::thread::spawn(|| {
            loop {
                std::thread::sleep(Duration::from_secs(60));
                let mut s = SESSION.lock().unwrap();
                if s.as_ref().is_some_and(|l| l.last_used.elapsed() > IDLE_UNLOAD) {
                    *s = None;
                }
            }
        });
    });
}

/// RGBA cut-out: colour multiplied by the mask, alpha = mask (rembg's default "naive" cut-out).
pub fn cutout(im: &RgbaImage) -> Result<RgbaImage> {
    let mask = predict(im)?;
    let mut out = RgbaImage::new(im.width(), im.height());
    for (x, y, p) in im.enumerate_pixels() {
        let m = mask.get_pixel(x, y)[0] as u32;
        let f = |v: u8| ((v as u32 * m + 127) / 255) as u8;
        out.put_pixel(x, y, Rgba([f(p[0]), f(p[1]), f(p[2]), m as u8]));
    }
    Ok(out)
}

/// The foreground mask at the image's own size.
pub fn predict(im: &RgbaImage) -> Result<GrayImage> {
    let rgb = image::DynamicImage::ImageRgba8(im.clone()).to_rgb8();
    let small = image::imageops::resize(&rgb, INPUT, INPUT, FilterType::Lanczos3);
    let max = small.as_raw().iter().copied().max().unwrap_or(1).max(1) as f32;
    let n = (INPUT * INPUT) as usize;
    let mut data = vec![0f32; 3 * n];
    for (i, p) in small.pixels().enumerate() {
        for c in 0..3 {
            data[c * n + i] = (p[c] as f32 / max - MEAN[c]) / STD[c];
        }
    }
    let input = Tensor::from_array(([1usize, 3, INPUT as usize, INPUT as usize], data))?;

    let pred: Vec<f32> = {
        let mut guard = SESSION.lock().unwrap();
        if guard.is_none() {
            let path = ensure_model(&|m| eprintln!("{m}"))?;
            let env = match ENV.get() {
                Some(e) => e,
                None => {
                    let e = ort::init().with_name("ratlabs-mesh").build()?;
                    ENV.get_or_init(|| e)
                }
            };
            let session = Session::builder(env)?.commit_from_file(&path).context("loading BiRefNet")?;
            *guard = Some(Loaded { session, last_used: Instant::now() });
            reaper();
        }
        let loaded = guard.as_mut().unwrap();
        let name = loaded.session.inputs()[0].name().to_string();
        let outputs = loaded.session.run(ort::inputs![name => input])?;
        let (_, raw) = outputs[0].try_extract_tensor::<f32>()?;
        let v = raw[..n].to_vec();
        loaded.last_used = Instant::now();
        v
    };

    let sig: Vec<f32> = pred.iter().map(|&x| 1.0 / (1.0 + (-x).exp())).collect();
    let (lo, hi) = sig.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let span = (hi - lo).max(1e-6);
    let mask = GrayImage::from_fn(INPUT, INPUT, |x, y| {
        let v = (sig[(y * INPUT + x) as usize] - lo) / span;
        image::Luma([(v * 255.0) as u8])
    });
    Ok(image::imageops::resize(&mask, im.width(), im.height(), FilterType::Lanczos3))
}
