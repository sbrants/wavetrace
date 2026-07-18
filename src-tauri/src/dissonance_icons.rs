//! Detect dissonance run category from the purple workshop icons beside Tier/Wave.

use std::sync::OnceLock;

use image::{imageops, RgbaImage};

use crate::state_machine::DissonanceKind;

const SEARCH_X: f32 = 0.35;
const SEARCH_Y: f32 = 0.50;
const SEARCH_W: f32 = 0.65;
const SEARCH_H: f32 = 0.22;
const MATCH_THRESHOLD: f64 = 0.62;
const MATCH_MARGIN: f64 = 0.03;

struct IconTemplate {
    kind: DissonanceKind,
    rgba: RgbaImage,
}

static TEMPLATES: OnceLock<Vec<IconTemplate>> = OnceLock::new();

fn templates() -> &'static [IconTemplate] {
    TEMPLATES.get_or_init(load_templates)
}

fn load_templates() -> Vec<IconTemplate> {
    let mut templates = Vec::new();
    let specs: &[(DissonanceKind, &[u8])] = &[
        (
            DissonanceKind::Attack,
            include_bytes!("../../fixtures/dissonance_icons/attack.png"),
        ),
        (
            DissonanceKind::Defense,
            include_bytes!("../../fixtures/dissonance_icons/defense.png"),
        ),
        (
            DissonanceKind::Utility,
            include_bytes!("../../fixtures/dissonance_icons/utility.png"),
        ),
        (
            DissonanceKind::UltimateWeapons,
            include_bytes!("../../fixtures/dissonance_icons/ultimate_weapons.png"),
        ),
    ];
    for (kind, bytes) in specs {
        if let Ok(img) = image::load_from_memory(bytes) {
            templates.push(IconTemplate {
                kind: *kind,
                rgba: img.to_rgba8(),
            });
        }
    }
    templates
}

/// Best-effort icon match in the tier/wave HUD region.
pub fn detect(frame: &RgbaImage) -> Option<DissonanceKind> {
    let templates = templates();
    if templates.is_empty() {
        return None;
    }

    if frame.height() < 200 {
        if let Some(crop) = find_hud_icon_crop(frame) {
            if let Some(kind) = detect_in_image(&crop, templates, true) {
                return Some(kind);
            }
        }
        return detect_in_image(frame, templates, false);
    }

    let region = crop_norm(frame, SEARCH_X, SEARCH_Y, SEARCH_W, SEARCH_H);
    if let Some(crop) = find_hud_icon_crop(&region) {
        if let Some(kind) = detect_in_image(&crop, templates, true) {
            return Some(kind);
        }
    }
    detect_in_image(&region, templates, false)
}

/// Match against a pre-cropped region (used by tests and diagnostics).
pub fn detect_in_region(region: &RgbaImage) -> Option<DissonanceKind> {
    detect_in_image(region, templates(), false)
}

/// Purple circle beside Tier/Wave in the in-run HUD.
fn is_dissonance_hud_icon_pixel(r: u8, g: u8, b: u8) -> bool {
    r >= 88 && r <= 128 && g >= 30 && g <= 70 && b >= 120 && b <= 175 && r > g + 35
}

fn find_hud_icon_crop(region: &RgbaImage) -> Option<RgbaImage> {
    let (x, y, w, h) = find_icon_bbox(region)?;
    let pad = 4u32;
    Some(
        imageops::crop_imm(
            region,
            x.saturating_sub(pad),
            y.saturating_sub(pad),
            (w + pad * 2).min(region.width().saturating_sub(x)),
            (h + pad * 2).min(region.height().saturating_sub(y)),
        )
        .to_image(),
    )
}

fn find_icon_bbox(region: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let y_min = if region.height() < 120 {
        0
    } else {
        (region.height() as f32 * 0.40) as u32
    };
    let x_min = (region.width() as f32 * 0.20) as u32;
    let x_max = (region.width() as f32 * 0.85) as u32;
    let mut min_x = region.width();
    let mut max_x = 0u32;
    let mut min_y = region.height();
    let mut max_y = 0u32;
    let mut count = 0u32;
    for y in y_min..region.height() {
        for x in x_min..x_max.min(region.width()) {
            let p = region.get_pixel(x, y);
            if is_dissonance_hud_icon_pixel(p[0], p[1], p[2]) {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
                count += 1;
            }
        }
    }
    if count < 80 || count > 2500 {
        return None;
    }
    let w = max_x - min_x + 1;
    let h = max_y - min_y + 1;
    if w < 18 || h < 18 || w > 70 || h > 70 {
        return None;
    }
    Some((min_x, min_y, w, h))
}

fn detect_in_image(
    region: &RgbaImage,
    templates: &[IconTemplate],
    aligned: bool,
) -> Option<DissonanceKind> {
    if templates.is_empty() {
        return None;
    }
    let mut best: Option<(DissonanceKind, f64)> = None;
    for template in templates {
        let score = if aligned {
            aligned_template_score(region, &template.rgba)
        } else {
            best_template_score(region, &template.rgba)
        };
        if score >= MATCH_THRESHOLD {
            if best.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
                best = Some((template.kind, score));
            }
        }
    }
    if let Some((kind, top)) = best {
        let mut runner_up = 0.0f64;
        for template in templates {
            if template.kind == kind {
                continue;
            }
            let score = if aligned {
                aligned_template_score(region, &template.rgba)
            } else {
                best_template_score(region, &template.rgba)
            };
            runner_up = runner_up.max(score);
        }
        if top - runner_up >= MATCH_MARGIN {
            return Some(kind);
        }
    }
    None
}

fn crop_norm(img: &RgbaImage, x: f32, y: f32, w: f32, h: f32) -> RgbaImage {
    let fw = img.width() as f32;
    let fh = img.height() as f32;
    let x0 = (x * fw).round() as u32;
    let y0 = (y * fh).round() as u32;
    let w_px = (w * fw).round() as u32;
    let h_px = (h * fh).round() as u32;
    let w_px = w_px.max(1).min(img.width().saturating_sub(x0));
    let h_px = h_px.max(1).min(img.height().saturating_sub(y0));
    imageops::crop_imm(img, x0, y0, w_px, h_px).to_image()
}

fn aligned_template_score(region: &RgbaImage, template: &RgbaImage) -> f64 {
    if region.width() == template.width() && region.height() == template.height() {
        return normalized_match(region, template, 0, 0);
    }
    let resized = imageops::resize(
        region,
        template.width(),
        template.height(),
        imageops::FilterType::Triangle,
    );
    normalized_match(&resized, template, 0, 0)
}

fn best_template_score(region: &RgbaImage, template: &RgbaImage) -> f64 {
    let tw = template.width();
    let th = template.height();
    if tw == 0 || th == 0 || region.width() < tw || region.height() < th {
        return 0.0;
    }
    let mut best = 0.0f64;
    let step_x = (tw / 4).max(1);
    let step_y = (th / 4).max(1);
    let max_x = region.width() - tw;
    let max_y = region.height() - th;
    let mut y = 0;
    while y <= max_y {
        let mut x = 0;
        while x <= max_x {
            let score = normalized_match(region, template, x, y);
            if score > best {
                best = score;
            }
            x += step_x;
        }
        y += step_y;
    }
    best
}

fn normalized_match(region: &RgbaImage, template: &RgbaImage, ox: u32, oy: u32) -> f64 {
    let tw = template.width();
    let th = template.height();
    let mut sum = 0.0f64;
    let mut count = 0u32;
    for ty in 0..th {
        for tx in 0..tw {
            let t = template.get_pixel(tx, ty);
            if t[3] < 32 {
                continue;
            }
            let p = region.get_pixel(ox + tx, oy + ty);
            if p[3] < 32 {
                continue;
            }
            let dr = i32::from(p[0]) - i32::from(t[0]);
            let dg = i32::from(p[1]) - i32::from(t[1]);
            let db = i32::from(p[2]) - i32::from(t[2]);
            let dist = ((dr * dr + dg * dg + db * db) as f64).sqrt();
            sum += 1.0 - (dist / 441.67295593);
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        sum / f64::from(count)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
    }

    fn assert_fixture(file: &str, expected: DissonanceKind) {
        if templates().is_empty() {
            return;
        }
        let path = fixtures_dir().join(file);
        let img = image::open(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
        let detected = detect(&img.to_rgba8());
        assert_eq!(detected, Some(expected), "{file}");
    }

    #[test]
    fn attack_dissonance_fixture() {
        assert_fixture("attack dissonance.png", DissonanceKind::Attack);
    }

    #[test]
    fn defense_dissonance_fixture() {
        assert_fixture("defense dissonance.png", DissonanceKind::Defense);
    }

    #[test]
    fn utility_dissonance_fixture() {
        assert_fixture("utility dissonance.png", DissonanceKind::Utility);
    }

    #[test]
    fn ultimate_weapons_dissonance_fixture() {
        assert_fixture(
            "ultimate weapons dissonance.png",
            DissonanceKind::UltimateWeapons,
        );
    }
}
