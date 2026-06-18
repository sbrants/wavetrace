//! Desktop notifications for scanner events (local-only).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::{NotificationExt, PermissionState};

use crate::settings::{self, Settings};
use crate::state_machine::Action;

pub struct NotifyState {
    last_status: Mutex<String>,
    window_lost_notified: AtomicBool,
    last_milestone_wave: Mutex<u32>,
    permission_requested: AtomicBool,
}

impl Default for NotifyState {
    fn default() -> Self {
        Self {
            last_status: Mutex::new(String::new()),
            window_lost_notified: AtomicBool::new(false),
            last_milestone_wave: Mutex::new(0),
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
                show(
                    app,
                    "Game window not found",
                    "WaveTrace can't see the target window. Check Settings or bring the emulator to the foreground.",
                );
            }
        } else if status == "scanning" && prev == "window_not_found" {
            self.window_lost_notified.store(false, Ordering::SeqCst);
        }
    }

    pub fn on_actions(&self, app: &AppHandle, actions: &[Action]) {
        let cfg = load_settings();
        for action in actions {
            match action {
                Action::StartRun { .. } => {
                    *self.last_milestone_wave.lock().unwrap() = 0;
                }
                Action::EndRun {
                    final_wave,
                    peak_tier,
                } if cfg.notify_run_ended => {
                    let tier = peak_tier
                        .map(|t| format!("tier {t}"))
                        .unwrap_or_else(|| "run".into());
                    show(
                        app,
                        "Run ended",
                        &format!("Finished at wave {final_wave} ({tier})."),
                    );
                }
                Action::Snapshot { wave, .. } => {
                    if let Some(every) = cfg.notify_wave_every {
                        if *wave > 0 && *wave % every == 0 {
                            let mut last = self.last_milestone_wave.lock().unwrap();
                            if *last != *wave {
                                *last = *wave;
                                show(app, "Wave milestone", &format!("Reached wave {wave}."));
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
    }
}

fn load_settings() -> Settings {
    crate::db::open()
        .map(|conn| settings::load(&conn))
        .unwrap_or_default()
}

fn show(app: &AppHandle, title: &str, body: &str) {
    if let Some(state) = app.try_state::<NotifyState>() {
        state.ensure_permission(app);
    }
    let _ = app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show();
}
