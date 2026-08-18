//! Tauri commands exposed to the frontend.

use base64::Engine;
use tauri::{AppHandle, Manager, State};

use crate::backup::{self, BackupExport, BackupRestore};
use crate::db::{self, RunFilter, RunRow, SnapshotRow, WaveSkipRow};
use crate::debug_package::{self, DebugPackageExport, DebugScreenshotInput};
use crate::export::{self, CsvExportPayload, WorkbookExportPayload};
use crate::scanner::{ScanStartMode, Scanner};
use crate::settings::Settings;
use crate::{accounts, capture, settings};
use serde::{Deserialize, Serialize};

pub struct AppState {
    pub scanner: Scanner,
    /// History run comparison is active (2+ runs) — wave-milestone ntfy should attach that view.
    pub compare_capture_active: std::sync::Mutex<bool>,
    pub pending_wave_milestone_ntfy: std::sync::Mutex<Option<PendingWaveMilestoneNtfy>>,
}

#[derive(Clone)]
pub struct PendingWaveMilestoneNtfy {
    pub title: String,
    pub body: String,
    pub game_png_base64: String,
}

fn conn() -> Result<rusqlite::Connection, String> {
    db::open().map_err(|e| e.to_string())
}

/// Run DB / ADB / capture work off the UI thread (Tauri sync commands block the main thread).
async fn run_blocking<T, F>(label: &'static str, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("{label} task failed: {e}"))?
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    crate::tray::exit_app(&app);
}

#[tauri::command]
pub async fn list_windows() -> Vec<capture::WindowInfo> {
    run_blocking("list_windows", || Ok(capture::list_windows()))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub fn screen_capture_access() -> capture::ScreenCaptureAccess {
    capture::screen_capture_access()
}

#[tauri::command]
pub fn request_screen_capture_access() -> capture::ScreenCaptureAccess {
    capture::request_screen_capture_access()
}

#[tauri::command]
pub fn open_screen_recording_settings() -> Result<(), String> {
    capture::open_screen_recording_settings()
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    let url = url.trim();
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Only http(s) URLs are supported".into());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn open_scanner_logs_folder() -> Result<(), String> {
    let logs_dir = db::app_data_dir().join("logs");
    std::fs::create_dir_all(&logs_dir).map_err(|e| e.to_string())?;
    let log_file = db::app_log_path();
    reveal_in_file_manager(if log_file.exists() {
        &log_file
    } else {
        &logs_dir
    })
}

fn windows_explorer_powershell(path_display: &str, is_file: bool) -> String {
    let path_str = path_display.replace('\'', "''");
    if is_file {
        format!("Start-Process explorer.exe -ArgumentList '/select,\"{path_str}\"'")
    } else {
        format!("Start-Process explorer.exe -ArgumentList '{path_str}'")
    }
}

#[cfg(windows)]
fn windows_explorer_launch_command(path: &std::path::Path) -> Result<String, String> {
    let path = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(windows_explorer_powershell(
        &path.display().to_string(),
        path.is_file(),
    ))
}

fn reveal_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let ps = windows_explorer_launch_command(path)?;
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        if path.is_file() {
            cmd.arg("-R");
        }
        cmd.arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "linux")]
    {
        let target = if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        std::process::Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        Err("Opening folders is not supported on this platform".into())
    }
}

#[tauri::command]
pub async fn get_settings() -> Result<Settings, String> {
    run_blocking("get_settings", || Ok(settings::load(&conn()?))).await
}

#[tauri::command]
pub fn save_settings(new_settings: Settings) -> Result<(), String> {
    capture::clear_window_cache();
    let conn = conn()?;
    let mut merged = new_settings;
    // Managed only via set_compare_capture_active; don't clear when saving other settings.
    merged.compare_capture_active = settings::load(&conn).compare_capture_active;
    settings::save(&conn, &merged).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn send_test_ntfy(app: AppHandle) -> Result<(), String> {
    crate::notifications::send_test_ntfy(&app)
}

#[tauri::command]
pub fn get_ntfy_status(state: State<crate::notifications::NotifyState>) -> crate::notifications::NtfyStatusInfo {
    state.ntfy_status()
}

#[tauri::command]
pub fn clear_ntfy_rate_limit(state: State<crate::notifications::NotifyState>) {
    state.clear_ntfy_rate_limit();
}

#[tauri::command]
pub fn set_compare_capture_active(
    state: State<AppState>,
    active: bool,
) -> Result<(), String> {
    *state.compare_capture_active.lock().unwrap() = active;
    let conn = conn()?;
    let mut settings = settings::load(&conn);
    settings.compare_capture_active = active;
    settings::save(&conn, &settings).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn focus_main_window(app: AppHandle) {
    crate::tray::show_main_window(&app);
}

#[tauri::command]
pub fn complete_wave_milestone_ntfy(
    app: AppHandle,
    ui_png_base64: Option<String>,
) -> Result<(), String> {
    let pending = app
        .state::<AppState>()
        .pending_wave_milestone_ntfy
        .lock()
        .unwrap()
        .take();
    let Some(pending) = pending else {
        return Err("no pending wave milestone notification".into());
    };

    let ui_png_base64 = if ui_png_base64
        .as_ref()
        .is_some_and(|s| !s.trim().is_empty())
    {
        let len = ui_png_base64.as_ref().map(|s| s.len()).unwrap_or(0);
        db::append_app_log(&format!("wave milestone ui capture: from webview ({len} b64 chars)"));
        ui_png_base64
    } else {
        crate::tray::show_main_window(&app);
        let captured = (0..5).find_map(|attempt| {
            let delay_ms = 400 + attempt * 400;
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            capture::capture_own_app_window(&app)
                .ok()
                .and_then(|img| {
                    db::append_app_log(&format!(
                        "wave milestone ui capture: screen crop {}x{} (attempt {})",
                        img.width(),
                        img.height(),
                        attempt + 1
                    ));
                    capture::encode_png_base64(&img).ok()
                })
        });
        if captured.is_none() {
            db::append_app_log("wave milestone ui capture: FAILED (webview + screen crop)");
        }
        captured
    };

    crate::notifications::publish_ntfy_wave_milestone_blocking(
        &app,
        pending.title,
        pending.body,
        pending.game_png_base64,
        ui_png_base64,
    )
}

#[tauri::command]
pub async fn has_resumable_run(state: State<'_, AppState>) -> Result<bool, String> {
    if state.scanner.machine.lock().unwrap().has_active_run() {
        return Ok(true);
    }
    run_blocking("has_resumable_run", || {
        Ok(db::latest_open_run(&conn()?)
            .map_err(|e| e.to_string())?
            .is_some())
    })
    .await
}

#[tauri::command]
pub fn start_scanner(
    app: AppHandle,
    state: State<AppState>,
    mode: ScanStartMode,
) -> Result<(), String> {
    state.scanner.start(app, mode)
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
pub async fn list_runs(filter: RunFilter) -> Result<Vec<RunRow>, String> {
    run_blocking("list_runs", move || {
        db::list_runs(&conn()?, &filter).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub fn set_run_comment(run_id: String, comment: String) -> Result<(), String> {
    db::set_run_comment(&conn()?, &run_id, &comment).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_run_type(run_id: String, run_type: String) -> Result<(), String> {
    db::set_run_type(&conn()?, &run_id, &run_type).map_err(|e| e.to_string())
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
pub fn combine_runs(run_ids: Vec<String>, state: State<AppState>) -> Result<String, String> {
    let new_id = db::combine_runs(&conn()?, &run_ids).map_err(|e| e.to_string())?;
    let mut current = state.scanner.current_run_id.lock().unwrap();
    if current
        .as_ref()
        .is_some_and(|id| run_ids.iter().any(|d| d == id))
    {
        *current = None;
    }
    Ok(new_id)
}

#[tauri::command]
pub fn delete_snapshots(snapshot_ids: Vec<String>) -> Result<usize, String> {
    let conn = conn()?;
    db::delete_snapshots(&conn, &snapshot_ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_wave_skips(wave_skip_ids: Vec<String>) -> Result<usize, String> {
    let conn = conn()?;
    db::delete_wave_skips(&conn, &wave_skip_ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_wave_skip(wave_skip_id: String) -> Result<(), String> {
    let conn = conn()?;
    if db::delete_wave_skip(&conn, &wave_skip_id).map_err(|e| e.to_string())? {
        Ok(())
    } else {
        Err("Wave skip not found".into())
    }
}

#[tauri::command]
pub fn delete_snapshot(snapshot_id: String) -> Result<(), String> {
    let conn = conn()?;
    match db::delete_snapshot(&conn, &snapshot_id).map_err(|e| e.to_string())? {
        Some(_) => Ok(()),
        None => Err("Snapshot not found".into()),
    }
}

#[derive(Debug, Deserialize)]
pub struct GoldenComboUpdate {
    pub chance: Option<f64>,
    pub caret: Option<i64>,
    pub multiplier: Option<f64>,
}

#[tauri::command]
pub fn update_snapshot_golden_combo(
    snapshot_id: String,
    update: GoldenComboUpdate,
) -> Result<(), String> {
    let conn = conn()?;
    if db::update_snapshot_golden_combo(
        &conn,
        &snapshot_id,
        update.chance,
        update.caret,
        update.multiplier,
    )
    .map_err(|e| e.to_string())?
    {
        Ok(())
    } else {
        Err("Snapshot not found".into())
    }
}

#[tauri::command]
pub fn clear_snapshot_golden_combo(snapshot_ids: Vec<String>) -> Result<usize, String> {
    let conn = conn()?;
    db::clear_snapshot_golden_combo(&conn, &snapshot_ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn run_snapshots(run_id: String) -> Result<Vec<SnapshotRow>, String> {
    run_blocking("run_snapshots", move || {
        db::run_snapshots(&conn()?, &run_id).map_err(|e| e.to_string())
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct DashboardRunView {
    pub snapshot_total: usize,
    pub chart_snapshots: Vec<SnapshotRow>,
    pub skip_total: usize,
    pub chart_wave_skips: Vec<WaveSkipRow>,
    pub chart_normal_jumps: Vec<i64>,
    pub avg_golden_combo_caret: Option<f64>,
}

fn dashboard_run_view(run_id: &str) -> Result<DashboardRunView, String> {
    let conn = conn()?;
    let (snapshot_total, chart_snapshots) =
        db::run_snapshots_for_chart(&conn, run_id, db::CHART_SNAPSHOT_LIMIT)
            .map_err(|e| e.to_string())?;
    let (skip_total, chart_wave_skips) =
        db::run_wave_skips_for_chart(&conn, run_id, db::CHART_SKIP_LIMIT)
            .map_err(|e| e.to_string())?;
    let chart_normal_jumps =
        db::chart_normal_jump_waves(&conn, run_id, db::CHART_SKIP_LIMIT)
            .map_err(|e| e.to_string())?;
    let avg_golden_combo_caret =
        db::avg_golden_combo_caret(&conn, run_id).map_err(|e| e.to_string())?;
    Ok(DashboardRunView {
        snapshot_total,
        chart_snapshots,
        skip_total,
        chart_wave_skips,
        chart_normal_jumps,
        avg_golden_combo_caret,
    })
}

#[tauri::command]
pub async fn current_run_dashboard(
    state: State<'_, AppState>,
) -> Result<DashboardRunView, String> {
    let id = state.scanner.current_run_id.lock().unwrap().clone();
    run_blocking("current_run_dashboard", move || match id {
        Some(id) => dashboard_run_view(&id),
        None => Ok(DashboardRunView {
            snapshot_total: 0,
            chart_snapshots: Vec::new(),
            skip_total: 0,
            chart_wave_skips: Vec::new(),
            chart_normal_jumps: Vec::new(),
            avg_golden_combo_caret: None,
        }),
    })
    .await
}

#[tauri::command]
pub async fn run_dashboard_data(run_id: String) -> Result<DashboardRunView, String> {
    run_blocking("run_dashboard_data", move || dashboard_run_view(&run_id)).await
}

#[tauri::command]
pub async fn run_wave_skips(run_id: String) -> Result<Vec<WaveSkipRow>, String> {
    run_blocking("run_wave_skips", move || {
        db::run_wave_skips(&conn()?, &run_id).map_err(|e| e.to_string())
    })
    .await
}

/// Export all snapshots (with run metadata) for browser download.
#[tauri::command]
pub fn export_csv(filter: RunFilter) -> Result<CsvExportPayload, String> {
    let conn = conn()?;
    let (content, run_count, snapshot_count) =
        export::export_snapshots_csv(&conn, &filter).map_err(|e| e.to_string())?;
    Ok(CsvExportPayload {
        filename: export::snapshots_csv_filename(),
        content,
        run_count,
        snapshot_count,
    })
}

/// Export runs workbook (ODS) for browser download.
#[tauri::command]
pub fn export_workbook(filter: RunFilter) -> Result<WorkbookExportPayload, String> {
    let conn = conn()?;
    let (bytes, run_count, snapshot_count) = export::export_workbook_ods_bytes(&conn, &filter)?;
    Ok(WorkbookExportPayload {
        filename: export::workbook_ods_filename(),
        data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        run_count,
        snapshot_count,
    })
}

fn ensure_scanner_stopped(state: &State<AppState>) -> Result<(), String> {
    if state.scanner.is_running() {
        return Err("Stop the scanner before backing up or restoring.".into());
    }
    Ok(())
}

fn reset_scanner_state(state: &State<AppState>) {
    state.scanner.reset_after_db_restore();
}

/// Full database backup as a zip for browser download.
#[tauri::command]
pub fn export_backup(state: State<AppState>) -> Result<BackupExport, String> {
    ensure_scanner_stopped(&state)?;
    let (bytes, manifest) = backup::create_backup_zip()?;
    Ok(BackupExport {
        filename: backup::backup_filename(),
        data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        run_count: manifest.run_count,
        snapshot_count: manifest.snapshot_count,
    })
}

/// Replace the local database from a backup zip (base64).
#[tauri::command]
pub fn restore_backup(state: State<AppState>, data_base64: String) -> Result<BackupRestore, String> {
    ensure_scanner_stopped(&state)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.trim())
        .map_err(|e| format!("Invalid backup data: {e}"))?;
    let result = backup::restore_backup_zip(&bytes)?;
    reset_scanner_state(&state);
    Ok(result)
}

#[derive(Serialize)]
pub struct AppDataInfo {
    pub app_data_dir: String,
    pub logs_dir: String,
    pub backups_dir: String,
    pub database_path: String,
    pub app_log_path: String,
    pub install_kind: String,
    pub account_id: String,
    pub account_name: String,
}

#[tauri::command]
pub async fn get_app_data_info() -> AppDataInfo {
    run_blocking("get_app_data_info", || {
        let app_data = db::app_data_dir();
        let account = crate::accounts::current_account();
        Ok(AppDataInfo {
            app_data_dir: app_data.to_string_lossy().into_owned(),
            logs_dir: app_data.join("logs").to_string_lossy().into_owned(),
            backups_dir: app_data.join("backups").to_string_lossy().into_owned(),
            database_path: db::database_path().to_string_lossy().into_owned(),
            app_log_path: db::app_log_path().to_string_lossy().into_owned(),
            install_kind: db::detect_install_kind(&db::app_data_root()).to_string(),
            account_id: account.id,
            account_name: account.name,
        })
    })
    .await
    .unwrap_or_else(|_| AppDataInfo {
        app_data_dir: String::new(),
        logs_dir: String::new(),
        backups_dir: String::new(),
        database_path: String::new(),
        app_log_path: String::new(),
        install_kind: String::new(),
        account_id: String::new(),
        account_name: String::new(),
    })
}

#[tauri::command]
pub async fn game_save_status() -> crate::adb_save::GameSaveStatus {
    run_blocking("game_save_status", || {
        let custom_port = conn()
            .ok()
            .and_then(|c| settings::load(&c).save_pull_adb_port);
        Ok(crate::adb_save::probe_game_save(custom_port, true))
    })
    .await
    .unwrap_or_else(|e| crate::adb_save::GameSaveStatus {
        ready: false,
        adb_path: None,
        device_serial: None,
        detail: e,
    })
}

#[tauri::command]
pub fn pull_game_save() -> Result<crate::adb_save::GameSavePullResult, String> {
    let result = crate::adb_save::pull_game_save_from_settings()?;
    if result.written {
        reveal_in_file_manager(std::path::Path::new(&result.path))?;
    }
    Ok(result)
}

#[tauri::command]
pub fn pick_save_pull_dir() -> Result<Option<String>, String> {
    crate::adb_save::pick_output_dir()
}

#[derive(Serialize)]
pub struct AccountList {
    pub accounts: Vec<accounts::Account>,
    pub current_id: String,
    pub colors: Vec<String>,
}

fn current_account_id() -> String {
    accounts::current_id().unwrap_or_else(|| accounts::DEFAULT_ACCOUNT_ID.into())
}

#[tauri::command]
pub fn list_accounts() -> AccountList {
    AccountList {
        accounts: accounts::list_accounts(),
        current_id: current_account_id(),
        colors: accounts::COLOR_PALETTE
            .iter()
            .map(|c| (*c).to_string())
            .collect(),
    }
}

#[tauri::command]
pub fn create_account(name: String, color: Option<String>) -> Result<accounts::Account, String> {
    accounts::create_account(&name, color.as_deref())
}

#[tauri::command]
pub fn update_account(
    app: AppHandle,
    id: String,
    name: Option<String>,
    color: Option<String>,
) -> Result<accounts::Account, String> {
    let updated = accounts::update_account(&id, name.as_deref(), color.as_deref())?;
    if current_account_id() == id {
        crate::app_icon::apply_branding_to(&app).ok();
        crate::tray::refresh_account_branding(&app);
    }
    Ok(updated)
}

#[tauri::command]
pub fn delete_account(id: String) -> Result<(), String> {
    accounts::delete_account(&id)
}

#[tauri::command]
pub fn switch_account(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    if current_account_id() == id {
        return Ok(());
    }
    if state.scanner.is_running() {
        return Err("Stop the scanner before switching accounts in this window.".into());
    }
    accounts::spawn_and_replace(&id)?;
    crate::tray::exit_app(&app);
    Ok(())
}

#[tauri::command]
pub fn open_account_window(id: String) -> Result<(), String> {
    accounts::spawn_instance(&id)
}

#[derive(Serialize)]
pub struct ScannerLogView {
    pub path: String,
    pub lines: Vec<String>,
    pub total_lines: usize,
    /// True when fewer than total_lines are returned (line or byte cap).
    pub truncated: bool,
    /// True when the log file exceeded the byte tail limit.
    pub log_tail_truncated: bool,
}

const MAX_SCANNER_LOG_TAIL_BYTES: usize = 2 * 1024 * 1024;
const MAX_CLIPBOARD_PNG_BYTES: usize = 8 * 1024 * 1024;

fn count_lines_in_file(path: &std::path::Path) -> Result<usize, String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 64 * 1024];
    let mut lines = 0usize;
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        lines += buf[..n].iter().filter(|b| **b == b'\n').count();
    }
    Ok(lines)
}

fn read_file_tail_text(path: &std::path::Path, max_bytes: usize) -> Result<(String, bool), String> {
    use std::io::{Read, Seek, SeekFrom};
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let len = meta.len() as usize;
    if len == 0 {
        return Ok((String::new(), false));
    }
    let tail_truncated = len > max_bytes;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let read_len = len.min(max_bytes);
    if tail_truncated {
        file.seek(SeekFrom::End(-(read_len as i64)))
            .map_err(|e| e.to_string())?;
    }
    let mut buf = vec![0u8; read_len];
    file.read_exact(&mut buf).map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if tail_truncated {
        if let Some(idx) = text.find('\n') {
            text = text[idx + 1..].to_string();
        } else {
            text.clear();
        }
    }
    Ok((text, tail_truncated))
}

/// Tail the app log (last N lines, capped at 2 MiB from EOF).
#[tauri::command]
pub fn read_scanner_log(max_lines: usize) -> Result<ScannerLogView, String> {
    let path = db::app_log_path();
    let path_str = path.to_string_lossy().to_string();
    if !path.exists() {
        return Ok(ScannerLogView {
            path: path_str,
            lines: Vec::new(),
            total_lines: 0,
            truncated: false,
            log_tail_truncated: false,
        });
    }

    let max_lines = max_lines.clamp(10, 2000);
    let total_lines = count_lines_in_file(&path)?;
    let (content, log_tail_truncated) = read_file_tail_text(&path, MAX_SCANNER_LOG_TAIL_BYTES)?;
    let all: Vec<&str> = content.lines().collect();
    let line_truncated = all.len() > max_lines;
    let start = all.len().saturating_sub(max_lines);
    let lines: Vec<String> = all[start..].iter().map(|s| (*s).to_string()).collect();
    Ok(ScannerLogView {
        path: path_str,
        truncated: line_truncated || log_tail_truncated,
        log_tail_truncated,
        total_lines,
        lines,
    })
}

#[tauri::command]
pub fn append_app_log(source: String, message: String) -> Result<(), String> {
    db::append_app_log(&format!("[UI:{source}] {message}"));
    Ok(())
}

#[tauri::command]
pub fn capture_app_window(app: AppHandle) -> Result<String, String> {
    crate::tray::show_main_window(&app);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let img = capture::capture_own_app_window(&app)?;
    capture::encode_png_base64(&img)
}

#[tauri::command]
pub fn generate_debug_package(
    screenshots: Vec<DebugScreenshotInput>,
    state: State<AppState>,
) -> Result<DebugPackageExport, String> {
    let input = debug_package::DebugPackageBuildInput {
        screenshots,
        scanner_running: state.scanner.is_running(),
        live: state.scanner.cached_live_state(),
        current_run_id: state.scanner.current_run_id.lock().unwrap().clone(),
        has_resumable_run: state.scanner.has_resumable_run().unwrap_or(false),
    };
    let export = debug_package::create_debug_package(&input)?;
    reveal_in_file_manager(std::path::Path::new(&export.path))?;
    Ok(export)
}

#[derive(Serialize)]
pub struct OcrProbeResult {
    pub window_found: bool,
    pub target_substring: String,
    pub width: u32,
    pub height: u32,
    pub elapsed_ms: u64,
    pub all_lines: Vec<String>,
    pub coin_lines: Vec<String>,
    pub tier_wave_lines: Vec<String>,
    pub mode_lines: Vec<String>,
    pub tier: Option<u32>,
    pub wave: Option<u32>,
    pub coin_per_minute: Option<f64>,
    pub coin_status: String,
    pub mode: String,
    pub preview_png_base64: Option<String>,
}

/// One-shot capture + OCR for Settings diagnostics (runs off the UI thread).
#[tauri::command]
pub async fn probe_ocr() -> Result<OcrProbeResult, String> {
    tauri::async_runtime::spawn_blocking(probe_ocr_blocking)
        .await
        .map_err(|e| format!("OCR probe task failed: {e}"))?
}

fn probe_ocr_blocking() -> Result<OcrProbeResult, String> {
    let bundle = crate::diagnostics::probe_target_window()?;
    let p = bundle.probe;
    let preview_png_base64 = bundle
        .preview_png
        .as_ref()
        .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes));
    Ok(OcrProbeResult {
        window_found: p.window_found,
        target_substring: p
            .resolved_target
            .as_ref()
            .map(|t| t.title_substring.clone())
            .unwrap_or_default(),
        width: p.width,
        height: p.height,
        elapsed_ms: p.capture_ms + p.ocr_ms,
        all_lines: bundle.all_lines.clone(),
        coin_lines: p.coin_lines,
        tier_wave_lines: bundle.all_lines,
        mode_lines: Vec::new(),
        tier: p.tier,
        wave: p.wave,
        coin_per_minute: p.coin_per_minute,
        coin_status: p.coin_status,
        mode: p.mode,
        preview_png_base64,
    })
}

/// Capture the configured window and return it as a base64 PNG for Settings preview.
#[tauri::command]
pub async fn preview_capture() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(preview_capture_blocking)
        .await
        .map_err(|e| format!("preview task failed: {e}"))?
}

fn preview_capture_blocking() -> Result<String, String> {
    let cfg = settings::load(&conn()?);
    let target = cfg.target_window.ok_or("No target window configured")?;
    let img = capture::capture_target(&target)
        .ok_or("Window not found or minimized")?;
    let bytes = crate::diagnostics::encode_preview_png(&img)
        .ok_or("Failed to encode preview image")?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[tauri::command]
pub fn copy_image_to_clipboard(png_base64: String) -> Result<(), String> {
    let trimmed = png_base64.trim();
    let est_bytes = trimmed.len().saturating_mul(3) / 4;
    if est_bytes > MAX_CLIPBOARD_PNG_BYTES {
        return Err(format!(
            "image too large ({est_bytes} bytes, max {MAX_CLIPBOARD_PNG_BYTES})"
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .map_err(|e| format!("invalid image data: {e}"))?;
    if bytes.len() > MAX_CLIPBOARD_PNG_BYTES {
        return Err(format!(
            "image too large ({} bytes, max {MAX_CLIPBOARD_PNG_BYTES})",
            bytes.len()
        ));
    }
    let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let width = rgba.width() as usize;
    let height = rgba.height() as usize;
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_image(arboard::ImageData {
            width,
            height,
            bytes: rgba.into_raw().into(),
        })
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod log_tail_tests {
    use super::{count_lines_in_file, read_file_tail_text};
    use std::io::Write;

    fn temp_log(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("wavetrace_{name}_{}", std::process::id()))
    }

    #[test]
    fn tail_reads_last_bytes_and_skips_partial_line() {
        let path = temp_log("small");
        let mut f = std::fs::File::create(&path).expect("create");
        write!(f, "line1\nline2\nline3\n").expect("write");
        let (text, truncated) = read_file_tail_text(&path, 64).expect("tail");
        assert!(!truncated);
        assert!(text.contains("line3"));
        assert_eq!(count_lines_in_file(&path).expect("count"), 3);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tail_truncates_large_files() {
        let path = temp_log("big");
        let mut f = std::fs::File::create(&path).expect("create");
        write!(f, "{}", "x".repeat(5000)).expect("pad");
        write!(f, "\nfinal\n").expect("tail line");
        let (text, truncated) = read_file_tail_text(&path, 100).expect("tail");
        assert!(truncated);
        assert!(text.contains("final"));
        assert!(!text.contains("xxxx"));
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod reveal_tests {
    use super::windows_explorer_powershell;

    #[test]
    fn explorer_command_quotes_file_paths_with_spaces() {
        let cmd = windows_explorer_powershell(r"C:\Users\Some User\logs\wavetrace.log", true);
        assert!(cmd.contains("/select,\"C:\\Users\\Some User\\logs\\wavetrace.log\""));
    }

    #[test]
    fn explorer_command_quotes_directory_paths_with_spaces() {
        let cmd = windows_explorer_powershell(r"C:\Users\Some User\AppData\wavetrace\logs", false);
        assert!(cmd.contains(r"'C:\Users\Some User\AppData\wavetrace\logs'"));
    }
}
