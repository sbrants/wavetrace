//! Full-frame OCR via Tesseract (Goal.md recommended engine).
//!
//! Install: `winget install UB-Mannheim.TesseractOCR`
//! The app auto-detects `C:\Program Files\Tesseract-OCR` if it is not on PATH.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use image::RgbaImage;
use rusty_tesseract::{image_to_string, Args, Image};

/// Tesseract is invoked as an external process — serialize calls.
static OCR_SERIAL: Mutex<()> = Mutex::new(());

static TESSERACT_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// OCR the entire capture and return every non-empty text line discovered.
pub fn ocr_full_frame(img: &RgbaImage) -> Result<Vec<String>, String> {
    let _guard = OCR_SERIAL
        .lock()
        .map_err(|e| format!("OCR lock poisoned: {e}"))?;

    configure_tesseract_env()?;

    let dynamic = prepare_image(img);
    let tess_img = Image::from_dynamic_image(&dynamic).map_err(|e| {
        format!(
            "Failed to prepare image for Tesseract: {e}. \
             Install Tesseract and ensure `tesseract` is on PATH."
        )
    })?;

    let args = Args {
        lang: "eng".to_string(),
        config_variables: HashMap::new(),
        dpi: Some(150),
        // Sparse game UI text across the full window.
        psm: Some(11),
        oem: Some(3),
    };

    let text = image_to_string(&tess_img, &args).map_err(|e| {
        format!(
            "Tesseract OCR failed: {e}. \
             Install Tesseract (https://github.com/UB-Mannheim/tesseract/wiki) and add it to PATH."
        )
    })?;

    Ok(split_lines(&text))
}

/// Locate Tesseract and expose it to child processes (winget often omits PATH).
fn configure_tesseract_env() -> Result<(), String> {
    let dir = TESSERACT_DIR.get_or_init(find_tesseract_install_dir);
    let Some(dir) = dir else {
        return Err(
            "Tesseract not found. Install with: winget install UB-Mannheim.TesseractOCR"
                .into(),
        );
    };

    let tessdata = dir.join("tessdata");
    if tessdata.is_dir() {
        std::env::set_var("TESSDATA_PREFIX", &tessdata);
    }

    let dir_str = dir.to_string_lossy();
    let path = std::env::var("PATH").unwrap_or_default();
    if !path
        .split(';')
        .any(|entry| entry.eq_ignore_ascii_case(dir_str.as_ref()))
    {
        std::env::set_var("PATH", format!("{dir_str};{path}"));
    }

    Ok(())
}

fn find_tesseract_install_dir() -> Option<PathBuf> {
    if let Ok(cmd) = std::env::var("TESSERACT_CMD") {
        let exe = PathBuf::from(&cmd);
        if exe.is_file() {
            return exe.parent().map(|p| p.to_path_buf());
        }
    }

    for candidate in [
        r"C:\Program Files\Tesseract-OCR",
        r"C:\Program Files (x86)\Tesseract-OCR",
    ] {
        let dir = PathBuf::from(candidate);
        if dir.join("tesseract.exe").is_file() {
            return Some(dir);
        }
    }

    None
}

/// Downscale large emulator frames so Tesseract stays responsive.
fn prepare_image(img: &RgbaImage) -> image::DynamicImage {
    const MAX_WIDTH: u32 = 900;
    if img.width() <= MAX_WIDTH {
        return image::DynamicImage::ImageRgba8(img.clone());
    }
    let scale = MAX_WIDTH as f32 / img.width() as f32;
    let new_h = ((img.height() as f32) * scale).round().max(1.0) as u32;
    let resized = image::imageops::resize(
        img,
        MAX_WIDTH,
        new_h,
        image::imageops::FilterType::Triangle,
    );
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
