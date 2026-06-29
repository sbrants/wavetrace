//! Full-frame OCR: Windows.Media.Ocr on Windows, Tesseract elsewhere.

#[cfg(windows)]
use std::ptr;
#[cfg(windows)]
use std::slice;
use std::sync::{Mutex, OnceLock};

use image::{imageops, RgbaImage};

#[cfg(windows)]
use windows::{
    core::Interface,
    Graphics::Imaging::{BitmapBufferAccessMode, BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::{OcrEngine, OcrResult},
    Win32::System::WinRT::{IMemoryBufferByteAccess, RoInitialize, RO_INIT_MULTITHREADED},
};

#[cfg(windows)]
use pollster::block_on;

#[cfg(windows)]
static WINRT_INIT: OnceLock<Result<(), String>> = OnceLock::new();

#[cfg(windows)]
static OCR_ENGINE: OnceLock<Result<OcrEngine, String>> = OnceLock::new();

#[cfg(windows)]
static OCR_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(not(windows))]
static TESSDATA_INIT: OnceLock<()> = OnceLock::new();

#[cfg(not(windows))]
fn ensure_tesseract_paths() {
    TESSDATA_INIT.get_or_init(|| {
        if std::env::var_os("TESSDATA_PREFIX").is_some() {
            return;
        }
        #[cfg(target_os = "macos")]
        if let Ok(exe) = std::env::current_exe() {
            if let Some(resources) = exe
                .parent()
                .and_then(|macos| macos.parent())
                .map(|contents| contents.join("Resources"))
            {
                let tessdata = resources.join("tessdata");
                if tessdata.is_dir() {
                    std::env::set_var("TESSDATA_PREFIX", tessdata);
                }
            }
        }
    });
}

/// OCR the entire capture and return every non-empty text line discovered.
#[cfg(windows)]
pub fn ocr_full_frame(img: &RgbaImage) -> Result<Vec<String>, String> {
    let dynamic = prepare_image(img);
    let rgba = dynamic.to_rgba8();
    let result = recognize_rgba8(&rgba)?;
    lines_from_result(&result)
}

#[cfg(not(windows))]
pub fn ocr_full_frame(img: &RgbaImage) -> Result<Vec<String>, String> {
    ensure_tesseract_paths();
    let mut all_lines = Vec::new();
    let mut any_ok = false;
    for &region in OCR_REGIONS {
        match ocr_region(img, region) {
            Ok(lines) => {
                if !lines.is_empty() {
                    any_ok = true;
                    all_lines.extend(lines);
                }
            }
            Err(e) => eprintln!("OCR region {} failed: {e}", region.name),
        }
    }
    if !any_ok || all_lines.is_empty() {
        return Err("Tesseract OCR returned no text".into());
    }
    Ok(all_lines)
}

/// Normalized capture sub-rectangle for a targeted Tesseract pass.
#[cfg(not(windows))]
#[derive(Debug, Clone, Copy)]
struct OcrRegion {
    name: &'static str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    upscale: u32,
}

/// Portrait phone-mirror layout (~1:2.2). Fractions validated on 400×851 captures.
#[cfg(not(windows))]
const OCR_REGIONS: &[OcrRegion] = &[
    OcrRegion {
        name: "coin",
        x: 0.0,
        y: 0.0,
        w: 0.5,
        h: 0.2,
        upscale: 3,
    },
    OcrRegion {
        name: "wave_skip",
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 0.5,
        upscale: 3,
    },
    OcrRegion {
        name: "tier_wave",
        x: 0.5,
        y: 0.5,
        w: 0.5,
        h: 0.5,
        upscale: 3,
    },
];

#[cfg(not(windows))]
impl OcrRegion {
    fn to_pixels(self, frame_w: u32, frame_h: u32) -> (u32, u32, u32, u32) {
        let x = (self.x * frame_w as f32).round() as u32;
        let y = (self.y * frame_h as f32).round() as u32;
        let w = (self.w * frame_w as f32)
            .round()
            .min(frame_w.saturating_sub(x) as f32) as u32;
        let h = (self.h * frame_h as f32)
            .round()
            .min(frame_h.saturating_sub(y) as f32) as u32;
        (x, y, w.max(1), h.max(1))
    }
}

#[cfg(not(windows))]
fn crop_region(img: &RgbaImage, region: OcrRegion) -> RgbaImage {
    let (x, y, w, h) = region.to_pixels(img.width(), img.height());
    imageops::crop_imm(img, x, y, w, h).to_image()
}

#[cfg(not(windows))]
fn ocr_region(img: &RgbaImage, region: OcrRegion) -> Result<Vec<String>, String> {
    let crop = crop_region(img, region);
    let gray = prepare_region_for_tesseract(&crop, region.upscale);
    let width = gray.width() as i32;
    let height = gray.height() as i32;
    let bytes_per_line = width;
    let text = run_tesseract(
        gray.as_raw(),
        width,
        height,
        1,
        bytes_per_line,
        tesseract::PageSegMode::PsmSingleBlock,
    )?;
    Ok(split_lines(&text))
}

/// Grayscale, upscale, and contrast-stretch a cropped HUD region before OCR.
#[cfg(not(windows))]
fn prepare_region_for_tesseract(img: &RgbaImage, upscale: u32) -> image::GrayImage {
    let mut gray = imageops::grayscale(img);
    if upscale > 1 {
        let new_w = gray.width().saturating_mul(upscale);
        let new_h = gray.height().saturating_mul(upscale);
        gray = imageops::resize(&gray, new_w, new_h, imageops::FilterType::Lanczos3);
    }
    imageops::colorops::contrast(&gray, 30.0)
}

/// Run a single Tesseract pass over a raw grayscale frame. Uses the builder API
/// (rather than `tesseract::ocr_from_frame`) so we can hint a source resolution,
/// which keeps the LSTM engine from misjudging glyph scale on upscaled frames.
#[cfg(not(windows))]
fn run_tesseract(
    frame: &[u8],
    width: i32,
    height: i32,
    bytes_per_pixel: i32,
    bytes_per_line: i32,
    psm: tesseract::PageSegMode,
) -> Result<String, String> {
    let mut engine = tesseract::Tesseract::new(None, Some("eng"))
        .map_err(|e| format!("Tesseract init failed: {e}"))?
        .set_frame(frame, width, height, bytes_per_pixel, bytes_per_line)
        .map_err(|e| format!("Tesseract set_frame failed: {e}"))?
        .set_source_resolution(192);
    engine.set_page_seg_mode(psm);
    let mut engine = engine
        .recognize()
        .map_err(|e| format!("Tesseract recognize failed: {e}"))?;
    engine
        .get_text()
        .map_err(|e| format!("Tesseract get_text failed: {e}"))
}

#[cfg(windows)]
fn init_winrt() -> Result<(), String> {
    WINRT_INIT
        .get_or_init(|| unsafe {
            RoInitialize(RO_INIT_MULTITHREADED).map_err(|e| format!("RoInitialize failed: {e}"))
        })
        .clone()
}

#[cfg(windows)]
fn ocr_engine() -> Result<&'static OcrEngine, String> {
    OCR_ENGINE
        .get_or_init(|| {
            init_winrt()?;
            OcrEngine::TryCreateFromUserProfileLanguages()
                .map_err(|e| format!("Windows OCR engine unavailable: {e}"))
        })
        .as_ref()
        .map_err(|e| e.clone())
}

#[cfg(windows)]
fn recognize_rgba8(img: &RgbaImage) -> Result<OcrResult, String> {
    let _guard = OCR_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|e| format!("OCR mutex poisoned: {e}"))?;
    init_winrt()?;
    let engine = ocr_engine()?;
    let bitmap = rgba_to_software_bitmap(img)?;
    block_on(async {
        engine
            .RecognizeAsync(&bitmap)
            .map_err(|e| format!("Windows OCR RecognizeAsync failed: {e}"))?
            .await
            .map_err(|e| format!("Windows OCR recognition failed: {e}"))
    })
}

#[cfg(windows)]
fn lines_from_result(result: &OcrResult) -> Result<Vec<String>, String> {
    let lines = result
        .Lines()
        .map_err(|e| format!("Windows OCR Lines() failed: {e}"))?;
    let count = lines
        .Size()
        .map_err(|e| format!("Windows OCR line count failed: {e}"))?;
    let mut out = Vec::new();
    for i in 0..count {
        let line = lines
            .GetAt(i)
            .map_err(|e| format!("Windows OCR line {i} failed: {e}"))?;
        let text = line
            .Text()
            .map_err(|e| format!("Windows OCR line {i} text failed: {e}"))?
            .to_string();
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
    if out.is_empty() {
        if let Ok(text) = result.Text() {
            out = split_lines(&text.to_string());
        }
    }
    Ok(out)
}

#[cfg(windows)]
fn rgba_to_software_bitmap(img: &RgbaImage) -> Result<SoftwareBitmap, String> {
    let width = img.width() as i32;
    let height = img.height() as i32;
    let expected_len = (width as u64)
        .checked_mul(height as u64)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| "Image dimensions overflow".to_string())?;

    let bitmap = SoftwareBitmap::Create(BitmapPixelFormat::Bgra8, width, height)
        .map_err(|e| format!("SoftwareBitmap::Create failed: {e}"))?;

    {
        let bmp_buf = bitmap
            .LockBuffer(BitmapBufferAccessMode::Write)
            .map_err(|e| format!("LockBuffer failed: {e}"))?;
        let array: IMemoryBufferByteAccess = bmp_buf
            .CreateReference()
            .map_err(|e| format!("CreateReference failed: {e}"))?
            .cast()
            .map_err(|e| format!("IMemoryBufferByteAccess cast failed: {e}"))?;

        let mut data = ptr::null_mut();
        let mut capacity = 0u32;
        unsafe {
            array
                .GetBuffer(&mut data, &mut capacity)
                .map_err(|e| format!("GetBuffer failed: {e}"))?;
        }

        if capacity as u64 != expected_len {
            return Err(format!(
                "SoftwareBitmap buffer size mismatch: expected {expected_len}, got {capacity}"
            ));
        }

        let src = img.as_raw();
        let dst = unsafe { slice::from_raw_parts_mut(data, capacity as usize) };
        for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
    }

    Ok(bitmap)
}

/// Downscale large emulator frames so OCR stays responsive.
#[cfg(windows)]
fn prepare_image(img: &RgbaImage) -> image::DynamicImage {
    const MAX_WIDTH: u32 = 900;
    if img.width() <= MAX_WIDTH {
        return image::DynamicImage::ImageRgba8(img.clone());
    }
    let scale = MAX_WIDTH as f32 / img.width() as f32;
    let new_h = ((img.height() as f32) * scale).round().max(1.0) as u32;
    let resized = imageops::resize(img, MAX_WIDTH, new_h, imageops::FilterType::Triangle);
    image::DynamicImage::ImageRgba8(resized)
}

fn split_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_lines_trims_and_drops_blanks() {
        assert_eq!(
            split_lines("  Tier 14\n\nWave 2000\n"),
            vec!["Tier 14".to_string(), "Wave 2000".to_string()]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn region_pixels_match_portrait_layout() {
        let coin = OCR_REGIONS[0];
        assert_eq!(coin.to_pixels(400, 851), (0, 0, 200, 170));
        let tier_wave = OCR_REGIONS[2];
        assert_eq!(tier_wave.to_pixels(400, 851), (200, 426, 200, 425));
    }
}
