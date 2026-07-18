//! Window enumeration and capture via xcap.

use base64::Engine;
use std::sync::Mutex;

use image::RgbaImage;
use serde::Serialize;

use crate::settings::TargetWindow;

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

/// Whether the OS grants this app the ability to read window titles and capture
/// window pixels. macOS gates both behind the Screen Recording (TCC) permission;
/// Windows and Linux don't, so they report `NotRequired`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenCaptureAccess {
    Granted,
    Denied,
    NotRequired,
}

#[cfg(target_os = "macos")]
mod macos_screen_recording {
    // CGPreflightScreenCaptureAccess / CGRequestScreenCaptureAccess live in the
    // CoreGraphics framework and are available since macOS 10.15 (our minimum
    // deployment target). Without Screen Recording access, CGWindowListCopyWindowInfo
    // returns windows with empty titles, so the window picker comes up empty.
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    pub fn has_access() -> bool {
        // SAFETY: no-argument CoreGraphics calls with no pointer arguments.
        unsafe { CGPreflightScreenCaptureAccess() }
    }

    /// Shows the system prompt the first time access is undetermined and returns
    /// the current grant state. When already denied it returns false without a
    /// prompt (the user must re-enable it in System Settings).
    pub fn request_access() -> bool {
        // SAFETY: no-argument CoreGraphics calls with no pointer arguments.
        unsafe { CGRequestScreenCaptureAccess() }
    }
}

/// Current Screen Recording permission state (no prompt).
pub fn screen_capture_access() -> ScreenCaptureAccess {
    #[cfg(target_os = "macos")]
    {
        if macos_screen_recording::has_access() {
            ScreenCaptureAccess::Granted
        } else {
            ScreenCaptureAccess::Denied
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        ScreenCaptureAccess::NotRequired
    }
}

/// Request Screen Recording permission, prompting on first launch (macOS only).
pub fn request_screen_capture_access() -> ScreenCaptureAccess {
    #[cfg(target_os = "macos")]
    {
        if macos_screen_recording::request_access() {
            ScreenCaptureAccess::Granted
        } else {
            ScreenCaptureAccess::Denied
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        ScreenCaptureAccess::NotRequired
    }
}

/// Open the macOS Screen Recording settings pane (no-op elsewhere).
pub fn open_screen_recording_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

/// Cached target window id — avoids re-scoring every window each poll.
static WINDOW_CACHE: Mutex<Option<(String, u32)>> = Mutex::new(None);

pub fn clear_window_cache() {
    if let Ok(mut guard) = WINDOW_CACHE.lock() {
        *guard = None;
    }
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
    a.contains("wavetrace")
        || t.contains("wavetrace")
        || a.contains("wavewatch")
        || t.contains("wavewatch")
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
        || a.contains("parallels")
        || a.contains("qemu")
        || a.contains("android")
        || t.contains("parallels")
        || t.contains("android emulator")
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
    let mx = monitor.x().ok()?;
    let my = monitor.y().ok()?;
    let rel_x = (wx - mx).max(0) as u32;
    let rel_y = (wy - my).max(0) as u32;
    let w = ww.min(mon_img.width().saturating_sub(rel_x)).max(1);
    let h = wh.min(mon_img.height().saturating_sub(rel_y)).max(1);
    Some(crop_region(&mon_img, rel_x, rel_y, w, h))
}

fn try_capture_window(w: &xcap::Window) -> Option<RgbaImage> {
    if w.is_minimized().unwrap_or(true) {
        return None;
    }
    capture_window_image(w).map(|(img, _)| img)
}

fn cache_window_id(title_substring: &str, window_id: u32) {
    if let Ok(mut guard) = WINDOW_CACHE.lock() {
        *guard = Some((title_substring.to_string(), window_id));
    }
}

fn capture_from_cached_id(windows: &[xcap::Window], title_substring: &str) -> Option<RgbaImage> {
    let cached_id = WINDOW_CACHE.lock().ok().and_then(|g| {
        g.as_ref()
            .filter(|(t, _)| t == title_substring)
            .map(|(_, id)| *id)
    })?;

    let needle = title_substring.to_lowercase();
    for w in windows {
        if w.id().ok() != Some(cached_id) {
            continue;
        }
        // The OS can reuse a window id (HWND on Windows) for a different window
        // after the original closes — e.g. when the emulator is restarted. Confirm
        // the cached id still points at a matching game window before trusting it,
        // otherwise we'd silently capture (and OCR) the wrong window.
        let title = w.title().unwrap_or_default();
        let app = w.app_name().unwrap_or_default();
        if !title.to_lowercase().contains(&needle) || is_our_app_window(&title, &app) {
            break;
        }
        if let Some(img) = try_capture_window(w) {
            let area = img.width().saturating_mul(img.height());
            if area >= MIN_CAPTURE_AREA {
                return Some(img);
            }
        }
        break;
    }
    clear_window_cache();
    None
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

/// Capture the configured target window. User-picked windows are matched by exact
/// title (and app name when saved); auto-detected targets use substring heuristics.
pub fn capture_target(target: &TargetWindow) -> Option<RgbaImage> {
    if target.user_selected {
        capture_by_exact_title(&target.title_substring, &target.process_name)
    } else {
        capture_by_title(&target.title_substring)
    }
}

/// Whether a live window matches a user-selected target (title equality, optional app).
fn window_matches_exact_target(
    title: &str,
    app_name: &str,
    target_title: &str,
    target_app: &str,
) -> bool {
    if title.to_lowercase() != target_title.to_lowercase() {
        return false;
    }
    if is_our_app_window(title, app_name) {
        return false;
    }
    let filter = target_app.trim();
    if filter.is_empty() {
        return true;
    }
    let a = app_name.to_lowercase();
    let f = filter.to_lowercase();
    a == f || a.contains(&f) || f.contains(&a)
}

fn capture_from_cached_id_exact(
    windows: &[xcap::Window],
    target_title: &str,
    target_app: &str,
) -> Option<RgbaImage> {
    let cache_key = format!("exact:{target_title}\0{target_app}");
    let cached_id = WINDOW_CACHE.lock().ok().and_then(|g| {
        g.as_ref()
            .filter(|(k, _)| k == &cache_key)
            .map(|(_, id)| *id)
    })?;

    for w in windows {
        if w.id().ok() != Some(cached_id) {
            continue;
        }
        let title = w.title().unwrap_or_default();
        let app = w.app_name().unwrap_or_default();
        if !window_matches_exact_target(&title, &app, target_title, target_app) {
            break;
        }
        if let Some(img) = try_capture_window(w) {
            return Some(img);
        }
        break;
    }
    clear_window_cache();
    None
}

/// Capture the non-minimized window whose title equals `title` (case-insensitive).
/// When `app_name` is set, the window's app name must also match.
pub fn capture_by_exact_title(title: &str, app_name: &str) -> Option<RgbaImage> {
    let windows = xcap::Window::all().ok()?;
    let cache_key = format!("exact:{title}\0{app_name}");

    if let Some(img) = capture_from_cached_id_exact(&windows, title, app_name) {
        return Some(img);
    }

    for w in &windows {
        let wtitle = w.title().unwrap_or_default();
        let app = w.app_name().unwrap_or_default();
        if !window_matches_exact_target(&wtitle, &app, title, app_name) {
            continue;
        }
        if w.is_minimized().unwrap_or(true) {
            continue;
        }
        if let Some(img) = try_capture_window(w) {
            if let Some(id) = w.id().ok() {
                if let Ok(mut guard) = WINDOW_CACHE.lock() {
                    *guard = Some((cache_key, id));
                }
            }
            return Some(img);
        }
    }
    None
}

/// Capture the largest non-minimized window whose title contains `title_substring`
/// (case-insensitive). Prefers emulator-sized windows over narrow title-bar matches.
/// Retains the matched window id between calls for faster subsequent captures.
pub fn capture_by_title(title_substring: &str) -> Option<RgbaImage> {
    let needle = title_substring.to_lowercase();
    let windows = xcap::Window::all().ok()?;

    if let Some(img) = capture_from_cached_id(&windows, title_substring) {
        return Some(img);
    }

    let mut best: Option<(u32, RgbaImage, u32)> = None;
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
        let window_id = w.id().ok().unwrap_or(0);
        let replace = match &best {
            None => true,
            Some((best_score, _, _)) => score > *best_score,
        };
        if replace {
            best = Some((score, img, window_id));
        }
    }

    if let Some((_, img, window_id)) = best {
        if window_id != 0 {
            cache_window_id(title_substring, window_id);
        }
        Some(img)
    } else {
        None
    }
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

/// Capture the WaveTrace application window (for debug/support bundles).
pub fn capture_own_app_window() -> Result<RgbaImage, String> {
    let windows = xcap::Window::all().map_err(|e| e.to_string())?;
    let mut monitor_fallback: Option<RgbaImage> = None;
    for w in &windows {
        let title = w.title().unwrap_or_default();
        let app = w.app_name().unwrap_or_default();
        if !is_our_app_window(&title, &app) {
            continue;
        }
        if let Some(img) = try_capture_window(w) {
            return Ok(img);
        }
        if monitor_fallback.is_none() {
            monitor_fallback = capture_window_via_monitor(w);
        }
    }
    monitor_fallback.ok_or_else(|| {
        "Could not capture the WaveTrace window. Make sure the app window is visible.".into()
    })
}

pub fn encode_png_base64(img: &RgbaImage) -> Result<String, String> {
    use std::io::Cursor;

    let mut buf = Vec::new();
    image::DynamicImage::ImageRgba8(img.clone())
        .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| format!("png encode failed: {e}"))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(buf))
}

#[cfg(test)]
mod tests {
    use super::window_matches_exact_target;

    #[test]
    fn exact_target_requires_title_equality() {
        assert!(window_matches_exact_target(
            "NoxPlayer",
            "Nox",
            "NoxPlayer",
            "Nox"
        ));
        assert!(!window_matches_exact_target(
            "The Tower - Google Chrome",
            "Google Chrome",
            "NoxPlayer",
            "Nox"
        ));
    }

    #[test]
    fn exact_target_skips_our_app() {
        assert!(!window_matches_exact_target(
            "WaveTrace",
            "WaveTrace",
            "WaveTrace",
            ""
        ));
    }
}
