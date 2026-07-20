//! Desktop notifications for scanner events, with optional ntfy phone mirror.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use image::RgbaImage;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::{NotificationExt, PermissionState};

use crate::parser::CoinReading;
use crate::settings::{self, Settings};
use crate::state_machine::{Action, LiveState, PollInput, RunType};

/// OCR values from the same capture frame as an optional screenshot attachment.
#[derive(Debug, Clone, Copy, Default)]
pub struct NotifyFrameContext {
    pub tier: Option<u32>,
    pub coin_per_minute: Option<f64>,
}

pub fn frame_context_from_poll(input: &PollInput) -> NotifyFrameContext {
    NotifyFrameContext {
        tier: input.tier,
        coin_per_minute: match input.coin {
            CoinReading::Rate(v) => Some(v),
            _ => None,
        },
    }
}

pub struct NotifyState {
    last_status: Mutex<String>,
    window_lost_notified: AtomicBool,
    last_milestone_wave: Mutex<u32>,
    last_research_notified: Mutex<Option<String>>,
    coin_unavailable_since: Mutex<Option<Instant>>,
    coin_unavailable_notified: AtomicBool,
    permission_requested: AtomicBool,
}

impl Default for NotifyState {
    fn default() -> Self {
        Self {
            last_status: Mutex::new(String::new()),
            window_lost_notified: AtomicBool::new(false),
            last_milestone_wave: Mutex::new(0),
            last_research_notified: Mutex::new(None),
            coin_unavailable_since: Mutex::new(None),
            coin_unavailable_notified: AtomicBool::new(false),
            permission_requested: AtomicBool::new(false),
        }
    }
}

impl NotifyState {
    pub fn ensure_permission(&self, app: &AppHandle) {
        if self.permission_requested.swap(true, Ordering::SeqCst) {
            return;
        }
        let Ok(state) = app.notification().permission_state() else {
            return;
        };
        if state != PermissionState::Granted {
            let _ = app.notification().request_permission();
        }
    }

    pub fn on_scanner_status(&self, app: &AppHandle, status: &str) {
        let cfg = load_settings();
        if !cfg.notify_window_lost {
            return;
        }

        let prev = self.last_status.lock().unwrap().clone();
        if prev == status {
            return;
        }
        *self.last_status.lock().unwrap() = status.to_string();

        if status == "window_not_found" && prev != "window_not_found" {
            if !self.window_lost_notified.swap(true, Ordering::SeqCst) {
                show(app, "Game window not found", "WaveTrace can't see the target window. Check Settings or bring the emulator to the foreground.", false, None);
            }
        } else if status == "scanning" && prev == "window_not_found" {
            self.window_lost_notified.store(false, Ordering::SeqCst);
        }
    }

    pub fn on_poll(
        &self,
        app: &AppHandle,
        ocr_lines: &[String],
        live: &LiveState,
        capture: Option<&RgbaImage>,
        frame: NotifyFrameContext,
    ) {
        let cfg = load_settings();
        if cfg.notify_research_complete {
            self.check_research_complete(app, ocr_lines, &cfg, capture, frame);
        }
        self.check_coin_unavailable(app, live, &cfg, capture);
    }

    fn check_research_complete(
        &self,
        app: &AppHandle,
        ocr_lines: &[String],
        cfg: &Settings,
        capture: Option<&RgbaImage>,
        frame: NotifyFrameContext,
    ) {
        let Some(name) = parse_research_complete(ocr_lines) else {
            *self.last_research_notified.lock().unwrap() = None;
            return;
        };
        let mut last = self.last_research_notified.lock().unwrap();
        if last.as_deref() == Some(name.as_str()) {
            return;
        }
        *last = Some(name.clone());
        drop(last);
        let mut parts = vec![name];
        if let Some(t) = frame.tier.or(live_tier_hint(ocr_lines)) {
            parts.push(format!("Tier {t}"));
        }
        if let Some(coin) = frame.coin_per_minute {
            parts.push(format_coin(coin));
        }
        let body = parts.join(" · ");
        show(
            app,
            "Lab research complete",
            &body,
            ntfy_attach_capture(cfg),
            capture,
        );
    }

    fn check_coin_unavailable(
        &self,
        app: &AppHandle,
        live: &LiveState,
        cfg: &Settings,
        capture: Option<&RgbaImage>,
    ) {
        let Some(threshold_secs) = cfg.notify_coin_unavailable_after_secs.filter(|&n| n > 0) else {
            *self.coin_unavailable_since.lock().unwrap() = None;
            self.coin_unavailable_notified
                .store(false, Ordering::SeqCst);
            return;
        };

        if live.run_active && live.total_coin_warning {
            let mut since = self.coin_unavailable_since.lock().unwrap();
            if since.is_none() {
                *since = Some(Instant::now());
            }
            let elapsed = since.unwrap().elapsed();
            drop(since);
            if elapsed >= std::time::Duration::from_secs(threshold_secs as u64)
                && !self
                    .coin_unavailable_notified
                    .swap(true, Ordering::SeqCst)
            {
                let mins = elapsed.as_secs() / 60;
                let body = if mins >= 1 {
                    format!(
                        "Game is showing total coins, not coins/min, for {mins}+ min. Snapshots won't update coin/min until the rate returns."
                    )
                } else {
                    format!(
                        "Game is showing total coins, not coins/min, for {threshold_secs}+ seconds. Snapshots won't update coin/min until the rate returns."
                    )
                };
                show(
                    app,
                    "Coin/min unavailable",
                    &body,
                    ntfy_attach_capture(cfg),
                    capture,
                );
            }
        } else {
            *self.coin_unavailable_since.lock().unwrap() = None;
            self.coin_unavailable_notified
                .store(false, Ordering::SeqCst);
        }
    }

    pub fn on_actions(
        &self,
        app: &AppHandle,
        actions: &[Action],
        capture: Option<&RgbaImage>,
        frame: NotifyFrameContext,
    ) {
        let cfg = load_settings();
        for action in actions {
            match action {
                Action::StartRun { .. } => {
                    *self.last_milestone_wave.lock().unwrap() = 0;
                }
                Action::EndRun {
                    final_wave,
                    peak_tier,
                    run_type,
                    snapshot_count,
                    avg_coin_per_minute,
                    last_coin_per_minute,
                } if cfg.notify_run_ended => {
                    let (title, body) = format_end_run_notification(
                        *final_wave,
                        frame.tier.or(*peak_tier),
                        *run_type,
                        *snapshot_count,
                        *avg_coin_per_minute,
                        frame.coin_per_minute.or(*last_coin_per_minute),
                    );
                    show(
                        app,
                        &title,
                        &body,
                        ntfy_attach_capture(&cfg),
                        capture,
                    );
                }
                Action::Snapshot {
                    wave,
                    tier,
                    coin_per_minute,
                } => {
                    if let Some(every) = cfg.notify_wave_every {
                        if every > 0 {
                            let mut last = self.last_milestone_wave.lock().unwrap();
                            for milestone in
                                crossed_wave_milestones(*wave, every, *last)
                            {
                                *last = milestone;
                                let (title, body) = format_wave_milestone_notification(
                                    milestone,
                                    frame.tier.or(*tier),
                                    frame.coin_per_minute.or(*coin_per_minute),
                                );
                                show(
                                    app,
                                    &title,
                                    &body,
                                    ntfy_attach_capture(&cfg),
                                    capture,
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub fn reset_run_tracking(&self) {
        *self.last_milestone_wave.lock().unwrap() = 0;
        *self.last_research_notified.lock().unwrap() = None;
        *self.coin_unavailable_since.lock().unwrap() = None;
        self.coin_unavailable_notified
            .store(false, Ordering::SeqCst);
    }

    /// After resume, treat milestones at or below `wave` as already notified.
    pub fn seed_milestone_from_wave(&self, wave: u32, every: Option<u32>) {
        let Some(every) = every.filter(|&n| n > 0) else {
            return;
        };
        *self.last_milestone_wave.lock().unwrap() = (wave / every) * every;
    }
}

fn load_settings() -> Settings {
    crate::db::open()
        .map(|conn| settings::load(&conn))
        .unwrap_or_default()
}

fn ntfy_attach_capture(cfg: &Settings) -> bool {
    cfg.notify_ntfy_enabled && cfg.notify_ntfy_attach_capture
}

/// Lab upgrade name from OCR banner ("Research Complete: Starting Cash Lv.33").
pub fn parse_research_complete(lines: &[String]) -> Option<String> {
    for (i, line) in lines.iter().enumerate() {
        let lower = line.to_lowercase();
        if !lower.contains("research complete") {
            continue;
        }
        if let Some((_, rest)) = line.split_once(':') {
            let name = rest.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
        if let Some(next) = lines.get(i + 1) {
            let name = next.trim();
            if !name.is_empty()
                && !name.starts_with('@')
                && !name.eq_ignore_ascii_case("CLAIM")
            {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn live_tier_hint(lines: &[String]) -> Option<u32> {
    for line in lines {
        let lower = line.to_lowercase();
        if let Some(rest) = lower.strip_prefix("tier ") {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(t) = digits.parse() {
                return Some(t);
            }
        }
    }
    None
}

/// Highest milestone newly reached by `wave` since `last_notified` (at most one alert per snapshot).
fn crossed_wave_milestones(wave: u32, every: u32, last_notified: u32) -> Vec<u32> {
    if every == 0 || wave == 0 {
        return Vec::new();
    }
    let highest = (wave / every) * every;
    if highest == 0 || highest <= last_notified {
        return Vec::new();
    }
    vec![highest]
}

fn show(
    app: &AppHandle,
    title: &str,
    body: &str,
    attach_ntfy_capture: bool,
    capture: Option<&RgbaImage>,
) {
    if let Some(state) = app.try_state::<NotifyState>() {
        state.ensure_permission(app);
    }
    let _ = app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show();
    let capture_owned = if attach_ntfy_capture {
        capture.cloned()
    } else {
        None
    };
    publish_ntfy_async(title, body, capture_owned);
}

fn format_wave_milestone_notification(
    wave: u32,
    tier: Option<u32>,
    coin_per_minute: Option<f64>,
) -> (String, String) {
    let title = format!("Wave {}", format_wave(wave));
    let mut parts = Vec::new();
    if let Some(t) = tier {
        parts.push(format!("Tier {t}"));
    }
    if let Some(coin) = coin_per_minute {
        parts.push(format_coin(coin));
    }
    let body = if parts.is_empty() {
        "Milestone reached.".to_string()
    } else {
        parts.join(" · ")
    };
    (title, body)
}

fn format_end_run_notification(
    final_wave: u32,
    peak_tier: Option<u32>,
    run_type: RunType,
    snapshot_count: u32,
    avg_coin_per_minute: Option<f64>,
    last_coin_per_minute: Option<f64>,
) -> (String, String) {
    let title = format!("Run ended — wave {}", format_wave(final_wave));
    let mut parts = Vec::new();
    if let Some(t) = peak_tier {
        parts.push(format!("Tier {t}"));
    }
    match (last_coin_per_minute, avg_coin_per_minute) {
        (Some(last), Some(avg)) if (last - avg).abs() > avg.abs() * 0.02 => {
            parts.push(format!("{} now", format_coin(last)));
            parts.push(format!("{} avg", format_coin(avg)));
        }
        (Some(last), None) => parts.push(format_coin(last)),
        (_, Some(avg)) => parts.push(format!("{} avg", format_coin(avg))),
        _ => {}
    }
    parts.push(format!("{snapshot_count} snapshots"));
    parts.push(run_type_label(run_type).to_string());
    (title, parts.join(" · "))
}

fn run_type_label(run_type: RunType) -> &'static str {
    match run_type {
        RunType::Farming => "farming",
        RunType::Tournament => "tournament",
        RunType::DissonanceAttack => "dissonance attack",
        RunType::DissonanceDefense => "dissonance defense",
        RunType::DissonanceUtility => "dissonance utility",
        RunType::DissonanceUltimateWeapons => "dissonance ultimate weapons",
    }
}

/// Game-style coin display (e.g. 44.2T), matching the frontend `formatCoin`.
fn format_coin(value: f64) -> String {
    const SUFFIXES: [&str; 12] = ["", "K", "M", "B", "T", "q", "Q", "s", "S", "O", "N", "D"];
    let mut idx = 0usize;
    let mut v = value;
    while v.abs() >= 1000.0 && idx < SUFFIXES.len() - 1 {
        v /= 1000.0;
        idx += 1;
    }
    let num = if v.abs() >= 100.0 {
        format!("{:.1}", v)
    } else {
        format!("{:.2}", v)
    };
    let trimmed = trim_trailing_zeros(&num);
    format!("{trimmed}{}/min", SUFFIXES[idx])
}

fn trim_trailing_zeros(num: &str) -> String {
    if !num.contains('.') {
        return num.to_string();
    }
    let trimmed = num.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
}

fn format_wave(wave: u32) -> String {
    let s = wave.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

/// ntfy.sh allows 2 MB per attachment (public server); stay safely below that.
const NTFY_ATTACH_MAX_BYTES: usize = 1_900_000;
const NTFY_ATTACH_MAX_WIDTH: u32 = 720;

struct NtfyAttachment {
    bytes: Vec<u8>,
    content_type: &'static str,
    filename: &'static str,
}

fn prepare_ntfy_capture(img: &RgbaImage) -> Result<NtfyAttachment, String> {
    use image::codecs::jpeg::JpegEncoder;

    let mut rgba = img.clone();
    while rgba.width() > NTFY_ATTACH_MAX_WIDTH {
        let new_w = (rgba.width() * 3 / 4).max(NTFY_ATTACH_MAX_WIDTH);
        let new_h =
            ((rgba.height() as u64 * new_w as u64) / rgba.width().max(1) as u64) as u32;
        rgba = image::imageops::resize(
            &rgba,
            new_w,
            new_h.max(1),
            image::imageops::FilterType::Triangle,
        );
    }

    let rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
    for quality in [85u8, 75, 65, 55, 45, 35] {
        let mut buf = Vec::new();
        let mut enc = JpegEncoder::new_with_quality(&mut buf, quality);
        enc.encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| format!("jpeg encode failed: {e}"))?;
        if buf.len() <= NTFY_ATTACH_MAX_BYTES {
            return Ok(NtfyAttachment {
                bytes: buf,
                content_type: "image/jpeg",
                filename: "game.jpg",
            });
        }
    }
    Err("game capture is too large for ntfy even after compression".into())
}

/// Fire-and-forget ntfy publish using saved settings (when enabled).
fn publish_ntfy_async(title: &str, body: &str, capture: Option<RgbaImage>) {
    let cfg = load_settings();
    if !cfg.notify_ntfy_enabled {
        return;
    }
    let title = title.to_string();
    let body = body.to_string();
    let topic = cfg.notify_ntfy_topic.clone();
    std::thread::spawn(move || {
        let result = if let Some(frame) = capture {
            match prepare_ntfy_capture(&frame) {
                Ok(att) => publish_ntfy_with_attachment(&topic, &title, &body, &att),
                Err(e) => {
                    eprintln!("ntfy capture encode failed: {e}");
                    publish_ntfy(&topic, &title, &body)
                }
            }
        } else {
            publish_ntfy(&topic, &title, &body)
        };
        if let Err(e) = result {
            eprintln!("ntfy publish failed: {e}");
        }
    });
}

/// Publish a text-only notification to an ntfy topic (sync).
pub fn publish_ntfy(topic_or_url: &str, title: &str, body: &str) -> Result<(), String> {
    let url = settings::resolve_ntfy_url(topic_or_url)?;
    let response = ureq::post(&url)
        .header("Title", title)
        .header("Tags", "bell")
        .header("Priority", "default")
        .send(body)
        .map_err(|e| format!("ntfy request failed: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("ntfy returned HTTP {status}"));
    }
    Ok(())
}

/// Publish a notification with a compressed capture attached (sync).
fn publish_ntfy_with_attachment(
    topic_or_url: &str,
    title: &str,
    body: &str,
    attachment: &NtfyAttachment,
) -> Result<(), String> {
    let url = settings::resolve_ntfy_url(topic_or_url)?;
    let response = ureq::put(&url)
        .header("Title", title)
        .header("Message", body)
        .header("Filename", attachment.filename)
        .header("Content-Type", attachment.content_type)
        .header("Tags", "bell")
        .header("Priority", "default")
        .send(&attachment.bytes)
        .map_err(|e| format!("ntfy image upload failed: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("ntfy returned HTTP {status}"));
    }
    Ok(())
}

/// Send a test notification using the currently saved ntfy settings.
pub fn send_test_ntfy() -> Result<(), String> {
    let cfg = load_settings();
    if cfg.notify_ntfy_topic.trim().is_empty() {
        return Err("Set an ntfy topic first".into());
    }
    publish_ntfy(
        &cfg.notify_ntfy_topic,
        "WaveTrace",
        "Test notification — phone alerts are working.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_coin_uses_game_suffixes() {
        assert_eq!(format_coin(44.2e12), "44.2T/min");
        assert_eq!(format_coin(2.65e6), "2.65M/min");
    }

    #[test]
    fn wave_milestone_includes_tier_and_coin() {
        let (title, body) = format_wave_milestone_notification(2000, Some(15), Some(44.2e12));
        assert_eq!(title, "Wave 2,000");
        assert_eq!(body, "Tier 15 · 44.2T/min");
    }

    #[test]
    fn crossed_wave_milestones_exact_hit() {
        assert_eq!(crossed_wave_milestones(1000, 1000, 0), vec![1000]);
        assert_eq!(crossed_wave_milestones(1000, 1000, 1000), Vec::<u32>::new());
    }

    #[test]
    fn crossed_wave_milestones_after_wave_skip() {
        assert_eq!(crossed_wave_milestones(1003, 1000, 0), vec![1000]);
        assert_eq!(crossed_wave_milestones(1003, 1000, 1000), Vec::<u32>::new());
        assert_eq!(crossed_wave_milestones(2005, 1000, 1000), vec![2000]);
    }

    #[test]
    fn crossed_wave_milestones_large_skip_notifies_latest_only() {
        assert_eq!(crossed_wave_milestones(2500, 1000, 0), vec![2000]);
    }

    #[test]
    fn seed_milestone_from_wave_skips_already_passed() {
        let state = NotifyState::default();
        state.seed_milestone_from_wave(5003, Some(100));
        assert_eq!(*state.last_milestone_wave.lock().unwrap(), 5000);
        assert_eq!(crossed_wave_milestones(5003, 100, 5000), Vec::<u32>::new());
        assert_eq!(crossed_wave_milestones(5100, 100, 5000), vec![5100]);
    }

    #[test]
    fn parse_research_complete_from_colon_line() {
        let lines = vec![
            "Executed 928 times".into(),
            "Research Complete:".into(),
            "Starting Cash Lv.33".into(),
        ];
        assert_eq!(
            parse_research_complete(&lines).as_deref(),
            Some("Starting Cash Lv.33")
        );
    }

    #[test]
    fn parse_research_complete_inline() {
        let lines = vec!["Research Complete: Battle Condition Reduction Lv.8".into()];
        assert_eq!(
            parse_research_complete(&lines).as_deref(),
            Some("Battle Condition Reduction Lv.8")
        );
    }

    #[test]
    fn parse_research_complete_absent() {
        assert_eq!(parse_research_complete(&["Wave 100".into()]), None);
    }

    #[test]
    fn crossed_wave_milestones_below_first() {
        assert_eq!(crossed_wave_milestones(999, 1000, 0), Vec::<u32>::new());
    }

    #[test]
    fn end_run_includes_summary_stats() {
        let (title, body) = format_end_run_notification(
            2003,
            Some(15),
            RunType::Farming,
            127,
            Some(42.1e12),
            Some(44.2e12),
        );
        assert_eq!(title, "Run ended — wave 2,003");
        assert!(body.contains("Tier 15"));
        assert!(body.contains("44.2T/min now"));
        assert!(body.contains("42.1T/min avg"));
        assert!(body.contains("127 snapshots"));
        assert!(body.contains("farming"));
    }

    #[test]
    fn prepare_ntfy_capture_fits_public_limit() {
        let img = RgbaImage::from_fn(884, 1880, |x, y| {
            image::Rgba([
                ((x * 3) % 256) as u8,
                ((y * 5) % 256) as u8,
                128,
                255,
            ])
        });
        let att = prepare_ntfy_capture(&img).expect("jpeg");
        assert!(att.bytes.len() <= NTFY_ATTACH_MAX_BYTES);
        assert_eq!(att.content_type, "image/jpeg");
    }
}
