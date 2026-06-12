//! Tauri commands exposed to the frontend.

use base64::Engine;
use tauri::{AppHandle, State};

use crate::db::{self, RunFilter, RunRow, SnapshotRow};
use crate::fixture_capture::{self, CaptureEntry};
use crate::scanner::Scanner;
use crate::settings::Settings;
use crate::state_machine::LiveState;
use crate::{capture, scanner, settings};
use serde::Serialize;

pub struct AppState {
    pub scanner: Scanner,
}

fn conn() -> Result<rusqlite::Connection, String> {
    db::open().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_windows() -> Vec<capture::WindowInfo> {
    capture::list_windows()
}

#[tauri::command]
pub fn get_settings() -> Result<Settings, String> {
    Ok(settings::load(&conn()?))
}

#[tauri::command]
pub fn save_settings(new_settings: Settings) -> Result<(), String> {
    settings::save(&conn()?, &new_settings).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn start_scanner(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    state.scanner.start(app)
}

#[tauri::command]
pub fn stop_scanner(state: State<AppState>) {
    state.scanner.stop();
}

#[tauri::command]
pub fn scanner_running(state: State<AppState>) -> bool {
    state.scanner.is_running()
}

#[tauri::command]
pub fn live_state(state: State<AppState>) -> LiveState {
    state.scanner.machine.lock().unwrap().live_state()
}

#[tauri::command]
pub fn manual_new_run(state: State<AppState>) -> Result<(), String> {
    let actions = state.scanner.machine.lock().unwrap().manual_new_run();
    let c = conn()?;
    scanner::apply_actions(
        &c,
        &state.scanner.current_run_id,
        &actions,
        &db::app_data_dir().join("logs"),
    );
    Ok(())
}

#[tauri::command]
pub fn list_runs(filter: RunFilter) -> Result<Vec<RunRow>, String> {
    db::list_runs(&conn()?, &filter).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_run_comment(run_id: String, comment: String) -> Result<(), String> {
    db::set_run_comment(&conn()?, &run_id, &comment).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_runs(run_ids: Vec<String>, state: State<AppState>) -> Result<usize, String> {
    let n = db::delete_runs(&conn()?, &run_ids).map_err(|e| e.to_string())?;
    let mut current = state.scanner.current_run_id.lock().unwrap();
    if current
        .as_ref()
        .is_some_and(|id| run_ids.iter().any(|d| d == id))
    {
        *current = None;
    }
    Ok(n)
}

#[tauri::command]
pub fn run_snapshots(run_id: String) -> Result<Vec<SnapshotRow>, String> {
    db::run_snapshots(&conn()?, &run_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn current_run_snapshots(state: State<AppState>) -> Result<Vec<SnapshotRow>, String> {
    let id = state.scanner.current_run_id.lock().unwrap().clone();
    match id {
        Some(id) => db::run_snapshots(&conn()?, &id).map_err(|e| e.to_string()),
        None => Ok(Vec::new()),
    }
}

/// Export runs to CSV next to the database; returns the file path.
#[tauri::command]
pub fn export_csv() -> Result<String, String> {
    let csv = db::export_runs_csv(&conn()?).map_err(|e| e.to_string())?;
    let path = db::app_data_dir().join("runs_export.csv");
    std::fs::write(&path, csv).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[derive(Serialize)]
pub struct CaptureBurstResult {
    pub saved: usize,
    pub coin_rate_detected: usize,
    pub manifest_path: String,
    pub captured_dir: String,
}

/// Save one frame + OCR metadata to fixtures/captured/.
#[tauri::command]
pub fn capture_fixture_once() -> Result<CaptureEntry, String> {
    let conn = conn()?;
    let target = settings::resolve_target_window(&conn)?;
    fixture_capture::capture_once(&target.title_substring)
}

/// Burst-capture frames for the OCR regression corpus.
#[tauri::command]
pub fn capture_fixture_burst(count: usize, interval_ms: u64) -> Result<CaptureBurstResult, String> {
    let conn = conn()?;
    let target = settings::resolve_target_window(&conn)?;
    let count = count.clamp(1, 200);
    let interval_ms = interval_ms.clamp(100, 10_000);
    let entries = fixture_capture::capture_burst(&target.title_substring, count, interval_ms)?;
    let coin_rate_detected = entries
        .iter()
        .filter(|e| e.classified.coin_rate_detected)
        .count();
    Ok(CaptureBurstResult {
        saved: entries.len(),
        coin_rate_detected,
        manifest_path: fixture_capture::manifest_path().to_string_lossy().to_string(),
        captured_dir: fixture_capture::captured_dir().to_string_lossy().to_string(),
    })
}

/// Capture the configured window and return it as a base64 PNG for Settings preview.
#[tauri::command]
pub fn preview_capture() -> Result<String, String> {
    let cfg = settings::load(&conn()?);
    let target = cfg
        .target_window
        .ok_or("No target window configured")?;
    let img = capture::capture_by_title(&target.title_substring)
        .ok_or("Window not found or minimized")?;
    let mut bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )
    .map_err(|e| e.to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}
