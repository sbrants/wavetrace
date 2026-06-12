//! Typed view over the settings table (Goal.md "settings" schema).

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::{capture, db};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TargetWindow {
    pub title_substring: String,
    #[serde(default)]
    pub process_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub target_window: Option<TargetWindow>,
    pub poll_interval_ms: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            target_window: None,
            poll_interval_ms: 1000,
        }
    }
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
    s
}

/// Pick a saved target window, or auto-detect a game window and persist it.
pub fn resolve_target_window(conn: &Connection) -> Result<TargetWindow, String> {
    let mut cfg = load(conn);
    if let Some(tw) = cfg.target_window.clone() {
        if !tw.title_substring.trim().is_empty() {
            return Ok(tw);
        }
    }

    const NEEDLES: &[&str] = &["the tower", "bluestacks", "ldplayer", "mumu"];
    for w in capture::list_windows() {
        let title_lower = w.title.to_lowercase();
        if NEEDLES.iter().any(|n| title_lower.contains(n)) {
            let tw = TargetWindow {
                title_substring: w.title.clone(),
                process_name: w.app_name,
            };
            cfg.target_window = Some(tw.clone());
            save(conn, &cfg).map_err(|e| e.to_string())?;
            return Ok(tw);
        }
    }

    Err(
        "No target window configured. Open Settings, pick the game/emulator window, and Save."
            .into(),
    )
}

pub fn save(conn: &Connection, s: &Settings) -> rusqlite::Result<()> {
    if let Some(tw) = &s.target_window {
        db::set_setting(conn, "target_window", &serde_json::to_string(tw).unwrap())?;
    }
    db::set_setting(conn, "poll_interval_ms", &s.poll_interval_ms.to_string())?;
    Ok(())
}
