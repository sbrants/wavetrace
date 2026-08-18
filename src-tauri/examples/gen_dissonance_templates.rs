//! Regenerate HUD icon templates from the four dissonance reference screenshots.
//! Run: `cargo run --release --example gen_dissonance_templates`
use std::path::PathBuf;

use image::{imageops, RgbaImage};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
}

fn crop_norm(img: &RgbaImage, x: f32, y: f32, w: f32, h: f32) -> RgbaImage {
    let fw = img.width() as f32;
    let fh = img.height() as f32;
    let x0 = (x * fw).round() as u32;
    let y0 = (y * fh).round() as u32;
    let w_px = ((w * fw).round() as u32).max(1).min(img.width().saturating_sub(x0));
    let h_px = ((h * fh).round() as u32).max(1).min(img.height().saturating_sub(y0));
    imageops::crop_imm(img, x0, y0, w_px, h_px).to_image()
}

fn is_dissonance_hud_icon_pixel(r: u8, g: u8, b: u8) -> bool {
    (88..=128).contains(&r)
        && (30..=70).contains(&g)
        && (120..=175).contains(&b)
        && r > g + 35
}

fn find_icon_bbox(region: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let y_min = if region.height() < 120 {
        0
    } else {
        (region.height() as f32 * 0.40) as u32
    };
    let x_min = (region.width() as f32 * 0.20) as u32;
    let x_max = (region.width() as f32 * 0.85) as u32;
    let width = region.width();
    let height = region.height();
    let mut visited = vec![false; (width * height) as usize];
    let mut best: Option<(u32, u32, u32, u32, i64)> = None;

    for y in y_min..height {
        for x in x_min..x_max.min(width) {
            let idx = (y * width + x) as usize;
            if visited[idx] {
                continue;
            }
            let p = region.get_pixel(x, y);
            if !is_dissonance_hud_icon_pixel(p[0], p[1], p[2]) {
                continue;
            }

            let mut stack = vec![(x, y)];
            visited[idx] = true;
            let mut min_x = x;
            let mut max_x = x;
            let mut min_y = y;
            let mut max_y = y;
            let mut count = 0u32;

            while let Some((cx, cy)) = stack.pop() {
                count += 1;
                min_x = min_x.min(cx);
                max_x = max_x.max(cx);
                min_y = min_y.min(cy);
                max_y = max_y.max(cy);
                for (nx, ny) in [
                    (cx.wrapping_sub(1), cy),
                    (cx + 1, cy),
                    (cx, cy.wrapping_sub(1)),
                    (cx, cy + 1),
                ] {
                    if nx < x_min || nx >= x_max.min(width) || ny < y_min || ny >= height {
                        continue;
                    }
                    let nidx = (ny * width + nx) as usize;
                    if visited[nidx] {
                        continue;
                    }
                    let np = region.get_pixel(nx, ny);
                    if !is_dissonance_hud_icon_pixel(np[0], np[1], np[2]) {
                        continue;
                    }
                    visited[nidx] = true;
                    stack.push((nx, ny));
                }
            }

            if !(80..=2500).contains(&count) {
                continue;
            }
            let w = max_x - min_x + 1;
            let h = max_y - min_y + 1;
            if w < 18 || h < 18 || w > 70 || h > 70 {
                continue;
            }
            let aspect = (w as i64 - h as i64).abs();
            let size_err = (w as i64 - 40).abs() + (h as i64 - 40).abs();
            let score = -(aspect * 10 + size_err);
            if best.as_ref().map(|(_, _, _, _, s)| score > *s).unwrap_or(true) {
                best = Some((min_x, min_y, w, h, score));
            }
        }
    }

    best.map(|(x, y, w, h, _)| (x, y, w, h))
}

fn main() {
    let files = [
        ("attack dissonance.png", "attack"),
        ("defense dissonance.png", "defense"),
        ("utility dissonance.png", "utility"),
        ("ultimate weapons dissonance.png", "ultimate_weapons"),
    ];
    let out_dir = fixtures_dir().join("dissonance_icons");
    std::fs::create_dir_all(&out_dir).ok();
    for (file, name) in files {
        let path = fixtures_dir().join(file);
        let img = image::open(&path).unwrap_or_else(|e| panic!("{file}: {e}")).to_rgba8();
        let region = crop_norm(&img, 0.35, 0.50, 0.65, 0.22);
        let (x, y, w, h) = find_icon_bbox(&region).unwrap_or_else(|| panic!("{name}: icon not found"));
        let pad = 4u32;
        let crop = imageops::crop_imm(
            &region,
            x.saturating_sub(pad),
            y.saturating_sub(pad),
            (w + pad * 2).min(region.width().saturating_sub(x)),
            (h + pad * 2).min(region.height().saturating_sub(y)),
        )
        .to_image();
        let out = out_dir.join(format!("{name}.png"));
        crop.save(&out).expect("save template");
        println!("wrote {} ({}x{})", out.display(), crop.width(), crop.height());
    }
}
