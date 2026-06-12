//! Full-frame field OCR — one Tesseract pass over the entire capture.

use image::RgbaImage;

use crate::ocr;

#[derive(Debug, Default, Clone)]
pub struct FieldOcr {
    /// Every non-empty line Tesseract found in the capture.
    pub all_lines: Vec<String>,
}

/// OCR the full window capture once.
pub fn ocr_all_fields(frame: &RgbaImage) -> FieldOcr {
    ocr_all_fields_cancellable(frame, &|| true)
}

/// Like [`ocr_all_fields`] but returns promptly when `should_continue` is false.
pub fn ocr_all_fields_cancellable<F: Fn() -> bool>(
    frame: &RgbaImage,
    should_continue: &F,
) -> FieldOcr {
    if !should_continue() {
        return FieldOcr::default();
    }
    match ocr::ocr_full_frame(frame) {
        Ok(all_lines) => FieldOcr { all_lines },
        Err(e) => {
            eprintln!("OCR error: {e}");
            FieldOcr::default()
        }
    }
}

/// One-shot OCR for Settings diagnostics (same as a normal poll).
pub fn ocr_probe_fields(frame: &RgbaImage) -> Result<FieldOcr, String> {
    let all_lines = ocr::ocr_full_frame(frame)?;
    Ok(FieldOcr { all_lines })
}
