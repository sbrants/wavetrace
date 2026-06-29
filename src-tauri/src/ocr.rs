//! Full-frame OCR: Windows.Media.Ocr on Windows, Tesseract elsewhere.

#[cfg(windows)]
use std::ptr;
#[cfg(windows)]
use std::slice;
use std::sync::{Mutex, OnceLock};

use image::RgbaImage;
#[cfg(windows)]
use image::imageops;

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
    let gray = prepare_image_for_tesseract(img);
    let width = gray.width() as i32;
    let height = gray.height() as i32;
    // Grayscale: 1 byte per pixel, so the row stride equals the width.
    let bytes_per_line = width;
    let text = run_tesseract(gray.as_raw(), width, height, 1, bytes_per_line)?;
    let lines = split_lines(&text);
    if lines.is_empty() {
        return Err("Tesseract OCR returned no text".into());
    }
    Ok(lines)
}

/// Tesseract is far less forgiving than Windows OCR on the game's small,
/// stylized HUD digits. Boost its odds by feeding a grayscale, upscaled,
/// contrast-stretched frame instead of the raw RGBA capture.
#[cfg(not(windows))]
fn prepare_image_for_tesseract(img: &RgbaImage) -> image::GrayImage {
    use image::imageops;

    // Drop color: the LSTM model works on luminance and color noise only hurts.
    let mut gray = imageops::grayscale(img);

    // The HUD wave/coin glyphs are tiny in phone-mirroring captures (~880px
    // wide). Upscale small frames so the recognizer sees larger characters.
    // Cap the factor so latency with the (slower) `best` model stays bounded.
    const MIN_WIDTH: u32 = 1280;
    const MAX_SCALE: f32 = 1.6;
    if gray.width() > 0 && gray.width() < MIN_WIDTH {
        let scale = (MIN_WIDTH as f32 / gray.width() as f32).min(MAX_SCALE);
        let new_w = ((gray.width() as f32) * scale).round().max(1.0) as u32;
        let new_h = ((gray.height() as f32) * scale).round().max(1.0) as u32;
        gray = imageops::resize(&gray, new_w, new_h, imageops::FilterType::Lanczos3);
    }

    // Stretch contrast so light HUD text separates from dark/gradient panels.
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
) -> Result<String, String> {
    let mut engine = tesseract::Tesseract::new(None, Some("eng"))
        .map_err(|e| format!("Tesseract init failed: {e}"))?
        .set_frame(frame, width, height, bytes_per_pixel, bytes_per_line)
        .map_err(|e| format!("Tesseract set_frame failed: {e}"))?
        .set_source_resolution(192)
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
}
