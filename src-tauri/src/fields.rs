//! Anchor-based field OCR using bundled fixture images.
//!
//! * **Coin/min** — icon templates ($, coin, diamond) extracted from
//!   `coin_per_minute_location.png`; the coin row is OCR'd once the bar is found.
//! * **Tier/Wave** — `Wave_and_Tier.png` panel template match, then OCR the card.

use image::{GrayImage, RgbaImage};

use crate::{anchor, capture, ocr};

/// Reference height the anchor fixtures were captured against.
const REF_H: u32 = 2400;

#[derive(Debug, Default, Clone)]
pub struct FieldOcr {
    pub tier_wave_lines: Vec<String>,
    pub coin_lines: Vec<String>,
    pub mode_lines: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct FracRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Debug, Clone)]
struct TemplateMatch {
    x: u32,
    y: u32,
    confidence: f32,
}

fn fixture_path(name: &str) -> String {
    format!("{}/../fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

fn load_fixture(name: &str) -> Option<RgbaImage> {
    image::open(fixture_path(name)).ok().map(|i| i.to_rgba8())
}

fn crop_frac(img: &RgbaImage, rect: FracRect) -> RgbaImage {
    let x = (rect.x * img.width() as f32).round() as u32;
    let y = (rect.y * img.height() as f32).round() as u32;
    let w = (rect.w * img.width() as f32).round() as u32;
    let h = (rect.h * img.height() as f32).round() as u32;
    capture::crop_region(img, x, y, w.max(1), h.max(1))
}

fn scale_sy(frame: &RgbaImage) -> f32 {
    frame.height() as f32 / REF_H as f32
}

fn resize_sy(img: &RgbaImage, frame: &RgbaImage) -> RgbaImage {
    let sy = scale_sy(frame);
    let w = ((img.width() as f32) * sy).round() as u32;
    let h = ((img.height() as f32) * sy).round() as u32;
    image::imageops::resize(
        img,
        w.max(1),
        h.max(1),
        image::imageops::FilterType::Triangle,
    )
}

/// Downscale large search images so template matching stays fast.
fn locate_template(search: &RgbaImage, template: &GrayImage) -> Option<TemplateMatch> {
    const MAX_SEARCH_W: u32 = 640;
    let scale = if search.width() > MAX_SEARCH_W {
        search.width() as f32 / MAX_SEARCH_W as f32
    } else {
        1.0
    };
    let scaled_search = if scale > 1.0 {
        let sw = MAX_SEARCH_W;
        let sh = ((search.height() as f32) / scale) as u32;
        image::imageops::resize(search, sw, sh.max(1), image::imageops::FilterType::Triangle)
    } else {
        search.clone()
    };
    let scaled_template = if scale > 1.0 {
        let iw = ((template.width() as f32) / scale).round() as u32;
        let ih = ((template.height() as f32) / scale).round() as u32;
        image::imageops::resize(
            template,
            iw.max(1),
            ih.max(1),
            image::imageops::FilterType::Triangle,
        )
    } else {
        template.clone()
    };

    let m = anchor::locate(&anchor::to_gray(&scaled_search), &scaled_template)?;
    Some(TemplateMatch {
        x: (m.x as f32 * scale).round() as u32,
        y: (m.y as f32 * scale).round() as u32,
        confidence: m.confidence,
    })
}

fn top_resource_search_band(frame: &RgbaImage) -> (RgbaImage, u32, u32) {
    let w = ((frame.width() as f32) * 0.65).round() as u32;
    let h = ((frame.height() as f32) * 0.28).round() as u32;
    (capture::crop_region(frame, 0, 0, w.max(1), h.max(1)), 0, 0)
}

fn tier_wave_search_band(frame: &RgbaImage) -> (RgbaImage, u32, u32) {
    // Tier/Wave card sits mid-right, above the upgrades header (~28–38% from top).
    let left = (frame.width() as f32 * 0.45) as u32;
    let top = (frame.height() as f32 * 0.26) as u32;
    let h = (frame.height() as f32 * 0.18) as u32;
    (
        capture::crop_region(frame, left, top, frame.width() - left, h.max(1)),
        left,
        top,
    )
}

/// Right-column OCR region for the tier/wave card.
fn tier_wave_ocr_region(frame: &RgbaImage, origin_x: u32, origin_y: u32) -> RgbaImage {
    let w = (frame.width() as f32 * 0.444).round() as u32;
    let h = (frame.height() as f32 * 0.70).round() as u32;
    capture::crop_region(frame, origin_x, origin_y, w.max(200), h.max(400))
}

fn tier_wave_default_origin(frame: &RgbaImage) -> (u32, u32) {
    (
        (frame.width() as f32 * 0.556).round() as u32,
        (frame.height() as f32 * 0.10).round() as u32,
    )
}

/// Bottom strip for Retry / GAME STATS / Intro Sprint detection.
fn mode_crop(frame: &RgbaImage) -> RgbaImage {
    let top = (frame.height() as f32 * 0.72) as u32;
    capture::crop_region(frame, 0, top, frame.width(), frame.height().saturating_sub(top))
}

/// Icon templates for the three resource rows in `coin_per_minute_location.png`.
fn coin_bar_icon_templates(location: &RgbaImage, frame: &RgbaImage) -> Vec<(u32, GrayImage)> {
    let row_h = 1.0 / 3.0;
    let icon_w = 0.22;
    let rows = [
        (0, FracRect { x: 0.0, y: 0.0, w: icon_w, h: row_h }),           // $
        (1, FracRect { x: 0.0, y: row_h, w: icon_w, h: row_h }),         // coin
        (2, FracRect { x: 0.0, y: row_h * 2.0, w: icon_w, h: row_h }),  // diamond
    ];
    rows.into_iter()
        .map(|(idx, rect)| {
            let patch = crop_frac(location, rect);
            let scaled = resize_sy(&patch, frame);
            (idx, anchor::to_gray(&scaled))
        })
        .collect()
}

fn coin_text_crop(
    frame: &RgbaImage,
    location: &RgbaImage,
    band_left: u32,
    band_top: u32,
    icon_match: &TemplateMatch,
    row_index: u32,
) -> RgbaImage {
    let sy = scale_sy(frame);
    let row_h = ((location.height() as f32 / 3.0) * sy).round() as u32;
    let text_x_off = (location.width() as f32 * 0.20 * sy).round() as u32;
    let text_w = (location.width() as f32 * 0.80 * sy).round() as u32;

    let abs_x = band_left + icon_match.x + text_x_off;
    let abs_y = match row_index {
        0 => band_top + icon_match.y + row_h, // $ matched → coin row below
        1 => band_top + icon_match.y,         // coin icon matched
        2 => band_top + icon_match.y.saturating_sub(row_h * 2), // diamond → coin row above
        _ => band_top + icon_match.y,
    };

    capture::crop_region(frame, abs_x, abs_y, text_w.max(40), row_h.max(24))
}

/// Locate the coin/min OCR crop using icon templates from `coin_per_minute_location.png`.
pub fn coin_ocr_crop(frame: &RgbaImage) -> Option<RgbaImage> {
    let location = load_fixture("coin_per_minute_location.png")?;
    let (search, band_left, band_top) = top_resource_search_band(frame);
    let icons = coin_bar_icon_templates(&location, frame);

    let max_icon_y = (search.height() as f32 * 0.55).round() as u32;
    let mut candidates: Vec<(u32, TemplateMatch)> = Vec::new();
    for (row_idx, template) in icons {
        let Some(m) = locate_template(&search, &template) else {
            continue;
        };
        if m.confidence < 0.45 || m.y > max_icon_y {
            continue;
        }
        candidates.push((row_idx, m));
    }
    let row_priority = |row: u32| -> u32 { match row { 1 => 0, 0 => 1, _ => 2 } };
    candidates.sort_by(|a, b| {
        row_priority(a.0)
            .cmp(&row_priority(b.0))
            .then(b.1.confidence.partial_cmp(&a.1.confidence).unwrap())
    });

    if let Some((row_idx, icon_match)) = candidates.into_iter().next() {
        return Some(coin_text_crop(
            frame,
            &location,
            band_left,
            band_top,
            &icon_match,
            row_idx,
        ));
    }

    let full = resize_sy(&location, frame);
    let m = locate_template(&search, &anchor::to_gray(&full))?;
    if m.confidence < 0.38 || m.y > max_icon_y {
        return None;
    }
    let sy = scale_sy(frame);
    let row_h = ((location.height() as f32 / 3.0) * sy).round() as u32;
    let text_x = band_left + m.x + (location.width() as f32 * 0.20 * sy).round() as u32;
    let text_y = band_top + m.y + row_h;
    let text_w = (location.width() as f32 * 0.80 * sy).round() as u32;
    Some(capture::crop_region(
        frame,
        text_x,
        text_y,
        text_w.max(40),
        row_h.max(24),
    ))
}

/// Proportional fallback when icon templates miss (common on scaled emulator frames).
fn coin_fallback_ocr_crop(frame: &RgbaImage) -> RgbaImage {
    crop_frac(
        frame,
        FracRect {
            x: 0.08,
            y: 0.018,
            w: 0.44,
            h: 0.042,
        },
    )
}

/// True when any line parses to an actual coin/min rate (not junk/balance).
fn lines_have_coin_rate(lines: &[String]) -> bool {
    lines.iter().any(|l| {
        matches!(
            crate::parser::parse_coin_anchor_crop(l.trim()),
            crate::parser::CoinReading::Rate(_)
        )
    })
}

/// OCR the coin/min row, trying progressively looser sources. Each source is
/// attempted binarized (best for the tiny rate text) then plain; the first
/// source that yields a parseable rate wins, otherwise the first non-empty
/// result is returned so the classifier can still inspect it.
fn ocr_coin_lines(frame: &RgbaImage) -> Vec<String> {
    ocr_coin_lines_cancellable(frame, &|| true)
}

fn ocr_coin_lines_cancellable<F: Fn() -> bool>(frame: &RgbaImage, should_continue: &F) -> Vec<String> {
    let crop = coin_ocr_crop(frame).unwrap_or_else(|| coin_fallback_ocr_crop(frame));
    let wide = crop_frac(
        frame,
        FracRect {
            x: 0.05,
            y: 0.012,
            w: 0.55,
            h: 0.055,
        },
    );

    let mut first_nonempty = Vec::new();
    for (src, binary) in [(&crop, true), (&crop, false), (&wide, true), (&wide, false)] {
        if !should_continue() {
            return first_nonempty;
        }
        let lines = if binary {
            ocr::ocr_lines_binarized(src).unwrap_or_default()
        } else {
            ocr::ocr_lines(src).unwrap_or_default()
        };
        if first_nonempty.is_empty() && !lines.is_empty() {
            first_nonempty = lines.clone();
        }
        if lines_have_coin_rate(&lines) {
            return lines;
        }
    }
    first_nonempty
}

fn ocr_tier_wave_lines(frame: &RgbaImage) -> Vec<String> {
    // Fixed layout crop is fast to compute and matches the reference HUD column.
    let (dx, dy) = tier_wave_default_origin(frame);
    let layout_lines =
        ocr::ocr_lines(&tier_wave_ocr_region(frame, dx, dy)).unwrap_or_default();
    if lines_contain_tier_or_wave(&layout_lines) {
        return layout_lines;
    }

    let Some(panel) = load_fixture("Wave_and_Tier.png") else {
        return layout_lines;
    };

    let scaled_panel = resize_sy(&panel, frame);
    let panel_gray = anchor::to_gray(&scaled_panel);
    let (search, band_left, band_top) = tier_wave_search_band(frame);

    // Heart icon (bottom-right of panel) is a distinctive template.
    let heart = crop_frac(
        &panel,
        FracRect {
            x: 0.72,
            y: 0.48,
            w: 0.22,
            h: 0.45,
        },
    );
    let heart_gray = anchor::to_gray(&resize_sy(&heart, frame));
    let mut candidates: Vec<(TemplateMatch, u32, u32)> = Vec::new();
    if let Some(m) = locate_template(&search, &panel_gray) {
        if m.confidence >= 0.55 {
            let px = band_left + m.x;
            let py = band_top + m.y;
            candidates.push((m, px, py));
        }
    }
    if let Some(m) = locate_template(&search, &heart_gray) {
        if m.confidence >= 0.60 {
            let sy = scale_sy(frame);
            let ox = (panel.width() as f32 * 0.72 * sy).round() as u32;
            let oy = (panel.height() as f32 * 0.48 * sy).round() as u32;
            let px = band_left + m.x.saturating_sub(ox);
            let py = band_top + m.y.saturating_sub(oy);
            candidates.push((m, px, py));
        }
    }
    candidates.sort_by(|a, b| b.0.confidence.partial_cmp(&a.0.confidence).unwrap());

    for (_m, panel_x, panel_y) in &candidates {
        let crop = tier_wave_ocr_region(frame, *panel_x, *panel_y);
        let lines = ocr::ocr_lines(&crop).unwrap_or_default();
        if lines_contain_tier_and_wave(&lines) {
            return lines;
        }
    }

    layout_lines
}

fn lines_contain_tier_or_wave(lines: &[String]) -> bool {
    lines.iter().any(|l| {
        let lower = l.to_lowercase();
        lower.contains("tier")
            || lower.contains("wave")
            || lower.contains("lave")
            || lower.contains("ive ")
            || lower.contains("7ier")
            || crate::classify::find_tier_panel(l).is_some()
            || crate::classify::find_wave_panel(l).is_some()
    })
}

fn lines_contain_tier_and_wave(lines: &[String]) -> bool {
    let mut tier = false;
    let mut wave = false;
    for l in lines {
        if crate::classify::find_tier_panel(l).is_some() {
            tier = true;
        }
        if crate::classify::find_wave_panel(l).is_some() {
            wave = true;
        }
    }
    tier && wave
}

/// OCR all tracked fields via anchor template matching.
pub fn ocr_all_fields(frame: &RgbaImage) -> FieldOcr {
    ocr_all_fields_cancellable(frame, &|| true)
}

/// Like [`ocr_all_fields`] but skips remaining work when `should_continue` is false
/// (used so Stop returns promptly instead of waiting for every OCR pass).
pub fn ocr_all_fields_cancellable<F: Fn() -> bool>(frame: &RgbaImage, should_continue: &F) -> FieldOcr {
    let coin_lines = ocr_coin_lines_cancellable(frame, should_continue);
    if !should_continue() {
        return FieldOcr {
            coin_lines,
            tier_wave_lines: Vec::new(),
            mode_lines: Vec::new(),
        };
    }
    let tier_wave_lines = ocr_tier_wave_lines(frame);
    if !should_continue() {
        return FieldOcr {
            coin_lines,
            tier_wave_lines,
            mode_lines: Vec::new(),
        };
    }
    let mode_lines = ocr::ocr_lines(&mode_crop(frame)).unwrap_or_default();
    FieldOcr {
        coin_lines,
        tier_wave_lines,
        mode_lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify;
    use crate::parser::CoinReading;
    use crate::state_machine::GameMode;

    fn full_game_fixture() -> RgbaImage {
        load_fixture("expected_state_full_game.png").expect("fixture")
    }

    #[test]
    #[cfg(windows)]
    #[ignore]
    fn debug_anchor_matches_on_full_game() {
        let img = full_game_fixture();
        let location = load_fixture("coin_per_minute_location.png").unwrap();
        let (coin_search, _, _) = top_resource_search_band(&img);
        eprintln!("coin search band: {}x{}", coin_search.width(), coin_search.height());
        for (idx, template) in coin_bar_icon_templates(&location, &img) {
            if let Some(m) = locate_template(&coin_search, &template) {
                eprintln!("coin icon {idx}: conf={:.3} at ({}, {})", m.confidence, m.x, m.y);
            } else {
                eprintln!("coin icon {idx}: no match");
            }
        }
        let full = resize_sy(&location, &img);
        if let Some(m) = locate_template(&coin_search, &anchor::to_gray(&full)) {
            eprintln!("coin full bar: conf={:.3} at ({}, {})", m.confidence, m.x, m.y);
        }
        let panel = load_fixture("Wave_and_Tier.png").unwrap();
        let (tw_search, _, _) = tier_wave_search_band(&img);
        eprintln!("tier search band: {}x{}", tw_search.width(), tw_search.height());
        let scaled = resize_sy(&panel, &img);
        if let Some(m) = locate_template(&tw_search, &anchor::to_gray(&scaled)) {
            eprintln!("tier panel: conf={:.3} at ({}, {})", m.confidence, m.x, m.y);
        }
        let sword = crop_frac(
            &panel,
            FracRect { x: 0.72, y: 0.05, w: 0.22, h: 0.42 },
        );
        let sword_gray = anchor::to_gray(&resize_sy(&sword, &img));
        if let Some(m) = locate_template(&tw_search, &sword_gray) {
            eprintln!("tier sword: conf={:.3} at ({}, {})", m.confidence, m.x, m.y);
        }
        let (search, bl, bt) = tier_wave_search_band(&img);
        let scaled_panel = resize_sy(&panel, &img);
        let panel_gray = anchor::to_gray(&scaled_panel);
        if let Some(m) = locate_template(&search, &panel_gray) {
            let crop = tier_wave_ocr_region(&img, bl + m.x, bt + m.y);
            eprintln!("panel candidate OCR: {:?}", ocr::ocr_lines(&crop));
        }
        let lines = ocr_tier_wave_lines(&img);
        eprintln!("tier OCR lines: {lines:?}");
        eprintln!("band fallback OCR: {:?}", ocr::ocr_lines(&search));
        let (dx, dy) = tier_wave_default_origin(&img);
        eprintln!(
            "layout fallback OCR: {:?}",
            ocr::ocr_lines(&tier_wave_ocr_region(&img, dx, dy))
        );
        let coin = ocr_coin_lines(&img);
        eprintln!("coin OCR lines: {coin:?}");
        let manual = capture::crop_region(&img, 560, 780, 500, 400);
        eprintln!("old layout crop OCR: {:?}", ocr::ocr_lines(&manual));
        let matched = capture::crop_region(&img, 489, 752, 442, 139);
        eprintln!("matched crop OCR: {:?}", ocr::ocr_lines(&matched));
        let wide = capture::crop_region(&img, 600, 240, 480, 1680);
        eprintln!("wide crop OCR: {:?}", ocr::ocr_lines(&wide));
        eprintln!("fixture anchor OCR: {:?}", ocr::ocr_lines(&panel));
    }

    #[test]
    #[cfg(windows)]
    fn anchor_locates_coin_on_full_game_fixture() {
        let img = full_game_fixture();
        let lines = ocr_coin_lines(&img);
        assert!(!lines.is_empty(), "coin anchor OCR empty: {lines:?}");
    }

    #[test]
    #[cfg(windows)]
    fn anchor_locates_tier_wave_on_full_game_fixture() {
        let img = full_game_fixture();
        let lines = ocr_tier_wave_lines(&img);
        assert!(
            lines_contain_tier_or_wave(&lines),
            "tier/wave anchor OCR: {lines:?}"
        );
    }

    #[test]
    #[cfg(windows)]
    fn field_ocr_classifies_full_game_fixture() {
        let img = full_game_fixture();
        let fields = ocr_all_fields(&img);
        let input = classify::classify(
            &fields.mode_lines,
            Some(&fields.coin_lines),
            Some(&fields.tier_wave_lines),
        );
        assert_eq!(input.tier, Some(12), "tier_wave={:?}", fields.tier_wave_lines);
        assert_eq!(input.wave, Some(4571), "tier_wave={:?}", fields.tier_wave_lines);
        assert!(
            matches!(input.coin, CoinReading::Rate(_)),
            "coin_lines={:?} coin={:?}",
            fields.coin_lines,
            input.coin
        );
        assert_eq!(input.mode, GameMode::Normal);
    }

    #[test]
    #[cfg(windows)]
    fn tier_wave_on_scaled_capture() {
        let img = full_game_fixture();
        let scaled = image::imageops::resize(
            &img,
            978,
            2084,
            image::imageops::FilterType::Triangle,
        );
        let fields = ocr_all_fields(&scaled);
        let input = classify::classify(
            &fields.mode_lines,
            Some(&fields.coin_lines),
            Some(&fields.tier_wave_lines),
        );
        assert!(
            input.tier.is_some() || input.wave.is_some(),
            "scaled OCR: coin={:?} tier_wave={:?} tier={:?} wave={:?}",
            fields.coin_lines,
            fields.tier_wave_lines,
            input.tier,
            input.wave
        );
    }
}
