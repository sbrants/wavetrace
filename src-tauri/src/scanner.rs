//! Scanner thread: capture -> OCR -> classify -> state machine -> DB + events.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::state_machine::{Action, LiveState, RunStateMachine, RunType};
use crate::{capture, db, fields, settings};

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStartMode {
    NewRun,
    ResumePrevious,
}

#[derive(Clone, Serialize)]
pub struct ScannerEvent {
    pub status: String, // scanning | window_not_found | ocr_error | stopped
    pub live: Option<LiveState>,
    pub current_run_id: Option<String>,
}

pub struct Scanner {
    running: Arc<AtomicBool>,
    pub machine: Arc<Mutex<RunStateMachine>>,
    pub current_run_id: Arc<Mutex<Option<String>>>,
    app: Arc<Mutex<Option<AppHandle>>>,
    /// Last emitted live state — UI reads this without waiting on the scanner mutex.
    cached_live: Arc<Mutex<LiveState>>,
}

impl Default for Scanner {
    fn default() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            machine: Arc::new(Mutex::new(RunStateMachine::new())),
            current_run_id: Arc::new(Mutex::new(None)),
            app: Arc::new(Mutex::new(None)),
            cached_live: Arc::new(Mutex::new(LiveState::idle())),
        }
    }
}

impl Scanner {
    pub fn cached_live_state(&self) -> LiveState {
        self.cached_live.lock().unwrap().clone()
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Ok(guard) = self.app.lock() {
            if let Some(app) = guard.as_ref() {
                emit(
                    app,
                    "stopped",
                    &self.machine,
                    &self.current_run_id,
                    &self.cached_live,
                );
            }
        }
    }

    /// Clear in-memory scan state after the database file was replaced.
    pub fn reset_after_db_restore(&self) {
        *self.machine.lock().unwrap() = RunStateMachine::new();
        *self.current_run_id.lock().unwrap() = None;
        *self.cached_live.lock().unwrap() = LiveState::idle();
    }

    pub fn has_resumable_run(&self) -> Result<bool, String> {
        if self.machine.lock().unwrap().has_active_run() {
            return Ok(true);
        }
        let conn = db::open().map_err(|e| e.to_string())?;
        Ok(db::latest_open_run(&conn)
            .map_err(|e| e.to_string())?
            .is_some())
    }

    pub fn start(&self, app: AppHandle, mode: ScanStartMode) -> Result<(), String> {
        if self.is_running() {
            return Ok(());
        }
        let conn = db::open().map_err(|e| e.to_string())?;
        let cfg = settings::load(&conn);
        let target = settings::resolve_target_window(&conn)?;

        self.running.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.app.lock() {
            *guard = Some(app.clone());
        }

        let log_path = db::app_data_dir().join("logs");
        std::fs::create_dir_all(&log_path).ok();
        let start_actions = match mode {
            ScanStartMode::NewRun => self.machine.lock().unwrap().manual_new_run(),
            ScanStartMode::ResumePrevious => self.prepare_resume(&conn)?,
        };
        if !start_actions.is_empty() {
            apply_actions(&conn, &self.current_run_id, &start_actions, &log_path);
            notify_scanner_actions(&app, &start_actions);
        }

        let running = self.running.clone();
        let machine = self.machine.clone();
        let current_run_id = self.current_run_id.clone();
        let app_slot = self.app.clone();
        let cached_live = self.cached_live.clone();

        std::thread::spawn(move || {
            let log_path = db::app_data_dir().join("logs");
            std::fs::create_dir_all(&log_path).ok();
            emit(&app, "starting", &machine, &current_run_id, &cached_live);

            while running.load(Ordering::SeqCst) {
                let tick = Instant::now();

                if !running.load(Ordering::SeqCst) {
                    break;
                }
                let capture_started = Instant::now();
                let frame = capture::capture_by_title(&target.title_substring);
                let capture_ms = capture_started.elapsed().as_millis();
                let status = match frame {
                    None => {
                        emit(
                            &app,
                            "window_not_found",
                            &machine,
                            &current_run_id,
                            &cached_live,
                        );
                        sleep_remainder(tick, cfg.poll_interval_ms);
                        continue;
                    }
                    Some(full) => {
                        let should_continue = || running.load(Ordering::SeqCst);
                        let fields = fields::ocr_all_fields_cancellable(&full, &should_continue);
                        if !should_continue() {
                            break;
                        }
                        let input = fields::poll_input_from_fields(&fields);
                        log_line(
                            &log_path,
                            &format!(
                                "poll {}x{} capture_ms={} ocr_ms={} \
                                 tier={:?} wave={:?} coin={:?} skip={:?} lines={:?}",
                                full.width(),
                                full.height(),
                                capture_ms,
                                fields.ocr_ms,
                                input.tier,
                                input.wave,
                                input.coin,
                                input.wave_skip_overlay,
                                fields.all_lines,
                            ),
                        );
                        let actions = machine.lock().unwrap().poll(input);
                        if !actions.is_empty() {
                            apply_actions(&conn, &current_run_id, &actions, &log_path);
                            notify_scanner_actions(&app, &actions);
                        }
                        "scanning"
                    }
                };
                emit(&app, status, &machine, &current_run_id, &cached_live);
                sleep_remainder(tick, cfg.poll_interval_ms);
            }
            emit(&app, "stopped", &machine, &current_run_id, &cached_live);
            if let Ok(mut guard) = app_slot.lock() {
                *guard = None;
            }
        });
        Ok(())
    }

    fn prepare_resume(&self, conn: &rusqlite::Connection) -> Result<Vec<Action>, String> {
        let Some((id, run_type)) = db::latest_open_run(conn).map_err(|e| e.to_string())? else {
            return Err("No run to resume — start a new run instead.".into());
        };
        let (last_wave, peak_tier) = db::snapshot_stats(conn, &id).map_err(|e| e.to_string())?;
        let run_type = match run_type.as_str() {
            "tournament" => RunType::Tournament,
            _ => RunType::Farming,
        };
        // Always re-sync from DB: the game may have advanced while the scanner was stopped.
        self.machine.lock().unwrap().resume_from_db(
            run_type,
            last_wave.unwrap_or(0) as u32,
            peak_tier.map(|t| t as u32),
        );
        *self.current_run_id.lock().unwrap() = Some(id);
        Ok(Vec::new())
    }
}

pub fn apply_actions(
    conn: &rusqlite::Connection,
    current_run_id: &Arc<Mutex<Option<String>>>,
    actions: &[Action],
    _log_path: &std::path::Path,
) {
    for action in actions {
        let result = match action {
            Action::StartRun { run_type } => {
                // Drop stale tracking before start_run closes any open rows.
                current_run_id.lock().unwrap().take();
                db::start_run(conn, run_type.as_str())
                    .map(|id| *current_run_id.lock().unwrap() = Some(id))
            }
            Action::Snapshot {
                wave,
                tier,
                coin_per_minute,
            } => {
                let id = current_run_id.lock().unwrap().clone();
                match id {
                    Some(id) => db::insert_snapshot(
                        conn,
                        &id,
                        *wave as i64,
                        tier.map(|t| t as i64),
                        *coin_per_minute,
                    ),
                    None => Ok(()),
                }
            }
            Action::WaveSkip {
                at_wave,
                skipped_count,
                skip_multiplier,
                coin_per_minute,
            } => {
                let id = current_run_id.lock().unwrap().clone();
                match id {
                    Some(id) => db::insert_wave_skip(
                        conn,
                        &id,
                        *at_wave as i64,
                        *skipped_count as i64,
                        skip_multiplier.map(|n| n as i64),
                        *coin_per_minute,
                    ),
                    None => Ok(()),
                }
            }
            Action::EndRun {
                final_wave,
                peak_tier,
            } => {
                let id = current_run_id.lock().unwrap().take();
                match id {
                    Some(id) => db::end_run(
                        conn,
                        &id,
                        Some(*final_wave as i64),
                        peak_tier.map(|t| t as i64),
                    ),
                    None => Ok(()),
                }
            }
        };
        if let Err(e) = result {
            db::append_scanner_log(&format!("DB error applying {action:?}: {e}"));
        } else if matches!(action, Action::WaveSkip { .. }) {
            db::append_scanner_log(&format!("Recorded {action:?}"));
        }
    }
}

fn log_line(_dir: &std::path::Path, msg: &str) {
    db::append_scanner_log(msg);
}

fn emit(
    app: &AppHandle,
    status: &str,
    machine: &Arc<Mutex<RunStateMachine>>,
    current_run_id: &Arc<Mutex<Option<String>>>,
    cached_live: &Arc<Mutex<LiveState>>,
) {
    let live = machine.lock().unwrap().live_state();
    *cached_live.lock().unwrap() = live.clone();
    if let Some(notify) = app.try_state::<crate::notifications::NotifyState>() {
        notify.on_scanner_status(app, status);
    }
    crate::tray::update_scanner_ui(app, status, &live);
    let event = ScannerEvent {
        status: status.to_string(),
        live: Some(live),
        current_run_id: current_run_id.lock().unwrap().clone(),
    };
    app.emit("scanner-update", event).ok();
}

pub fn notify_scanner_actions(app: &AppHandle, actions: &[Action]) {
    if let Some(notify) = app.try_state::<crate::notifications::NotifyState>() {
        notify.on_actions(app, actions);
    }
}

fn sleep_remainder(tick: Instant, interval_ms: u64) {
    let elapsed = tick.elapsed();
    let interval = Duration::from_millis(interval_ms);
    if elapsed < interval {
        std::thread::sleep(interval - elapsed);
    }
}
