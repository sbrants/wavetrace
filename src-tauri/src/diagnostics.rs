//! One-shot target-window capture + OCR for Settings diagnostics and debug packages.

use std::io::Cursor;
use std::time::Instant;

use image::{ImageFormat, RgbaImage};
use serde::Serialize;

use crate::capture;
use crate::db;
use crate::fields;
use crate::parser::CoinReading;
use crate::settings::{self, TargetWindow};
use crate::state_machine::{DissonanceKind, GameMode};

const MAX_PROBE_LINES: usize = 80;
const PREVIEW_MAX_W: u32 = 400;

#[derive(Debug, Clone, Serialize)]
pub struct TargetWindowProbe {
    pub configured_target: Option<TargetWindow>,
    pub resolved_target: Option<TargetWindow>,
    pub resolve_error: Option<String>,
    pub window_found: bool,
    pub capture_ms: u64,
    pub ocr_ms: u64,
    pub width: u32,
    pub height: u32,
    pub tier: Option<u32>,
    pub wave: Option<u32>,
    pub coin_per_minute: Option<f64>,
    pub coin_status: String,
    pub mode: String,
    pub dissonance: Option<DissonanceKind>,
    pub line_count: usize,
    pub sample_lines: Vec<String>,
    pub coin_lines: Vec<String>,
}

pub struct TargetWindowProbeBundle {
    pub probe: TargetWindowProbe,
    pub preview_png: Option<Vec<u8>>,
    /// Full OCR line list (Settings probe UI); debug package uses `probe.sample_lines` only.
    pub all_lines: Vec<String>,
}

pub fn preview_thumbnail(img: &RgbaImage) -> RgbaImage {
    if img.width() <= PREVIEW_MAX_W {
        return img.clone();
    }
    let scale = PREVIEW_MAX_W as f32 / img.width() as f32;
    let h = ((img.height() as f32) * scale).round() as u32;
    image::imageops::resize(img, PREVIEW_MAX_W, h.max(1), image::imageops::FilterType::Triangle)
}

pub fn encode_preview_png(img: &RgbaImage) -> Option<Vec<u8>> {
    let thumb = preview_thumbnail(img);
    let mut bytes = Vec::new();
    thumb
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .ok()?;
    Some(bytes)
}

fn coin_status(coin: CoinReading) -> &'static str {
    match coin {
        CoinReading::Rate(_) => "rate",
        CoinReading::Total(_) => "total_balance",
        CoinReading::Unreadable => "unreadable",
    }
}

fn game_mode_label(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Normal => "normal",
        GameMode::TotalCoin => "total_coin",
        GameMode::IntroSprint => "intro_sprint",
        GameMode::Tournament => "tournament",
        GameMode::EndOfRun => "end_of_run",
        GameMode::Unknown => "unknown",
    }
}

/// Capture the configured (or auto-resolved) game window and OCR it once.
pub fn probe_target_window() -> Result<TargetWindowProbeBundle, String> {
    let conn = db::open().map_err(|e| e.to_string())?;
    let settings = settings::load(&conn);
    let configured_target = settings.target_window.clone();
    let (resolved_target, resolve_error) = match settings::resolve_target_window(&conn) {
        Ok(tw) => (Some(tw.clone()), None),
        Err(e) => (None, Some(e)),
    };

    let Some(target) = resolved_target.clone() else {
        return Ok(TargetWindowProbeBundle {
            probe: TargetWindowProbe {
                configured_target,
                resolved_target: None,
                resolve_error,
                window_found: false,
                capture_ms: 0,
                ocr_ms: 0,
                width: 0,
                height: 0,
                tier: None,
                wave: None,
                coin_per_minute: None,
                coin_status: "no_target".into(),
                mode: "unknown".into(),
                dissonance: None,
                line_count: 0,
                sample_lines: Vec::new(),
                coin_lines: Vec::new(),
            },
            preview_png: None,
            all_lines: Vec::new(),
        });
    };

    let capture_started = Instant::now();
    let frame = capture::capture_target(&target);
    let capture_ms = capture_started.elapsed().as_millis() as u64;

    let Some(img) = frame else {
        return Ok(TargetWindowProbeBundle {
            probe: TargetWindowProbe {
                configured_target,
                resolved_target: Some(target),
                resolve_error,
                window_found: false,
                capture_ms,
                ocr_ms: 0,
                width: 0,
                height: 0,
                tier: None,
                wave: None,
                coin_per_minute: None,
                coin_status: "window_not_found".into(),
                mode: "unknown".into(),
                dissonance: None,
                line_count: 0,
                sample_lines: Vec::new(),
                coin_lines: Vec::new(),
            },
            preview_png: None,
            all_lines: Vec::new(),
        });
    };

    let ocr_started = Instant::now();
    let fields = fields::ocr_probe_fields(&img)?;
    let ocr_ms = ocr_started.elapsed().as_millis() as u64;
    let input = fields::poll_input_from_fields(&fields, &img);
    let coin_per_minute = match input.coin {
        CoinReading::Rate(v) => Some(v),
        _ => None,
    };
    let coin_lines: Vec<String> = fields
        .all_lines
        .iter()
        .filter(|l| l.to_lowercase().contains("/min"))
        .cloned()
        .collect();
    let line_count = fields.all_lines.len();
    let sample_lines = fields
        .all_lines
        .iter()
        .take(MAX_PROBE_LINES)
        .cloned()
        .collect();
    let preview_png = encode_preview_png(&img);

    Ok(TargetWindowProbeBundle {
        probe: TargetWindowProbe {
            configured_target,
            resolved_target: Some(target),
            resolve_error,
            window_found: true,
            capture_ms,
            ocr_ms,
            width: img.width(),
            height: img.height(),
            tier: input.tier,
            wave: input.wave,
            coin_per_minute,
            coin_status: coin_status(input.coin).into(),
            mode: game_mode_label(input.mode).into(),
            dissonance: input.dissonance,
            line_count,
            sample_lines,
            coin_lines,
        },
        preview_png,
        all_lines: fields.all_lines,
    })
}
