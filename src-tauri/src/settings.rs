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
    #[serde(default)]
    pub notify_wave_every: Option<u32>,
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
            notify_wave_every: None,
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
    if let Ok(Some(v)) = db::get_setting(conn, "notify_wave_every") {
        s.notify_wave_every = v.parse().ok();
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
    if let Some(n) = s.notify_wave_every {
        db::set_setting(conn, "notify_wave_every", &n.to_string())?;
    } else {
        db::set_setting(conn, "notify_wave_every", "")?;
    }
    Ok(())
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
    }
}
