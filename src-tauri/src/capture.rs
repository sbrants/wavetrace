//! Window enumeration and capture via xcap.

use base64::Engine;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use image::RgbaImage;
use serde::Serialize;

use crate::adb_save;
use crate::settings::TargetWindow;

/// Where the scanner captures frames from: a desktop window, or a phone/emulator over ADB.
#[derive(Debug, Clone)]
pub enum CaptureTarget {
    Window(TargetWindow),
    AdbPhone {
        adb: PathBuf,
        serial: String,
        #[allow(dead_code)]
        label: String,
    },
}

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

/// Whether this is a Linux Wayland session. Window listing goes through X11 there, so only
/// XWayland windows show up — native Wayland ones (e.g. ika's scrcpy console) never do.
pub fn wayland_session() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
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

/// Whether a window belongs to WaveTrace itself, so the game search never captures us.
/// Matching is deliberately narrow: a title *substring* test also matches unrelated
/// windows that merely mention the name — a File Explorer window open on the install
/// folder, a browser on the releases page — which would then be captured instead.
fn is_our_app_window(title: &str, app_name: &str) -> bool {
    let t = title.trim().to_lowercase();
    let a = app_name.to_lowercase();
    a.contains("wavetrace")
        || a.contains("wavewatch")
        || is_our_window_title(&t)
}

fn is_our_window_title(t: &str) -> bool {
    t == "wavetrace"
        || t == "wavewatch"
        || t == "wavetrace (dev)"
        || t == "wavewatch (dev)"
        || t.starts_with("wavetrace — ")
        || t.starts_with("wavetrace (dev) — ")
        || t.starts_with("wavewatch — ")
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
    capture_screen_rect(&monitor, wx, wy, ww, wh)
}

fn capture_screen_rect(
    monitor: &xcap::Monitor,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Option<RgbaImage> {
    let mon_img = monitor.capture_image().ok()?;
    let mx = monitor.x().ok()?;
    let my = monitor.y().ok()?;
    let rel_x = (x - mx).max(0) as u32;
    let rel_y = (y - my).max(0) as u32;
    let w = width.min(mon_img.width().saturating_sub(rel_x)).max(1);
    let h = height.min(mon_img.height().saturating_sub(rel_y)).max(1);
    Some(crop_region(&mon_img, rel_x, rel_y, w, h))
}

/// Capture the WaveTrace application window (for debug/support bundles).
///
/// This crops the monitor at the window's own reported rect rather than searching the
/// OS window list: that list excludes windows owned by this process, so our window is
/// never in it, and a title search for "wavetrace" instead matches unrelated windows
/// (a File Explorer tab open on the install folder, a browser on the release page).
pub fn capture_own_app_window(app: &tauri::AppHandle) -> Result<RgbaImage, String> {
    use tauri::Manager;

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "WaveTrace window not found".to_string())?;
    if window.is_minimized().unwrap_or(false) {
        return Err("The WaveTrace window is minimized.".into());
    }
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .and_then(|m| {
            let p = m.position();
            xcap::Monitor::from_point(p.x, p.y).ok()
        })
        .or_else(|| xcap::Monitor::from_point(pos.x, pos.y).ok())
        .ok_or_else(|| "Could not find the monitor showing the WaveTrace window".to_string())?;
    capture_screen_rect(&monitor, pos.x, pos.y, size.width, size.height).ok_or_else(|| {
        "Could not capture the WaveTrace window. Make sure the app window is visible.".into()
    })
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

/// Why a target capture produced no frame. Distinguishes "the window is gone from
/// the OS window list" (minimized, cloaked onto another virtual desktop, hidden by
/// the emulator, locked session) from "the window is there but the pixels wouldn't
/// come out", which need very different user-facing advice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum CaptureFailure {
    /// The OS window enumeration itself failed.
    EnumerateFailed { error: String },
    /// No enumerated window matched the configured title/app.
    NoMatchingWindow,
    /// A window matched but reports as minimized (`IsIconic`).
    Minimized,
    /// A window matched and was capturable in principle, but returned no pixels.
    CaptureFailed,
    /// The capture call stopped returning and was given up on.
    TimedOut { after_ms: u64 },
    /// No usable ADB / phone device to capture from (adb missing, or none detected).
    AdbUnavailable { error: String },
    /// A phone was reachable but the `screencap` pull or PNG decode failed.
    AdbCaptureFailed { error: String },
}

impl CaptureFailure {
    /// Short stable tag for logs and scanner status events.
    pub fn tag(&self) -> &'static str {
        match self {
            CaptureFailure::EnumerateFailed { .. } => "enumerate_failed",
            CaptureFailure::NoMatchingWindow => "no_matching_window",
            CaptureFailure::Minimized => "minimized",
            CaptureFailure::CaptureFailed => "capture_failed",
            CaptureFailure::AdbUnavailable { .. } => "adb_unavailable",
            CaptureFailure::AdbCaptureFailed { .. } => "adb_capture_failed",
            CaptureFailure::TimedOut { .. } => "timed_out",
        }
    }
}

/// Captures on a helper thread so the caller can give up waiting. The OS capture call
/// can stop returning altogether (seen with GPU-accelerated emulators), and a scanner
/// blocked inside it stops polling, logs nothing, and keeps reporting the status it last
/// emitted — the app looks like it is still scanning while nothing is recorded.
pub struct TimeboxedCapture {
    worker: Option<Worker>,
    /// Helper threads left behind by timeouts, still stuck in their capture call.
    abandoned: u32,
    last_spawn: Option<std::time::Instant>,
}

struct Worker {
    request: std::sync::mpsc::Sender<(CaptureTarget, std::time::Duration)>,
    reply: std::sync::mpsc::Receiver<Result<RgbaImage, CaptureFailure>>,
}

/// Abandoned threads to tolerate before spawning replacements slowly, so a permanently
/// stuck capture path leaks threads at a trickle instead of one per poll.
const MAX_ABANDONED_WORKERS: u32 = 3;
const SLOW_RESPAWN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

impl Default for TimeboxedCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeboxedCapture {
    pub fn new() -> Self {
        Self {
            worker: None,
            abandoned: 0,
            last_spawn: None,
        }
    }

    /// Number of helper threads abandoned mid-capture so far.
    pub fn abandoned_workers(&self) -> u32 {
        self.abandoned
    }

    pub fn capture(
        &mut self,
        target: &CaptureTarget,
        timeout: std::time::Duration,
    ) -> Result<RgbaImage, CaptureFailure> {
        if self.worker.is_none() {
            let throttled = self.abandoned >= MAX_ABANDONED_WORKERS
                && self
                    .last_spawn
                    .is_some_and(|at| at.elapsed() < SLOW_RESPAWN_INTERVAL);
            if throttled {
                return Err(CaptureFailure::TimedOut {
                    after_ms: timeout.as_millis() as u64,
                });
            }
            self.worker = Some(Worker::spawn());
            self.last_spawn = Some(std::time::Instant::now());
        }
        let worker = self.worker.as_ref().expect("worker just created");

        if worker.request.send((target.clone(), timeout)).is_err() {
            self.worker = None;
            return Err(CaptureFailure::CaptureFailed);
        }
        match worker.reply.recv_timeout(timeout) {
            Ok(result) => {
                if result.is_ok() {
                    self.abandoned = 0;
                }
                result
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // The thread is still inside the capture call. Drop it: its next send
                // fails, at which point it exits on its own if it ever returns.
                self.worker = None;
                self.abandoned += 1;
                Err(CaptureFailure::TimedOut {
                    after_ms: timeout.as_millis() as u64,
                })
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                self.worker = None;
                Err(CaptureFailure::CaptureFailed)
            }
        }
    }
}

impl Worker {
    fn spawn() -> Worker {
        let (request_tx, request_rx) = std::sync::mpsc::channel::<(CaptureTarget, std::time::Duration)>();
        let (reply_tx, reply_rx) = std::sync::mpsc::channel::<Result<RgbaImage, CaptureFailure>>();
        std::thread::Builder::new()
            .name("wavetrace-capture".into())
            .spawn(move || {
                #[cfg(windows)]
                let mut session = crate::capture_session::WindowSession::new();
                while let Ok((target, timeout)) = request_rx.recv() {
                    let result = capture_for_scanner(
                        &target,
                        timeout,
                        #[cfg(windows)]
                        &mut session,
                    );
                    if reply_tx.send(result).is_err() {
                        break;
                    }
                }
            })
            .ok();
        Worker {
            request: request_tx,
            reply: reply_rx,
        }
    }
}

/// Scanner capture: answer from the worker's persistent session when it's bound to this
/// target's window, otherwise take the regular path and, if it found and captured the
/// window, bind a session to it for the following ticks. Only the scanner's worker does
/// this — one-off captures (previews, fixtures, new-run detection) stay on the regular path.
#[cfg(windows)]
fn capture_target_with_session(
    target: &CaptureTarget,
    session: &mut crate::capture_session::WindowSession,
) -> Result<RgbaImage, CaptureFailure> {
    let CaptureTarget::Window(tw) = target else {
        return capture_target_detailed(target);
    };
    if let Some(result) = session.capture(tw) {
        return result;
    }
    let result = capture_window_detailed(tw);
    if result.is_ok() {
        if let Some(id) = cached_window_id(tw) {
            session.bind(tw, id);
        }
    }
    result
}

/// The window id the regular path matched for `target` on its last success (both the
/// exact-title and substring searches record it in `WINDOW_CACHE` under these keys).
#[cfg(windows)]
fn cached_window_id(target: &TargetWindow) -> Option<u32> {
    let key = if target.user_selected {
        format!("exact:{}\0{}", target.title_substring, target.process_name)
    } else {
        target.title_substring.clone()
    };
    WINDOW_CACHE
        .lock()
        .ok()?
        .as_ref()
        .filter(|(k, _)| *k == key)
        .map(|(_, id)| *id)
}

/// Capture the configured target. User-picked windows are matched by exact title
/// (and app name when saved); auto-detected window targets use substring heuristics;
/// an ADB phone target is grabbed via `screencap`.
pub fn capture_target(target: &CaptureTarget) -> Option<RgbaImage> {
    capture_target_detailed(target).ok()
}

/// Same as [`capture_target`] but reports why the capture produced no frame.
pub fn capture_target_detailed(target: &CaptureTarget) -> Result<RgbaImage, CaptureFailure> {
    match target {
        CaptureTarget::Window(tw) => capture_window_detailed(tw),
        CaptureTarget::AdbPhone { adb, serial, .. } => {
            capture_adb_frame(adb, serial, adb_save::ADB_SCREENCAP_TIMEOUT)
        }
    }
}

/// How long before the scanner's own deadline a scanner screencap is killed, so `adb`
/// is gone (and the device free for the next one) by the time the scanner gives up.
const ADB_KILL_MARGIN: std::time::Duration = std::time::Duration::from_millis(250);

/// The scanner worker's capture. ADB screencaps get the scanner's deadline: one left
/// running past it holds the device (ADB serializes per device), so the next screencap
/// queues behind it, overruns too, and slow captures snowball into a lasting outage.
fn capture_for_scanner(
    target: &CaptureTarget,
    timeout: std::time::Duration,
    #[cfg(windows)] session: &mut crate::capture_session::WindowSession,
) -> Result<RgbaImage, CaptureFailure> {
    if let CaptureTarget::AdbPhone { adb, serial, .. } = target {
        return capture_adb_frame(adb, serial, timeout.saturating_sub(ADB_KILL_MARGIN));
    }
    #[cfg(windows)]
    return capture_target_with_session(target, session);
    #[cfg(not(windows))]
    capture_target_detailed(target)
}

fn capture_window_detailed(target: &TargetWindow) -> Result<RgbaImage, CaptureFailure> {
    if target.user_selected {
        capture_by_exact_title_detailed(&target.title_substring, &target.process_name)
    } else {
        capture_by_title_detailed(&target.title_substring)
    }
}

/// Grab a single frame from a phone/emulator over ADB via `screencap`'s raw framebuffer
/// dump. No mirroring app or install on the device is needed — `screencap` ships with the
/// Android shell. Raw beats `-p` (PNG): PNG's on-device zlib encode costs the device real
/// CPU time (competing with the game itself for cycles) and is slower end-to-end even
/// though the raw payload is several times larger over the USB/adb pipe (measured ~600ms
/// device-side and ~100ms faster round trip on a Pixel 9a).
fn capture_adb_frame(
    adb: &Path,
    serial: &str,
    timeout: std::time::Duration,
) -> Result<RgbaImage, CaptureFailure> {
    let started = std::time::Instant::now();
    let bytes = adb_save::capture_screenshot(adb, serial, timeout).map_err(|error| {
        // Killed at the deadline: the device is slow, not missing.
        if started.elapsed() >= timeout {
            CaptureFailure::TimedOut {
                after_ms: timeout.as_millis() as u64,
            }
        } else {
            CaptureFailure::AdbCaptureFailed { error }
        }
    })?;
    let img =
        decode_raw_screencap(&bytes).map_err(|error| CaptureFailure::AdbCaptureFailed { error })?;
    if img.width() * img.height() < MIN_CAPTURE_AREA {
        return Err(CaptureFailure::CaptureFailed);
    }
    Ok(crop_letterbox(img))
}

/// A pixel counts as letterbox when every channel is at or below this — Android fills the
/// unused area around a portrait-locked app on a landscape display with pure black.
const LETTERBOX_MAX_CHANNEL: u8 = 8;
/// A column/row is a bar when fewer than 1 in this many sampled pixels are lit, so a stray
/// lit pixel (cursor, compression noise) doesn't stop the trim.
const LETTERBOX_LIT_TOLERANCE: usize = 100;
/// Bars thinner than this fraction of the frame are left alone: a few dark columns at the
/// edge of a normal full-screen frame are game content, not a letterbox.
const LETTERBOX_MIN_FRACTION: f32 = 0.05;
/// Only every Nth pixel along a column/row is checked — a bar is uniform, so sampling
/// keeps this to a few ms on a 2560×1440 frame.
const LETTERBOX_SAMPLE_STEP: usize = 4;

/// Trims black bars around the game when the device's display doesn't match the game's
/// portrait aspect — e.g. The Tower on a 2560×1440 landscape Android VM (ika/Cuttlefish),
/// drawn as a narrow column in the middle. OCR regions are fractions of the frame, so
/// without this they land on the bars and read nothing. Frames without bars, or that are
/// entirely black (loading), come back unchanged.
pub(crate) fn crop_letterbox(img: RgbaImage) -> RgbaImage {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let raw = img.as_raw();
    let lit = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        raw[i] > LETTERBOX_MAX_CHANNEL
            || raw[i + 1] > LETTERBOX_MAX_CHANNEL
            || raw[i + 2] > LETTERBOX_MAX_CHANNEL
    };
    let is_bar = |samples: &mut dyn Iterator<Item = (usize, usize)>| {
        let (mut n, mut lit_n) = (0usize, 0usize);
        for (x, y) in samples {
            n += 1;
            if lit(x, y) {
                lit_n += 1;
            }
        }
        lit_n * LETTERBOX_LIT_TOLERANCE <= n
    };
    let col_is_bar = |x: usize| is_bar(&mut (0..h).step_by(LETTERBOX_SAMPLE_STEP).map(|y| (x, y)));
    let row_is_bar = |y: usize| is_bar(&mut (0..w).step_by(LETTERBOX_SAMPLE_STEP).map(|x| (x, y)));

    let Some(left) = (0..w).find(|&x| !col_is_bar(x)) else {
        return img; // all black
    };
    let right = (0..w).rev().find(|&x| !col_is_bar(x)).unwrap_or(left) + 1;
    let top = (0..h).find(|&y| !row_is_bar(y)).unwrap_or(0);
    let bottom = (0..h).rev().find(|&y| !row_is_bar(y)).unwrap_or(h - 1) + 1;

    let trim_x = (w - (right - left)) as f32 >= w as f32 * LETTERBOX_MIN_FRACTION;
    let trim_y = (h - (bottom - top)) as f32 >= h as f32 * LETTERBOX_MIN_FRACTION;
    let (x0, x1) = if trim_x { (left, right) } else { (0, w) };
    let (y0, y1) = if trim_y { (top, bottom) } else { (0, h) };
    if (x0, x1, y0, y1) == (0, w, 0, h) || ((x1 - x0) * (y1 - y0)) < MIN_CAPTURE_AREA as usize {
        return img;
    }
    image::imageops::crop_imm(&img, x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)
        .to_image()
}

/// Android's `PixelFormat` values `screencap`'s raw header can report. Both lay out each
/// pixel as 4 bytes R,G,B,X — `Rgbx8888`'s 4th byte is unused padding (its value is
/// undefined, not necessarily 255) rather than a real alpha channel.
const HAL_PIXEL_FORMAT_RGBA_8888: u32 = 1;
const HAL_PIXEL_FORMAT_RGBX_8888: u32 = 2;

/// Decode `screencap`'s raw (no `-p`) output: a little-endian `u32 width, u32 height,
/// u32 format` header — plus, on newer Android versions, a trailing `u32 colorSpace` —
/// followed by the raw framebuffer with no row padding. The header version isn't declared
/// anywhere in the stream, so it's inferred from which header length makes the remaining
/// byte count line up with `width * height * 4`.
pub(crate) fn decode_raw_screencap(bytes: &[u8]) -> Result<RgbaImage, String> {
    if bytes.len() < 12 {
        return Err(format!("raw screencap output too short: {} bytes", bytes.len()));
    }
    let read_u32 = |off: usize| u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
    let width = read_u32(0);
    let height = read_u32(4);
    let format = read_u32(8);
    if !matches!(format, HAL_PIXEL_FORMAT_RGBA_8888 | HAL_PIXEL_FORMAT_RGBX_8888) {
        return Err(format!("unsupported raw screencap pixel format {format}"));
    }
    let pixel_bytes = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    let header_len = match bytes.len() - 12 {
        n if n == pixel_bytes => 12,       // width, height, format
        n if n == pixel_bytes + 4 => 16,   // + colorSpace
        _ => {
            return Err(format!(
                "raw screencap size mismatch: {} bytes for {width}x{height}",
                bytes.len()
            ))
        }
    };
    let mut pixels = bytes[header_len..].to_vec();
    if format == HAL_PIXEL_FORMAT_RGBX_8888 {
        for px in pixels.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
    }
    RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| format!("raw screencap buffer size mismatch for {width}x{height}"))
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
fn capture_by_exact_title_detailed(
    title: &str,
    app_name: &str,
) -> Result<RgbaImage, CaptureFailure> {
    let windows = xcap::Window::all().map_err(|e| CaptureFailure::EnumerateFailed {
        error: e.to_string(),
    })?;
    let cache_key = format!("exact:{title}\0{app_name}");

    if let Some(img) = capture_from_cached_id_exact(&windows, title, app_name) {
        return Ok(img);
    }

    let mut failure = CaptureFailure::NoMatchingWindow;
    for w in &windows {
        let wtitle = w.title().unwrap_or_default();
        let app = w.app_name().unwrap_or_default();
        if !window_matches_exact_target(&wtitle, &app, title, app_name) {
            continue;
        }
        if w.is_minimized().unwrap_or(true) {
            failure = CaptureFailure::Minimized;
            continue;
        }
        failure = CaptureFailure::CaptureFailed;
        if let Some(img) = try_capture_window(w) {
            if let Ok(id) = w.id() {
                if let Ok(mut guard) = WINDOW_CACHE.lock() {
                    *guard = Some((cache_key, id));
                }
            }
            return Ok(img);
        }
    }
    Err(failure)
}

/// Capture the largest non-minimized window whose title contains `title_substring`
/// (case-insensitive). Prefers emulator-sized windows over narrow title-bar matches.
/// Retains the matched window id between calls for faster subsequent captures.
fn capture_by_title_detailed(title_substring: &str) -> Result<RgbaImage, CaptureFailure> {
    let needle = title_substring.to_lowercase();
    let windows = xcap::Window::all().map_err(|e| CaptureFailure::EnumerateFailed {
        error: e.to_string(),
    })?;

    if let Some(img) = capture_from_cached_id(&windows, title_substring) {
        return Ok(img);
    }

    let mut failure = CaptureFailure::NoMatchingWindow;
    let mut best: Option<(u32, RgbaImage, u32)> = None;
    for w in &windows {
        let title = w.title().unwrap_or_default();
        if !title.to_lowercase().contains(&needle) {
            continue;
        }
        let app = w.app_name().unwrap_or_default();
        if is_our_app_window(&title, &app) {
            continue;
        }
        if w.is_minimized().unwrap_or(true) {
            failure = CaptureFailure::Minimized;
            continue;
        }
        failure = CaptureFailure::CaptureFailed;
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
        Ok(img)
    } else {
        Err(failure)
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
    use super::{crop_letterbox, decode_raw_screencap, is_our_app_window, window_matches_exact_target};
    use image::{Rgba, RgbaImage};

    /// A `w`×`h` black frame with a lit (game) rectangle at `x..x+gw`, `y..y+gh`.
    fn letterboxed(w: u32, h: u32, x: u32, y: u32, gw: u32, gh: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |px, py| {
            if px >= x && px < x + gw && py >= y && py < y + gh {
                Rgba([40, 20, 60, 255])
            } else {
                Rgba([0, 0, 0, 255])
            }
        })
    }

    #[test]
    fn crop_letterbox_trims_side_bars_of_landscape_display() {
        // ika on a 1440p monitor: portrait game centered on a 2560×1440 display.
        let img = crop_letterbox(letterboxed(2560, 1440, 950, 0, 660, 1440));
        assert_eq!((img.width(), img.height()), (660, 1440));
        assert_eq!(img.get_pixel(0, 0), &Rgba([40, 20, 60, 255]));
    }

    #[test]
    fn crop_letterbox_trims_top_and_bottom_bars() {
        let img = crop_letterbox(letterboxed(1080, 2400, 0, 300, 1080, 1800));
        assert_eq!((img.width(), img.height()), (1080, 1800));
    }

    #[test]
    fn crop_letterbox_keeps_full_frame_with_thin_dark_edge() {
        // 20 dark columns (<5%) at the edge of a normal phone frame are game content.
        let img = crop_letterbox(letterboxed(1080, 2400, 20, 0, 1060, 2400));
        assert_eq!((img.width(), img.height()), (1080, 2400));
    }

    #[test]
    fn crop_letterbox_ignores_stray_lit_pixels_in_bars() {
        let mut frame = letterboxed(2560, 1440, 950, 0, 660, 1440);
        frame.put_pixel(100, 400, Rgba([255, 255, 255, 255]));
        let img = crop_letterbox(frame);
        assert_eq!((img.width(), img.height()), (660, 1440));
    }

    #[test]
    fn crop_letterbox_leaves_all_black_frame_alone() {
        let img = crop_letterbox(letterboxed(2560, 1440, 0, 0, 0, 0));
        assert_eq!((img.width(), img.height()), (2560, 1440));
    }

    fn raw_screencap_bytes(width: u32, height: u32, format: u32, with_color_space: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&format.to_le_bytes());
        if with_color_space {
            bytes.extend_from_slice(&1u32.to_le_bytes());
        }
        for i in 0..(width * height) as usize {
            let v = (i % 255) as u8;
            bytes.extend_from_slice(&[v, v.wrapping_add(1), v.wrapping_add(2), 0xAB]);
        }
        bytes
    }

    #[test]
    fn decodes_legacy_header_without_color_space() {
        let bytes = raw_screencap_bytes(4, 3, super::HAL_PIXEL_FORMAT_RGBA_8888, false);
        let img = decode_raw_screencap(&bytes).expect("decode");
        assert_eq!((img.width(), img.height()), (4, 3));
        assert_eq!(img.get_pixel(0, 0).0, [0, 1, 2, 0xAB]);
    }

    #[test]
    fn decodes_header_with_color_space() {
        let bytes = raw_screencap_bytes(4, 3, super::HAL_PIXEL_FORMAT_RGBA_8888, true);
        let img = decode_raw_screencap(&bytes).expect("decode");
        assert_eq!((img.width(), img.height()), (4, 3));
        assert_eq!(img.get_pixel(0, 0).0, [0, 1, 2, 0xAB]);
    }

    #[test]
    fn rgbx_format_forces_opaque_alpha() {
        let bytes = raw_screencap_bytes(2, 2, super::HAL_PIXEL_FORMAT_RGBX_8888, true);
        let img = decode_raw_screencap(&bytes).expect("decode");
        for px in img.pixels() {
            assert_eq!(px.0[3], 255);
        }
    }

    #[test]
    fn rejects_unsupported_pixel_format() {
        let bytes = raw_screencap_bytes(2, 2, 4 /* RGB_565 */, true);
        assert!(decode_raw_screencap(&bytes).is_err());
    }

    #[test]
    fn rejects_size_mismatch() {
        let mut bytes = raw_screencap_bytes(4, 3, super::HAL_PIXEL_FORMAT_RGBA_8888, false);
        bytes.truncate(bytes.len() - 4);
        assert!(decode_raw_screencap(&bytes).is_err());
    }

    #[test]
    fn our_app_window_ignores_windows_that_merely_mention_the_name() {
        assert!(is_our_app_window("WaveTrace", "wavetrace"));
        assert!(is_our_app_window("WaveTrace", ""));
        assert!(is_our_app_window("WaveTrace — Farm", ""));
        assert!(is_our_app_window("WaveTrace (Dev) — Default", ""));
        assert!(!is_our_app_window(
            "Meringue.WaveTrace_0.3.2.0_x64 - File Explorer",
            "Windows Explorer"
        ));
        assert!(!is_our_app_window(
            "WaveTrace releases - Brave",
            "Brave Browser"
        ));
    }

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
