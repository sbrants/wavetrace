//! Field OCR via full-frame capture plus a dedicated Golden Combo HUD strip.

use std::time::Instant;

use image::RgbaImage;

use crate::classify;
use crate::ocr::{self, GoldenComboBandOcr};
use crate::parser::{self, is_golden_combo_candidate_line, parse_golden_combo};
use crate::state_machine::PollInput;

#[derive(Debug, Clone)]
pub struct FieldOcr {
    /// All non-empty text lines from the capture.
    pub all_lines: Vec<String>,
    /// Lines from the dedicated Golden Combo band (also prepended into `all_lines`).
    /// When the band misses, may hold full-frame GC salvage candidates for logging.
    pub gc_band_lines: Vec<String>,
    /// Exit Battle bottom as a fraction of frame height, when located.
    pub exit_battle_y: Option<f32>,
    /// Total OCR wall time (full-frame + GC band).
    pub ocr_ms: u64,
    pub full_ms: u64,
    pub gc_ms: u64,
    pub gc_yellow_ms: u64,
    pub gc_color_ms: u64,
    pub gc_ink: u32,
    /// `"-"`, `"ink"` (too little yellow), or `"busy"` (FX soup).
    pub gc_skip: &'static str,
}

impl Default for FieldOcr {
    fn default() -> Self {
        Self {
            all_lines: Vec::new(),
            gc_band_lines: Vec::new(),
            exit_battle_y: None,
            ocr_ms: 0,
            full_ms: 0,
            gc_ms: 0,
            gc_yellow_ms: 0,
            gc_color_ms: 0,
            gc_ink: 0,
            gc_skip: "-",
        }
    }
}

/// OCR tracked fields from the full window capture.
pub fn ocr_all_fields(frame: &RgbaImage) -> FieldOcr {
    ocr_all_fields_cancellable(
        frame,
        &|| true,
        ocr::DEFAULT_OCR_MAX_WIDTH,
        ocr::DEFAULT_GC_MAX_INK,
    )
}

/// Like [`ocr_all_fields`] but returns promptly when `should_continue` is false, and
/// takes the pre-OCR downscale target width (see
/// [`ocr::ocr_full_frame_located_with_max_width`]) and the GC "too busy" ink ceiling
/// (see [`ocr::ocr_golden_combo_band_anchored_with_max_ink`]) explicitly.
pub fn ocr_all_fields_cancellable<F: Fn() -> bool>(
    frame: &RgbaImage,
    should_continue: &F,
    max_ocr_width: u32,
    gc_max_ink: u32,
) -> FieldOcr {
    if !should_continue() {
        return FieldOcr::default();
    }

    let started = Instant::now();
    let full_started = Instant::now();
    match ocr::ocr_full_frame_located_with_max_width(frame, max_ocr_width) {
        Ok(located) => {
            let full_ms = full_started.elapsed().as_millis() as u64;
            let exit_bottom = ocr::exit_battle_bottom_norm(&located);
            let mut all_lines: Vec<String> = located.into_iter().map(|l| l.text).collect();

            // GC is a floating yellow toast (rises and fades; Y shifts with skip/etc.).
            // Scan the toast travel corridor — full-frame downscale often misses it.
            let gc = if should_continue() {
                ocr::ocr_golden_combo_band_anchored_with_max_ink(frame, exit_bottom, gc_max_ink)
            } else {
                GoldenComboBandOcr::default()
            };
            let mut gc_band_lines = gc.lines.clone();

            let band_has_gc = gc_band_lines
                .iter()
                .any(|l| is_golden_combo_candidate_line(l));
            if band_has_gc {
                // Dedicated band first so parse prefers its cleaner toast crop.
                let mut merged = gc_band_lines.clone();
                for line in &all_lines {
                    if !merged.iter().any(|e| e == line) {
                        merged.push(line.clone());
                    }
                }
                all_lines = merged;
            } else {
                // Band empty/junk — salvage GC-like lines already present in full-frame OCR.
                let salvaged = salvage_gc_lines_from_frame(&all_lines);
                if !salvaged.is_empty() {
                    gc_band_lines = salvaged.clone();
                    let mut merged = salvaged;
                    for line in &all_lines {
                        if !merged.iter().any(|e| e == line) {
                            merged.push(line.clone());
                        }
                    }
                    all_lines = merged;
                }
            }

            FieldOcr {
                all_lines,
                gc_band_lines,
                exit_battle_y: exit_bottom,
                ocr_ms: started.elapsed().as_millis() as u64,
                full_ms,
                gc_ms: gc.gc_ms(),
                gc_yellow_ms: gc.yellow_ms,
                gc_color_ms: gc.color_ms,
                gc_ink: gc.ink_pixels,
                gc_skip: gc.skip,
            }
        }
        Err(e) => {
            eprintln!("OCR error: {e}");
            crate::db::append_app_log(&format!("OCR error: {e}"));
            let full_ms = full_started.elapsed().as_millis() as u64;
            let gc = if should_continue() {
                ocr::ocr_golden_combo_band(frame)
            } else {
                GoldenComboBandOcr::default()
            };
            FieldOcr {
                all_lines: gc.lines.clone(),
                gc_band_lines: gc.lines.clone(),
                exit_battle_y: None,
                ocr_ms: started.elapsed().as_millis() as u64,
                full_ms,
                gc_ms: gc.gc_ms(),
                gc_yellow_ms: gc.yellow_ms,
                gc_color_ms: gc.color_ms,
                gc_ink: gc.ink_pixels,
                gc_skip: gc.skip,
            }
        }
    }
}

/// GC toast band only — skips full-frame OCR. Used between full HUD polls.
/// `exit_bottom_norm` should be the last known Exit Battle bottom from a full poll
/// (falls back to the default toast corridor when `None`). `gc_max_ink` is the "too
/// busy to be a toast" ceiling — see [`ocr::ocr_golden_combo_band_anchored_with_max_ink`].
pub fn ocr_gc_only_cancellable<F: Fn() -> bool>(
    frame: &RgbaImage,
    exit_bottom_norm: Option<f32>,
    should_continue: &F,
    gc_max_ink: u32,
) -> FieldOcr {
    if !should_continue() {
        return FieldOcr::default();
    }
    let started = Instant::now();
    let gc = ocr::ocr_golden_combo_band_anchored_with_max_ink(frame, exit_bottom_norm, gc_max_ink);
    FieldOcr {
        all_lines: gc.lines.clone(),
        gc_band_lines: gc.lines.clone(),
        exit_battle_y: exit_bottom_norm,
        ocr_ms: started.elapsed().as_millis() as u64,
        full_ms: 0,
        gc_ms: gc.gc_ms(),
        gc_yellow_ms: gc.yellow_ms,
        gc_color_ms: gc.color_ms,
        gc_ink: gc.ink_pixels,
        gc_skip: gc.skip,
    }
}

/// Parse Golden Combo from a GC-only band OCR result (no full-frame salvage).
pub fn golden_combo_from_gc_only(fields: &FieldOcr) -> parser::GoldenComboReading {
    parse_golden_combo(&fields.gc_band_lines)
}

fn salvage_gc_lines_from_frame(all_lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, line) in all_lines.iter().enumerate() {
        if !is_golden_combo_candidate_line(line) {
            continue;
        }
        // Keep one neighbor on each side so split `xo.0N` / caret crumbs still parse.
        let start = i.saturating_sub(1);
        let end = (i + 2).min(all_lines.len());
        for part in &all_lines[start..end] {
            if !out.iter().any(|e| e == part) {
                out.push((*part).clone());
            }
        }
    }
    out
}

pub fn poll_input_from_fields(fields: &FieldOcr, frame: &RgbaImage) -> PollInput {
    let mut input = classify::classify(&fields.all_lines);
    // Explicit dual parse: dedicated band + full-frame GC candidates, then merge.
    // Classify already parsed all_lines; this fills gaps when the band missed the toast
    // but full-frame still saw a GC-like crumb (common with short-lived toasts).
    input.golden_combo = merge_golden_combo_from_fields(fields);
    if input.dissonance.is_none() {
        input.dissonance = crate::dissonance_icons::detect(frame);
    }
    input
}

fn merge_golden_combo_from_fields(fields: &FieldOcr) -> parser::GoldenComboReading {
    let from_band = parse_golden_combo(&fields.gc_band_lines);
    let frame_candidates = salvage_gc_lines_from_frame(&fields.all_lines);
    let from_frame = parse_golden_combo(&frame_candidates);
    // Band first (yellow crop), frame salvage supplies chance/caret/mult the band omitted.
    from_band.merge_with(from_frame)
}

/// One-shot OCR for Settings diagnostics (same as a normal poll). A panic in here
/// unwinds into the Tauri event loop, which aborts the whole app, so report it instead.
pub fn ocr_probe_fields(frame: &RgbaImage) -> Result<FieldOcr, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ocr_all_fields(frame)))
        .map_err(|_| "OCR failed on this frame — see the app log for details.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::GoldenComboReading;

    #[test]
    fn salvage_picks_full_frame_gc_when_band_is_junk() {
        let fields = FieldOcr {
            all_lines: vec![
                "oo".into(),
                "000".into(),
                "The Tower 7.0.6.2".into(),
                "0.03%288 = xo.09!".into(),
                "EXIT BATTLE".into(),
            ],
            gc_band_lines: vec!["oo".into(), "000".into(), "00".into()],
            exit_battle_y: Some(0.35),
            ocr_ms: 1,
            ..Default::default()
        };
        let g = merge_golden_combo_from_fields(&fields);
        assert!(g.seen);
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(288));
        assert_eq!(g.multiplier, Some(0.09));
    }

    #[test]
    fn band_reading_merges_with_frame_salvage() {
        let fields = FieldOcr {
            all_lines: vec![
                "Golden Combo: 0.03% A310 =".into(),
                "xo.10!".into(),
            ],
            gc_band_lines: vec!["Golden Combo: 0.03% A310 =".into()],
            exit_battle_y: None,
            ocr_ms: 1,
            ..Default::default()
        };
        let g = merge_golden_combo_from_fields(&fields);
        assert_eq!(
            g,
            GoldenComboReading {
                seen: true,
                chance_percent: Some(0.03),
                caret_count: Some(310),
                multiplier: Some(0.1),
            }
        );
    }

    #[test]
    fn gc_only_parse_uses_band_lines_without_frame_salvage() {
        let fields = FieldOcr {
            all_lines: vec!["noise".into()],
            gc_band_lines: vec!["Golden Combo: 0.03% ^200 = x0.08".into()],
            exit_battle_y: Some(0.4),
            full_ms: 0,
            ..Default::default()
        };
        let g = golden_combo_from_gc_only(&fields);
        assert!(g.seen);
        assert_eq!(g.caret_count, Some(200));
        assert_eq!(g.multiplier, Some(0.08));
    }
}
