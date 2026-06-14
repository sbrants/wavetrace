//! Tauri commands exposed to the frontend.

use base64::Engine;
use tauri::{AppHandle, State};

use crate::db::{self, RunFilter, RunRow, SnapshotRow};
use crate::export::{self, CsvExportPayload, WorkbookExportPayload};
use crate::fixture_capture::{self, CaptureEntry};
use crate::scanner::{ScanStartMode, Scanner};
use crate::settings::Settings;
use crate::state_machine::{GameMode, LiveState};
use crate::{capture, fields, scanner, settings};
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
    capture::clear_window_cache();
    settings::save(&conn()?, &new_settings).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn has_resumable_run(state: State<AppState>) -> Result<bool, String> {
    state.scanner.has_resumable_run()
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
pub fn live_state(state: State<AppState>) -> LiveState {
    state.scanner.cached_live_state()
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
pub fn delete_snapshot(snapshot_id: String) -> Result<(), String> {
    let conn = conn()?;
    match db::delete_snapshot(&conn, &snapshot_id).map_err(|e| e.to_string())? {
        Some(_) => Ok(()),
        None => Err("Snapshot not found".into()),
    }
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
    let (bytes, run_count, snapshot_count) =
        export::export_workbook_ods_bytes(&conn, &filter)?;
    Ok(WorkbookExportPayload {
        filename: export::workbook_ods_filename(),
        data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        run_count,
        snapshot_count,
    })
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

/// Tail the scanner log (last N lines, capped at 2 MiB from EOF).
#[tauri::command]
pub fn read_scanner_log(max_lines: usize) -> Result<ScannerLogView, String> {
    let path = db::scanner_log_path();
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
    let (content, log_tail_truncated) =
        read_file_tail_text(&path, MAX_SCANNER_LOG_TAIL_BYTES)?;
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

#[derive(Serialize)]
pub struct CaptureBurstResult {
    pub saved: usize,
    pub coin_rate_detected: usize,
    pub manifest_path: String,
    pub captured_dir: String,
}

/// Save one frame + OCR metadata to fixtures/captured/.
#[tauri::command]
pub async fn capture_fixture_once() -> Result<CaptureEntry, String> {
    tauri::async_runtime::spawn_blocking(capture_fixture_once_blocking)
        .await
        .map_err(|e| format!("capture task failed: {e}"))?
}

fn capture_fixture_once_blocking() -> Result<CaptureEntry, String> {
    let conn = conn()?;
    let target = settings::resolve_target_window(&conn)?;
    fixture_capture::capture_once(&target.title_substring, false)
}

/// Burst-capture frames for the OCR regression corpus.
#[tauri::command]
pub async fn capture_fixture_burst(count: usize, interval_ms: u64) -> Result<CaptureBurstResult, String> {
    let count = count.clamp(1, 200);
    let interval_ms = interval_ms.clamp(100, 10_000);
    tauri::async_runtime::spawn_blocking(move || {
        capture_fixture_burst_blocking(count, interval_ms)
    })
    .await
    .map_err(|e| format!("capture task failed: {e}"))?
}

fn capture_fixture_burst_blocking(count: usize, interval_ms: u64) -> Result<CaptureBurstResult, String> {
    let conn = conn()?;
    let target = settings::resolve_target_window(&conn)?;
    let entries = fixture_capture::capture_burst(&target.title_substring, count, interval_ms, false)?;
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

fn preview_thumbnail(img: &image::RgbaImage) -> image::RgbaImage {
    const MAX_W: u32 = 400;
    if img.width() <= MAX_W {
        return img.clone();
    }
    let scale = MAX_W as f32 / img.width() as f32;
    let h = ((img.height() as f32) * scale).round() as u32;
    image::imageops::resize(
        img,
        MAX_W,
        h.max(1),
        image::imageops::FilterType::Triangle,
    )
}

/// One-shot capture + OCR for Settings diagnostics (runs off the UI thread).
#[tauri::command]
pub async fn probe_ocr() -> Result<OcrProbeResult, String> {
    tauri::async_runtime::spawn_blocking(probe_ocr_blocking)
        .await
        .map_err(|e| format!("OCR probe task failed: {e}"))?
}

fn probe_ocr_blocking() -> Result<OcrProbeResult, String> {
    use std::time::Instant;

    let conn = conn()?;
    let target = settings::resolve_target_window(&conn)?;
    let started = Instant::now();
    let frame = capture::capture_by_title(&target.title_substring);
    let elapsed_ms = started.elapsed().as_millis() as u64;

    let Some(img) = frame else {
        return Ok(OcrProbeResult {
            window_found: false,
            target_substring: target.title_substring,
            width: 0,
            height: 0,
            elapsed_ms,
            all_lines: Vec::new(),
            coin_lines: Vec::new(),
            tier_wave_lines: Vec::new(),
            mode_lines: Vec::new(),
            tier: None,
            wave: None,
            coin_per_minute: None,
            coin_status: "window_not_found".into(),
            mode: "unknown".into(),
            preview_png_base64: None,
        });
    };

    let ocr_started = Instant::now();
    let fields = fields::ocr_probe_fields(&img)?;
    let ocr_ms = ocr_started.elapsed().as_millis() as u64;

    let input = fields::poll_input_from_fields(&fields);
    let coin_per_minute = match input.coin {
        crate::parser::CoinReading::Rate(v) => Some(v),
        _ => None,
    };
    let coin_status = match input.coin {
        crate::parser::CoinReading::Rate(_) => "rate",
        crate::parser::CoinReading::Total(_) => "total_balance",
        crate::parser::CoinReading::Unreadable => "unreadable",
    }
    .to_string();
    let mode = match input.mode {
        GameMode::Normal => "normal",
        GameMode::TotalCoin => "total_coin",
        GameMode::IntroSprint => "intro_sprint",
        GameMode::Tournament => "tournament",
        GameMode::EndOfRun => "end_of_run",
        GameMode::Unknown => "unknown",
    }
    .to_string();

    let thumb = preview_thumbnail(&img);
    let mut bytes = Vec::new();
    let preview_png_base64 = thumb
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .ok()
        .map(|_| base64::engine::general_purpose::STANDARD.encode(&bytes));

    Ok(OcrProbeResult {
        window_found: true,
        target_substring: target.title_substring,
        width: img.width(),
        height: img.height(),
        elapsed_ms: elapsed_ms + ocr_ms,
        all_lines: fields.all_lines.clone(),
        coin_lines: fields
            .all_lines
            .iter()
            .filter(|l| l.to_lowercase().contains("/min"))
            .cloned()
            .collect(),
        tier_wave_lines: fields.all_lines.clone(),
        mode_lines: Vec::new(),
        tier: input.tier,
        wave: input.wave,
        coin_per_minute,
        coin_status,
        mode,
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
    let target = cfg
        .target_window
        .ok_or("No target window configured")?;
    let img = capture::capture_by_title(&target.title_substring)
        .ok_or("Window not found or minimized")?;
    let thumb = preview_thumbnail(&img);
    let mut bytes = Vec::new();
    thumb
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| e.to_string())?;
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
        std::env::temp_dir().join(format!("towerrun_{name}_{}", std::process::id()))
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
