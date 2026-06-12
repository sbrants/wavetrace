//! Window enumeration and capture via xcap.

use image::RgbaImage;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WindowInfo {
    pub title: String,
    pub app_name: String,
}

#[derive(Debug, Clone)]
pub struct CaptureProbe {
    pub width: u32,
    pub height: u32,
    pub method: &'static str,
}

pub fn list_windows() -> Vec<WindowInfo> {
    let Ok(windows) = xcap::Window::all() else {
        return Vec::new();
    };
    windows
        .iter()
        .filter_map(|w| {
            let title = w.title().ok()?;
            if title.trim().is_empty() {
                return None;
            }
            Some(WindowInfo {
                title,
                app_name: w.app_name().unwrap_or_default(),
            })
        })
        .collect()
}

/// Minimum window area (pixels²) for a plausible game/emulator capture.
const MIN_CAPTURE_AREA: u32 = 200_000;

fn is_our_app_window(title: &str, app_name: &str) -> bool {
    let t = title.to_lowercase();
    let a = app_name.to_lowercase();
    a.contains("towerrun") || t.contains("towerrun performance")
}

fn is_browser_window(app_name: &str, title: &str) -> bool {
    let a = app_name.to_lowercase();
    let t = title.to_lowercase();
    a.contains("chrome")
        || a.contains("firefox")
        || a.contains("msedge")
        || a.contains("brave")
        || t.contains("google chrome")
}

fn is_emulator_window(app_name: &str, title: &str) -> bool {
    let a = app_name.to_lowercase();
    let t = title.to_lowercase();
    a.contains("nox")
        || a.contains("bluestacks")
        || a.contains("ldplayer")
        || a.contains("mumu")
        || t.contains("noxplayer")
        || t.contains("bluestacks")
}

/// Rank candidate windows. Emulators win over browsers even when the browser tab
/// title also contains the game name and captures at a larger pixel area.
fn window_capture_score(img: &RgbaImage, app_name: &str, title: &str) -> u32 {
    let area = img.width().saturating_mul(img.height());
    if is_browser_window(app_name, title) {
        return area / 20;
    }
    if is_emulator_window(app_name, title) {
        return area.saturating_mul(4);
    }
    area
}

fn capture_window_image(w: &xcap::Window) -> Option<(RgbaImage, &'static str)> {
    if let Ok(img) = w.capture_image() {
        return Some((img, "window"));
    }
    capture_window_via_monitor(w).map(|img| (img, "monitor_crop"))
}

/// Crop the window bounds from its current monitor when direct window capture fails
/// (common with GPU-accelerated emulators under GDI).
fn capture_window_via_monitor(w: &xcap::Window) -> Option<RgbaImage> {
    let wx = w.x().ok()?;
    let wy = w.y().ok()?;
    let ww = w.width().ok()?;
    let wh = w.height().ok()?;
    let monitor = w.current_monitor().ok()?;
    let mon_img = monitor.capture_image().ok()?;
    let mx = monitor.x().ok()? as i32;
    let my = monitor.y().ok()? as i32;
    let rel_x = (wx - mx).max(0) as u32;
    let rel_y = (wy - my).max(0) as u32;
    let w = ww.min(mon_img.width().saturating_sub(rel_x)).max(1);
    let h = wh.min(mon_img.height().saturating_sub(rel_y)).max(1);
    Some(crop_region(&mon_img, rel_x, rel_y, w, h))
}

/// Diagnostic capture for a single window title (exact match, not substring search).
pub fn probe_window(title: &str) -> Option<CaptureProbe> {
    let windows = xcap::Window::all().ok()?;
    for w in &windows {
        if w.title().unwrap_or_default() != title {
            continue;
        }
        if w.is_minimized().unwrap_or(true) {
            return None;
        }
        let (img, method) = capture_window_image(w)?;
        return Some(CaptureProbe {
            width: img.width(),
            height: img.height(),
            method,
        });
    }
    None
}

/// Capture the largest non-minimized window whose title contains `title_substring`
/// (case-insensitive). Prefers emulator-sized windows over narrow title-bar matches.
pub fn capture_by_title(title_substring: &str) -> Option<RgbaImage> {
    let needle = title_substring.to_lowercase();
    let windows = xcap::Window::all().ok()?;
    let mut best: Option<(u32, RgbaImage, &'static str)> = None;
    for w in &windows {
        let title = w.title().unwrap_or_default();
        if !title.to_lowercase().contains(&needle) {
            continue;
        }
        if w.is_minimized().unwrap_or(true) {
            continue;
        }
        let app = w.app_name().unwrap_or_default();
        if is_our_app_window(&title, &app) {
            continue;
        }
        let Some((img, _method)) = capture_window_image(w) else {
            continue;
        };
        let area = img.width().saturating_mul(img.height());
        if area < MIN_CAPTURE_AREA {
            continue;
        }
        let score = window_capture_score(&img, &app, &title);
        let replace = match &best {
            None => true,
            Some((best_score, _, _)) => score > *best_score,
        };
        if replace {
            best = Some((score, img, _method));
        }
    }
    best.map(|(_, img, _)| img)
}

/// Crop a sub-region out of a captured frame. Coordinates are clamped to the
/// image bounds so out-of-range values can't panic.
pub fn crop_region(img: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
    let x = x.min(img.width().saturating_sub(1));
    let y = y.min(img.height().saturating_sub(1));
    let w = w.min(img.width() - x).max(1);
    let h = h.min(img.height() - y).max(1);
    image::imageops::crop_imm(img, x, y, w, h).to_image()
}
