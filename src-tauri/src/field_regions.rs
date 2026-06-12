//! Region OCR — proportional HUD crops with cached tier/wave panel matching.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use image::{GrayImage, RgbaImage};

use crate::anchor;
use crate::capture;
use crate::classify;
use crate::ocr;
use crate::parser::{parse_coin_anchor_crop, parse_tier, parse_wave, CoinReading};
use crate::state_machine::PollInput;

#[derive(Debug, Clone, Copy)]
pub struct RegionRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Default, Clone)]
pub struct RegionOcrResult {
    pub tier_text: Option<String>,
    pub wave_text: Option<String>,
    pub coin_text: Option<String>,
    pub mode_text: Option<String>,
    pub all_lines: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RegionTiming {
    pub match_ms: u64,
    pub ocr_ms: u64,
}

const PANEL_MATCH_THRESHOLD: f32 = 0.58;

static TIER_WAVE_TEMPLATE: OnceLock<(GrayImage, u32, u32)> = OnceLock::new();
static PANEL_CACHE: Mutex<Option<((u32, u32), RegionRect)>> = Mutex::new(None);

fn tier_wave_template() -> &'static (GrayImage, u32, u32) {
    TIER_WAVE_TEMPLATE.get_or_init(|| {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/Wave_and_Tier.png");
        let img = image::open(&path)
            .unwrap_or_else(|e| panic!("fixture missing {}: {e}", path.display()))
            .to_rgba8();
        let w = img.width();
        let h = img.height();
        (anchor::to_gray(&img), w, h)
    })
}

pub fn ocr_frame_regions_cancellable<F: Fn() -> bool>(
    frame: &RgbaImage,
    should_continue: &F,
) -> (RegionOcrResult, RegionTiming) {
    let mut timing = RegionTiming::default();

    if !should_continue() {
        return (RegionOcrResult::default(), timing);
    }

    let mut result = RegionOcrResult::default();
    let match_started = std::time::Instant::now();
    let panel = resolve_tier_wave_panel(frame);
    timing.match_ms = match_started.elapsed().as_millis() as u64;

    let ocr_started = std::time::Instant::now();

    let panel_rect = panel.or_else(|| fixed_tier_wave_panel(frame.width(), frame.height()));
    let coin_rect = fixed_coin_text_rect(frame.width(), frame.height());
    let mode_rect = mode_band_rect(frame.width(), frame.height());

    let coin_crop = crop_frame_rect(frame, coin_rect);
    let panel_crop = panel_rect.and_then(|r| crop_frame_rect(frame, r));
    let mode_crop = crop_frame_rect(frame, mode_rect);

    let mut tasks: Vec<(RgbaImage, i32, bool)> = Vec::with_capacity(3);
    let mut task_kinds: Vec<u8> = Vec::with_capacity(3);
    if let Some(c) = coin_crop {
        tasks.push((c, 7, true));
        task_kinds.push(0);
    }
    if let Some(c) = panel_crop {
        tasks.push((c, 6, false));
        task_kinds.push(1);
    }
    if let Some(c) = mode_crop {
        tasks.push((c, 7, false));
        task_kinds.push(2);
    }

    if !should_continue() {
        timing.ocr_ms = ocr_started.elapsed().as_millis() as u64;
        return (result, timing);
    }

    let ocr_results = ocr::ocr_parallel(tasks);
    for (kind, ocr_out) in task_kinds.iter().zip(ocr_results.iter()) {
        let Ok(text) = ocr_out else { continue };
        match kind {
            0 => {
                if !text.is_empty() {
                    result.coin_text = Some(text.clone());
                    result.all_lines.push(text.clone());
                }
            }
            1 => apply_tier_wave_panel_text(text, &mut result),
            2 => {
                if !text.trim().is_empty() {
                    result.mode_text = Some(text.clone());
                    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
                        result.all_lines.push(line.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    timing.ocr_ms = ocr_started.elapsed().as_millis() as u64;
    (result, timing)
}

/// Clear cached tier/wave panel position (e.g. after resolution change).
pub fn clear_anchor_cache() {
    if let Ok(mut guard) = PANEL_CACHE.lock() {
        *guard = None;
    }
}

fn resolve_tier_wave_panel(frame: &RgbaImage) -> Option<RegionRect> {
    let key = (frame.width(), frame.height());
    if let Ok(guard) = PANEL_CACHE.lock() {
        if let Some((cached_key, panel)) = guard.as_ref() {
            if *cached_key == key {
                return Some(*panel);
            }
        }
    }

    let panel = locate_tier_wave_panel(frame)?;
    if let Ok(mut guard) = PANEL_CACHE.lock() {
        *guard = Some((key, panel));
    }
    Some(panel)
}

fn locate_tier_wave_panel(frame: &RgbaImage) -> Option<RegionRect> {
    let (template, tw, th) = tier_wave_template();
    let roi = tier_wave_search_roi(frame.width(), frame.height());
    let roi_img = crop_frame_rect(frame, roi)?;
    let roi_gray = anchor::to_gray(&roi_img);

    let m = anchor::locate(&roi_gray, template)?;
    if m.confidence < PANEL_MATCH_THRESHOLD {
        return None;
    }

    Some(RegionRect {
        x: roi.x + m.x,
        y: roi.y + m.y,
        w: *tw,
        h: *th,
    })
}

fn tier_wave_search_roi(frame_w: u32, frame_h: u32) -> RegionRect {
    RegionRect {
        x: (frame_w as f32 * 0.48) as u32,
        y: (frame_h as f32 * 0.58) as u32,
        w: (frame_w as f32 * 0.50) as u32,
        h: (frame_h as f32 * 0.10) as u32,
    }
}

/// Fallback panel origin when template match fails (1080×2400 reference).
fn fixed_tier_wave_panel(frame_w: u32, frame_h: u32) -> Option<RegionRect> {
    let (_, tw, th) = tier_wave_template();
    Some(RegionRect {
        x: pct_rect(frame_w, frame_h, 0.504, 0.607, 0.0, 0.0).x,
        y: pct_rect(frame_w, frame_h, 0.0, 0.607, 0.0, 0.0).y,
        w: *tw,
        h: *th,
    })
}

/// Coin rate text to the right of the coin icon (second row of top resource bar).
fn fixed_coin_text_rect(frame_w: u32, frame_h: u32) -> RegionRect {
    pct_rect(frame_w, frame_h, 0.095, 0.048, 0.28, 0.021)
}

fn mode_band_rect(frame_w: u32, frame_h: u32) -> RegionRect {
    RegionRect {
        x: (frame_w as f32 * 0.12) as u32,
        y: (frame_h as f32 * 0.54) as u32,
        w: (frame_w as f32 * 0.76) as u32,
        h: (frame_h as f32 * 0.10) as u32,
    }
}

fn pct_rect(frame_w: u32, frame_h: u32, x: f32, y: f32, w: f32, h: f32) -> RegionRect {
    RegionRect {
        x: (frame_w as f32 * x).round() as u32,
        y: (frame_h as f32 * y).round() as u32,
        w: ((frame_w as f32 * w).round() as u32).max(1),
        h: ((frame_h as f32 * h).round() as u32).max(1),
    }
}

fn apply_tier_wave_panel_text(text: &str, result: &mut RegionOcrResult) {
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        result.all_lines.push(line.to_string());
        let lower = line.to_lowercase();
        if result.tier_text.is_none() && lower.contains("tier") {
            if let Some(idx) = lower.find("tier") {
                let sub = line[idx..].trim();
                if parse_tier(sub).is_some() {
                    result.tier_text = Some(sub.to_string());
                }
            }
        }
        if result.wave_text.is_none() && lower.contains("wave") {
            if let Some(idx) = lower.find("wave") {
                let sub = line[idx..].trim();
                if parse_wave(sub).is_some() {
                    result.wave_text = Some(sub.to_string());
                }
            }
        }
    }
}

fn crop_frame_rect(frame: &RgbaImage, rect: RegionRect) -> Option<RgbaImage> {
    if rect.w == 0 || rect.h == 0 {
        return None;
    }
    if rect.x >= frame.width() || rect.y >= frame.height() {
        return None;
    }
    Some(capture::crop_region(frame, rect.x, rect.y, rect.w, rect.h))
}

pub fn regions_to_poll_input(regions: &RegionOcrResult) -> PollInput {
    let mut tournament = false;
    let mut end_of_run = false;
    let mut intro_sprint = false;

    if let Some(mode) = &regions.mode_text {
        let lower = mode.to_lowercase();
        if lower.contains("retry") || lower.contains("game stats") {
            end_of_run = true;
        }
        if lower.contains("intro sprint") {
            intro_sprint = true;
        }
    }

    let mut tier = None;
    if let Some(t) = regions.tier_text.as_deref() {
        if let Some((v, plus)) = parse_tier(t) {
            tournament |= plus;
            tier = Some(v);
        }
    }

    let wave = regions.wave_text.as_deref().and_then(parse_wave);

    let coin = regions
        .coin_text
        .as_deref()
        .map(parse_coin_anchor_crop)
        .unwrap_or(CoinReading::Unreadable);

    let mode_lines: Vec<String> = regions
        .mode_text
        .as_deref()
        .map(|t| t.lines().map(str::to_string).collect())
        .unwrap_or_default();
    let classified = classify::classify_from_parts(
        tier,
        wave,
        coin,
        &mode_lines,
        tournament,
        end_of_run,
        intro_sprint,
    );

    PollInput {
        mode: classified.mode,
        tier: classified.tier,
        wave: classified.wave,
        coin: classified.coin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::CoinReading;
    use crate::state_machine::GameMode;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
    }

    fn load_fixture(name: &str) -> RgbaImage {
        image::open(fixtures_dir().join(name))
            .expect("fixture")
            .to_rgba8()
    }

    #[test]
    fn tier_wave_panel_matches_reference() {
        clear_anchor_cache();
        let frame = load_fixture("expected_state_full_game.png");
        let panel = locate_tier_wave_panel(&frame).expect("panel match");
        eprintln!(
            "panel @ ({}, {}) {}x{} frame {}x{}",
            panel.x,
            panel.y,
            panel.w,
            panel.h,
            frame.width(),
            frame.height()
        );
    }

    #[test]
    #[ignore = "writes debug crops to fixtures/captured/"]
    fn dump_region_crops() {
        clear_anchor_cache();
        let frame = load_fixture("expected_state_full_game.png");
        let panel = locate_tier_wave_panel(&frame);
        let coin = fixed_coin_text_rect(frame.width(), frame.height());
        let out = fixtures_dir().join("captured");
        std::fs::create_dir_all(&out).ok();

        if let Some(panel) = panel {
            eprintln!("panel {panel:?}");
            if let Some(crop) = crop_frame_rect(&frame, panel) {
                crop.save(out.join("debug_tier_wave_panel.png")).ok();
            }
        }
        if let Some(crop) = crop_frame_rect(&frame, coin) {
            crop.save(out.join("debug_coin.png")).ok();
            eprintln!("coin {coin:?}");
        }
    }

    #[test]
    #[ignore = "requires Windows OCR"]
    fn region_ocr_parses_reference_screenshot() {
        clear_anchor_cache();
        let frame = load_fixture("expected_state_full_game.png");
        let (regions, timing) = ocr_frame_regions_cancellable(&frame, &|| true);
        eprintln!("timing {:?}", timing);
        eprintln!("regions {:?}", regions);
        let input = regions_to_poll_input(&regions);
        assert_eq!(input.tier, Some(12));
        assert_eq!(input.wave, Some(4571));
        assert_eq!(input.coin, CoinReading::Rate(3.48e12));
        assert_eq!(input.mode, GameMode::Normal);
    }
}
