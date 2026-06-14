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
    Win32::System::WinRT::{RoInitialize, IMemoryBufferByteAccess, RO_INIT_MULTITHREADED},
};

#[cfg(windows)]
use futures::executor::block_on;

#[cfg(windows)]
static WINRT_INIT: OnceLock<Result<(), String>> = OnceLock::new();

#[cfg(windows)]
static OCR_ENGINE: OnceLock<Result<OcrEngine, String>> = OnceLock::new();

#[cfg(windows)]
static OCR_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

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
    let dynamic = prepare_image(img);
    let rgba = dynamic.to_rgba8();
    let width = rgba.width() as i32;
    let height = rgba.height() as i32;
    let bytes_per_line = width
        .checked_mul(4)
        .ok_or_else(|| "Image row byte width overflow".to_string())?;
    let text = tesseract::ocr_from_frame(
        rgba.as_raw(),
        width,
        height,
        4,
        bytes_per_line,
        "eng",
    )
    .map_err(|e| format!("Tesseract OCR failed: {e}"))?;
    let lines = split_lines(&text);
    if lines.is_empty() {
        return Err("Tesseract OCR returned no text".into());
    }
    Ok(lines)
}

#[cfg(windows)]
fn init_winrt() -> Result<(), String> {
    WINRT_INIT
        .get_or_init(|| unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)
                .map_err(|e| format!("RoInitialize failed: {e}"))
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
