//! Typed view over the settings table (Goal.md "settings" schema).

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::{capture, db};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TargetWindow {
    pub title_substring: String,
    #[serde(default)]
    pub process_name: String,
    /// True when the user picked a window in Settings. Capture matches that
    /// window by title (and app name when set), not by substring heuristics.
    #[serde(default)]
    pub user_selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub target_window: Option<TargetWindow>,
    pub poll_interval_ms: u64,
    #[serde(default = "default_true")]
    pub minimize_to_tray: bool,
    #[serde(default = "default_true")]
    pub notify_run_ended: bool,
    #[serde(default = "default_true")]
    pub notify_window_lost: bool,
    /// Show alerts in the OS notification center.
    #[serde(default = "default_true")]
    pub notify_desktop_enabled: bool,
    /// Alert when lab research completes (OCR: "Research Complete:").
    #[serde(default = "default_true")]
    pub notify_research_complete: bool,
    /// Alert when an event mission completes in-run (OCR: "EVENT MISSION COMPLETED").
    #[serde(default = "default_true")]
    pub notify_event_mission_complete: bool,
    /// Alert after coin/min has been unavailable this many seconds (total-coin screen).
    #[serde(default)]
    pub notify_coin_unavailable_after_secs: Option<u32>,
    #[serde(default)]
    pub notify_wave_every: Option<u32>,
    /// Mirror desktop notifications to an [ntfy](https://ntfy.sh) topic.
    #[serde(default)]
    pub notify_ntfy_enabled: bool,
    /// Attach the OCR capture frame to ntfy alerts (milestones, run ended, in-game popups).
    #[serde(default = "default_true")]
    pub notify_ntfy_attach_capture: bool,
    /// Topic name (`my-secret-topic`) or full URL (`https://ntfy.sh/my-secret-topic`).
    #[serde(default)]
    pub notify_ntfy_topic: String,
    /// Alert when the OS is shutting down or restarting (best-effort).
    #[serde(default = "default_true")]
    pub notify_system_shutdown: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            target_window: None,
            poll_interval_ms: 1500,
            minimize_to_tray: true,
            notify_run_ended: true,
            notify_window_lost: true,
            notify_desktop_enabled: true,
            notify_research_complete: true,
            notify_event_mission_complete: true,
            notify_coin_unavailable_after_secs: None,
            notify_wave_every: None,
            notify_ntfy_enabled: false,
            notify_ntfy_attach_capture: true,
            notify_ntfy_topic: String::new(),
            notify_system_shutdown: true,
        }
    }
}

/// Collapse a full window title to a short substring that still matches the
/// game/emulator after restart. Avoids saving e.g. a Chrome tab's full title
/// which would never match NoxPlayer's shorter title.
pub fn normalize_target_substring(title: &str) -> String {
    let lower = title.to_lowercase();
    if lower.contains("the tower") {
        return "The Tower".to_string();
    }
    for needle in ["noxplayer", "nox", "bluestacks", "ldplayer", "mumu"] {
        if lower.contains(needle) {
            return needle.to_string();
        }
    }
    title.trim().to_string()
}

pub fn load(conn: &Connection) -> Settings {
    let mut s = Settings::default();
    if let Ok(Some(v)) = db::get_setting(conn, "target_window") {
        s.target_window = serde_json::from_str(&v).ok();
    }
    if let Ok(Some(v)) = db::get_setting(conn, "poll_interval_ms") {
        if let Ok(ms) = v.parse() {
            s.poll_interval_ms = ms;
        }
    }
    if let Ok(Some(v)) = db::get_setting(conn, "minimize_to_tray") {
        if let Ok(on) = v.parse() {
            s.minimize_to_tray = on;
        }
    }
    if let Ok(Some(v)) = db::get_setting(conn, "notify_run_ended") {
        if let Ok(on) = v.parse() {
            s.notify_run_ended = on;
        }
    }
    if let Ok(Some(v)) = db::get_setting(conn, "notify_window_lost") {
        if let Ok(on) = v.parse() {
            s.notify_window_lost = on;
        }
    }
    if let Ok(Some(v)) = db::get_setting(conn, "notify_desktop_enabled") {
        if let Ok(on) = v.parse() {
            s.notify_desktop_enabled = on;
        }
    }
    if let Ok(Some(v)) = db::get_setting(conn, "notify_research_complete") {
        if let Ok(on) = v.parse() {
            s.notify_research_complete = on;
        }
    }
    if let Ok(Some(v)) = db::get_setting(conn, "notify_event_mission_complete") {
        if let Ok(on) = v.parse() {
            s.notify_event_mission_complete = on;
        }
    }
    if let Ok(Some(v)) = db::get_setting(conn, "notify_coin_unavailable_after_secs") {
        s.notify_coin_unavailable_after_secs = v.parse().ok().filter(|&n| n > 0);
    }
    if let Ok(Some(v)) = db::get_setting(conn, "notify_wave_every") {
        s.notify_wave_every = v.parse().ok();
    }
    if let Ok(Some(v)) = db::get_setting(conn, "notify_ntfy_enabled") {
        if let Ok(on) = v.parse() {
            s.notify_ntfy_enabled = on;
        }
    }
    if let Ok(Some(v)) = db::get_setting(conn, "notify_ntfy_attach_capture") {
        if let Ok(on) = v.parse() {
            s.notify_ntfy_attach_capture = on;
        }
    }
    if let Ok(Some(v)) = db::get_setting(conn, "notify_ntfy_topic") {
        s.notify_ntfy_topic = v;
    }
    if let Ok(Some(v)) = db::get_setting(conn, "notify_system_shutdown") {
        if let Ok(on) = v.parse() {
            s.notify_system_shutdown = on;
        }
    }
    s
}

fn is_emulator_app(app_name: &str, title: &str) -> bool {
    let a = app_name.to_lowercase();
    let t = title.to_lowercase();
    a.contains("nox")
        || a.contains("bluestacks")
        || a.contains("ldplayer")
        || a.contains("mumu")
        || t.contains("noxplayer")
        || t.contains("bluestacks")
}

/// Pick a saved target window, or auto-detect a game window and persist it.
pub fn resolve_target_window(conn: &Connection) -> Result<TargetWindow, String> {
    let mut cfg = load(conn);
    if let Some(tw) = cfg.target_window.clone() {
        if !tw.title_substring.trim().is_empty() {
            return Ok(tw);
        }
    }

    let mut best: Option<(u32, TargetWindow)> = None;
    for w in capture::list_windows() {
        let title_lower = w.title.to_lowercase();
        let mut score = 0u32;
        if title_lower.contains("the tower") {
            score += 10;
        }
        if is_emulator_app(&w.app_name, &w.title) {
            score += 100;
        }
        if score == 0 {
            continue;
        }
        let tw = TargetWindow {
            title_substring: normalize_target_substring(&w.title),
            process_name: w.app_name,
            user_selected: false,
        };
        if best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
            best = Some((score, tw));
        }
    }
    if let Some((_, tw)) = best {
        cfg.target_window = Some(tw.clone());
        save(conn, &cfg).map_err(|e| e.to_string())?;
        return Ok(tw);
    }

    Err(
        "No target window configured. Open Settings, pick the game/emulator window, and Save."
            .into(),
    )
}

pub fn save(conn: &Connection, s: &Settings) -> rusqlite::Result<()> {
    if let Some(tw) = &s.target_window {
        let to_save = if tw.user_selected {
            tw.clone()
        } else {
            let mut normalized = tw.clone();
            normalized.title_substring = normalize_target_substring(&tw.title_substring);
            normalized
        };
        db::set_setting(
            conn,
            "target_window",
            &serde_json::to_string(&to_save).unwrap(),
        )?;
    }
    db::set_setting(conn, "poll_interval_ms", &s.poll_interval_ms.to_string())?;
    db::set_setting(conn, "minimize_to_tray", &s.minimize_to_tray.to_string())?;
    db::set_setting(conn, "notify_run_ended", &s.notify_run_ended.to_string())?;
    db::set_setting(conn, "notify_window_lost", &s.notify_window_lost.to_string())?;
    db::set_setting(
        conn,
        "notify_desktop_enabled",
        &s.notify_desktop_enabled.to_string(),
    )?;
    db::set_setting(
        conn,
        "notify_research_complete",
        &s.notify_research_complete.to_string(),
    )?;
    db::set_setting(
        conn,
        "notify_event_mission_complete",
        &s.notify_event_mission_complete.to_string(),
    )?;
    if let Some(n) = s.notify_coin_unavailable_after_secs {
        db::set_setting(
            conn,
            "notify_coin_unavailable_after_secs",
            &n.to_string(),
        )?;
    } else {
        db::set_setting(conn, "notify_coin_unavailable_after_secs", "")?;
    }
    if let Some(n) = s.notify_wave_every {
        db::set_setting(conn, "notify_wave_every", &n.to_string())?;
    } else {
        db::set_setting(conn, "notify_wave_every", "")?;
    }
    db::set_setting(
        conn,
        "notify_ntfy_enabled",
        &s.notify_ntfy_enabled.to_string(),
    )?;
    db::set_setting(
        conn,
        "notify_ntfy_attach_capture",
        &s.notify_ntfy_attach_capture.to_string(),
    )?;
    db::set_setting(conn, "notify_ntfy_topic", &s.notify_ntfy_topic)?;
    db::set_setting(
        conn,
        "notify_system_shutdown",
        &s.notify_system_shutdown.to_string(),
    )?;
    Ok(())
}

/// Resolve a topic name or URL into a publish endpoint for ntfy.
pub fn resolve_ntfy_url(topic_or_url: &str) -> Result<String, String> {
    let raw = topic_or_url.trim();
    if raw.is_empty() {
        return Err("ntfy topic is empty".into());
    }
    if raw.starts_with("https://") || raw.starts_with("http://") {
        return Ok(raw.trim_end_matches('/').to_string());
    }
    if raw.contains('/') || raw.contains(' ') || raw.contains('?') {
        return Err(
            "Use a plain topic name (e.g. wavetrace-my-secret) or a full https:// URL".into(),
        );
    }
    Ok(format!("https://ntfy.sh/{raw}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_chrome_title_to_tower_substring() {
        assert_eq!(
            normalize_target_substring("The Tower - Google Chrome"),
            "The Tower"
        );
        assert_eq!(normalize_target_substring("NoxPlayer"), "noxplayer");
    }

    #[test]
    fn save_preserves_exact_title_when_user_selected() {
        let conn = db::open_in_memory().expect("db");
        let s = Settings {
            target_window: Some(TargetWindow {
                title_substring: "The Tower - Google Chrome".into(),
                process_name: "Google Chrome".into(),
                user_selected: true,
            }),
            ..Settings::default()
        };
        save(&conn, &s).expect("save");
        let loaded = load(&conn);
        let tw = loaded.target_window.expect("target");
        assert_eq!(tw.title_substring, "The Tower - Google Chrome");
        assert_eq!(tw.process_name, "Google Chrome");
        assert!(tw.user_selected);
    }

    #[test]
    fn save_normalizes_substring_when_not_user_selected() {
        let conn = db::open_in_memory().expect("db");
        let s = Settings {
            target_window: Some(TargetWindow {
                title_substring: "The Tower - Google Chrome".into(),
                process_name: String::new(),
                user_selected: false,
            }),
            ..Settings::default()
        };
        save(&conn, &s).expect("save");
        let loaded = load(&conn);
        let tw = loaded.target_window.expect("target");
        assert_eq!(tw.title_substring, "The Tower");
        assert!(!tw.user_selected);
    }

    #[test]
    fn default_notify_and_tray_settings() {
        let s = Settings::default();
        assert!(s.minimize_to_tray);
        assert!(s.notify_run_ended);
        assert!(s.notify_window_lost);
        assert_eq!(s.notify_wave_every, None);
        assert!(!s.notify_ntfy_enabled);
        assert!(s.notify_ntfy_topic.is_empty());
    }

    #[test]
    fn resolve_ntfy_url_accepts_topic_or_full_url() {
        assert_eq!(
            resolve_ntfy_url("wavetrace-secret").unwrap(),
            "https://ntfy.sh/wavetrace-secret"
        );
        assert_eq!(
            resolve_ntfy_url("https://ntfy.example.com/my-topic/").unwrap(),
            "https://ntfy.example.com/my-topic"
        );
        assert!(resolve_ntfy_url("").is_err());
        assert!(resolve_ntfy_url("bad topic").is_err());
    }

    #[test]
    fn save_and_load_ntfy_settings() {
        let conn = db::open_in_memory().expect("db");
        let s = Settings {
            notify_ntfy_enabled: true,
            notify_ntfy_attach_capture: false,
            notify_ntfy_topic: "wavetrace-test".into(),
            ..Settings::default()
        };
        save(&conn, &s).expect("save");
        let loaded = load(&conn);
        assert!(loaded.notify_ntfy_enabled);
        assert!(!loaded.notify_ntfy_attach_capture);
        assert_eq!(loaded.notify_ntfy_topic, "wavetrace-test");
    }
}
