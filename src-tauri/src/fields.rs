//! Field OCR via a single full-frame Windows OCR pass.

use std::time::Instant;

use image::RgbaImage;

use crate::classify;
use crate::ocr;
use crate::state_machine::PollInput;

#[derive(Debug, Default, Clone)]
pub struct FieldOcr {
    /// All non-empty text lines from the capture.
    pub all_lines: Vec<String>,
    pub ocr_ms: u64,
}

/// OCR tracked fields from the full window capture.
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

    let started = Instant::now();
    match ocr::ocr_full_frame(frame) {
        Ok(all_lines) => FieldOcr {
            all_lines,
            ocr_ms: started.elapsed().as_millis() as u64,
        },
        Err(e) => {
            eprintln!("OCR error: {e}");
            FieldOcr {
                ocr_ms: started.elapsed().as_millis() as u64,
                ..Default::default()
            }
        }
    }
}

pub fn poll_input_from_fields(fields: &FieldOcr) -> PollInput {
    classify::classify(&fields.all_lines)
}

/// One-shot OCR for Settings diagnostics (same as a normal poll).
pub fn ocr_probe_fields(frame: &RgbaImage) -> Result<FieldOcr, String> {
    Ok(ocr_all_fields(frame))
}
