//! Zip bundle for support: manifest, logs, settings, runtime state, and UI screenshots.

use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use base64::Engine;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::capture::{self, ScreenCaptureAccess};
use crate::db;
use crate::diagnostics;
use crate::settings::{self, Settings};
use crate::state_machine::LiveState;

pub const FORMAT_VERSION: u32 = 2;
const MANIFEST_ENTRY: &str = "manifest.json";
const SYSTEM_INFO_ENTRY: &str = "system-info.txt";
const SETTINGS_ENTRY: &str = "settings.json";
const RUNTIME_ENTRY: &str = "runtime.json";
const PATHS_ENTRY: &str = "paths.json";
const DATABASE_SUMMARY_ENTRY: &str = "database-summary.json";
const WINDOWS_ENTRY: &str = "windows.json";
const TARGET_WINDOW_ENTRY: &str = "target-window.json";
const TARGET_WINDOW_CAPTURE_ENTRY: &str = "target-window/capture.png";
const LOG_DIR: &str = "logs";
/// Tail cap per log file so support zips stay portable (users can have 80MB+ logs).
const MAX_LOG_TAIL_BYTES: usize = 8 * 1024 * 1024;
const MAX_LOG_FILES: usize = 3;

#[derive(Debug, Deserialize)]
pub struct DebugScreenshotInput {
    pub label: String,
    pub png_base64: String,
}

#[derive(Debug, Serialize)]
pub struct DebugPackageManifest {
    pub format_version: u32,
    pub app_version: String,
    pub created_at: String,
    pub install_kind: String,
    pub os: &'static str,
    pub arch: &'static str,
    pub screenshot_labels: Vec<String>,
    pub log_files: Vec<String>,
    pub settings_included: bool,
    pub runtime_included: bool,
    pub database_summary_included: bool,
    pub windows_included: bool,
    pub target_window_included: bool,
    pub target_window_capture_included: bool,
}

#[derive(Debug, Serialize)]
pub struct DebugPackageExport {
    pub filename: String,
    pub path: String,
}

#[derive(Debug)]
pub struct DebugPackageBuildInput {
    pub screenshots: Vec<DebugScreenshotInput>,
    pub scanner_running: bool,
    pub live: LiveState,
    pub current_run_id: Option<String>,
    pub has_resumable_run: bool,
}

#[derive(Debug, Serialize)]
struct DebugPaths {
    app_data_dir: String,
    logs_dir: String,
    backups_dir: String,
    database_path: String,
    app_log_path: String,
    install_kind: String,
}

#[derive(Debug, Serialize)]
struct DebugRuntime {
    scanner_running: bool,
    has_resumable_run: bool,
    current_run_id: Option<String>,
    live: LiveState,
    screen_capture_access: ScreenCaptureAccess,
}

#[derive(Debug, Serialize)]
struct DatabaseSummary {
    total_runs: usize,
    open_runs: usize,
    current_run_snapshot_count: Option<usize>,
    current_run_skip_count: Option<usize>,
    recent_runs: Vec<RecentRunSummary>,
}

#[derive(Debug, Serialize)]
struct RecentRunSummary {
    id: String,
    run_type: String,
    started_at: String,
    ended_at: Option<String>,
    final_wave: Option<i64>,
    peak_tier: Option<i64>,
    snapshot_count: i64,
    open: bool,
}

pub fn debug_package_filename() -> String {
    format!(
        "wavetrace-debug-{}.zip",
        Utc::now().format("%Y-%m-%dT%H-%M-%S")
    )
}

fn sanitize_label(label: &str) -> String {
    let s: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "screenshot".to_string()
    } else {
        s
    }
}

fn redact_settings(mut settings: Settings) -> Settings {
    if !settings.notify_ntfy_topic.is_empty() {
        settings.notify_ntfy_topic = "[redacted]".into();
    }
    settings
}

fn debug_paths() -> DebugPaths {
    let app_data = db::app_data_dir();
    DebugPaths {
        app_data_dir: app_data.to_string_lossy().into_owned(),
        logs_dir: app_data.join("logs").to_string_lossy().into_owned(),
        backups_dir: app_data.join("backups").to_string_lossy().into_owned(),
        database_path: db::database_path().to_string_lossy().into_owned(),
        app_log_path: db::app_log_path().to_string_lossy().into_owned(),
        install_kind: db::detect_install_kind(&app_data).to_string(),
    }
}

fn database_summary(current_run_id: Option<&str>) -> Result<DatabaseSummary, String> {
    let conn = db::open().map_err(|e| e.to_string())?;
    let runs = db::list_runs(&conn, &db::RunFilter::default()).map_err(|e| e.to_string())?;
    let open_runs = runs.iter().filter(|r| r.ended_at.is_none()).count();
    let recent_runs = runs
        .iter()
        .take(10)
        .map(|r| RecentRunSummary {
            id: r.id.clone(),
            run_type: r.run_type.clone(),
            started_at: r.started_at.clone(),
            ended_at: r.ended_at.clone(),
            final_wave: r.final_wave,
            peak_tier: r.peak_tier,
            snapshot_count: r.snapshot_count,
            open: r.ended_at.is_none(),
        })
        .collect();
    let (current_run_snapshot_count, current_run_skip_count) =
        if let Some(id) = current_run_id {
            (
                Some(db::snapshot_count(&conn, id).map_err(|e| e.to_string())?),
                Some(db::wave_skip_count(&conn, id).map_err(|e| e.to_string())?),
            )
        } else {
            (None, None)
        };
    Ok(DatabaseSummary {
        total_runs: runs.len(),
        open_runs,
        current_run_snapshot_count,
        current_run_skip_count,
        recent_runs,
    })
}

fn app_log_bundle_paths() -> Vec<PathBuf> {
    let logs_dir = db::app_data_dir().join("logs");
    let mut out = Vec::new();
    let current = db::app_log_path();
    if current.exists() {
        out.push(current);
    }
    for i in 1..MAX_LOG_FILES {
        let rotated = logs_dir.join(format!("wavetrace.log.{i}"));
        if rotated.exists() {
            out.push(rotated);
        }
    }
    out
}

fn read_log_tail(path: &Path, max_bytes: usize) -> Result<(Vec<u8>, bool), String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let len = meta.len() as usize;
    if len == 0 {
        return Ok((Vec::new(), false));
    }
    let tail_truncated = len > max_bytes;
    let read_len = len.min(max_bytes);
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    if tail_truncated {
        file.seek(SeekFrom::End(-(read_len as i64)))
            .map_err(|e| e.to_string())?;
    }
    let mut buf = vec![0u8; read_len];
    file.read_exact(&mut buf).map_err(|e| e.to_string())?;
    if tail_truncated {
        if let Some(idx) = buf.iter().position(|&b| b == b'\n') {
            buf = buf[idx + 1..].to_vec();
        }
    }
    Ok((buf, tail_truncated))
}

fn zip_entry<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    path: &str,
    bytes: &[u8],
    options: SimpleFileOptions,
) -> Result<(), String> {
    zip.start_file(path, options).map_err(|e| e.to_string())?;
    zip.write_all(bytes).map_err(|e| e.to_string())
}

fn zip_json<W: Write + Seek, T: Serialize>(
    zip: &mut ZipWriter<W>,
    path: &str,
    value: &T,
    options: SimpleFileOptions,
) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    zip_entry(zip, path, &bytes, options)
}

fn debug_package_output_dir() -> PathBuf {
    dirs::download_dir().unwrap_or_else(|| db::app_data_dir().join("debug-packages"))
}

pub fn write_debug_package(bytes: &[u8], filename: &str) -> Result<PathBuf, String> {
    let dir = debug_package_output_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(filename);
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn build_debug_package_zip(input: &DebugPackageBuildInput) -> Result<(Vec<u8>, String), String> {
    let screenshots = &input.screenshots;
    let paths = debug_paths();
    let install_kind = paths.install_kind.clone();
    let settings = redact_settings(settings::load(&db::open().map_err(|e| e.to_string())?));
    let runtime = DebugRuntime {
        scanner_running: input.scanner_running,
        has_resumable_run: input.has_resumable_run,
        current_run_id: input.current_run_id.clone(),
        live: input.live.clone(),
        screen_capture_access: capture::screen_capture_access(),
    };
    let db_summary = database_summary(input.current_run_id.as_deref())?;
    let windows = capture::list_windows();
    let target_window = diagnostics::probe_target_window()?;
    let target_window_capture_included = target_window.preview_png.is_some();

    let screenshot_labels: Vec<String> = screenshots
        .iter()
        .map(|s| sanitize_label(&s.label))
        .collect();

    let mut log_files = Vec::new();
    let mut log_payloads: Vec<(String, Vec<u8>)> = Vec::new();
    for path in app_log_bundle_paths() {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("wavetrace.log");
        let (bytes, truncated) = read_log_tail(&path, MAX_LOG_TAIL_BYTES)?;
        let entry = format!("{LOG_DIR}/{file_name}");
        if truncated {
            let mut prefixed = format!(
                "[WaveTrace debug package: log tail capped at {} MiB — see {} on disk for full file]\n",
                MAX_LOG_TAIL_BYTES / (1024 * 1024),
                path.display()
            )
            .into_bytes();
            prefixed.extend(bytes);
            log_payloads.push((entry.clone(), prefixed));
        } else {
            log_payloads.push((entry.clone(), bytes));
        }
        log_files.push(entry);
    }

    let manifest = DebugPackageManifest {
        format_version: FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: Utc::now().to_rfc3339(),
        install_kind: install_kind.clone(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        screenshot_labels: screenshot_labels.clone(),
        log_files: log_files.clone(),
        settings_included: true,
        runtime_included: true,
        database_summary_included: true,
        windows_included: true,
        target_window_included: true,
        target_window_capture_included,
    };

    let system_info = format!(
        "WaveTrace {}\nOS: {} {}\nInstall: {}\nCreated: {}\nApp data: {}\nDatabase: {}\nLog: {}\nScanner running: {}\nCurrent run: {}\n",
        manifest.app_version,
        manifest.os,
        manifest.arch,
        install_kind,
        manifest.created_at,
        paths.app_data_dir,
        paths.database_path,
        paths.app_log_path,
        input.scanner_running,
        input
            .current_run_id
            .as_deref()
            .unwrap_or("(none)"),
    );

    let mut zip_bytes = Vec::new();
    {
        let cursor = Cursor::new(&mut zip_bytes);
        let mut zip = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip_json(&mut zip, MANIFEST_ENTRY, &manifest, options)?;
        zip_entry(&mut zip, SYSTEM_INFO_ENTRY, system_info.as_bytes(), options)?;
        zip_json(&mut zip, SETTINGS_ENTRY, &settings, options)?;
        zip_json(&mut zip, RUNTIME_ENTRY, &runtime, options)?;
        zip_json(&mut zip, PATHS_ENTRY, &paths, options)?;
        zip_json(&mut zip, DATABASE_SUMMARY_ENTRY, &db_summary, options)?;
        zip_json(&mut zip, WINDOWS_ENTRY, &windows, options)?;
        zip_json(&mut zip, TARGET_WINDOW_ENTRY, &target_window.probe, options)?;
        if let Some(png) = &target_window.preview_png {
            zip_entry(&mut zip, TARGET_WINDOW_CAPTURE_ENTRY, png, options)?;
        }

        for (entry, bytes) in log_payloads {
            zip_entry(&mut zip, &entry, &bytes, options)?;
        }

        for (shot, label) in screenshots.iter().zip(screenshot_labels.iter()) {
            let png = base64::engine::general_purpose::STANDARD
                .decode(shot.png_base64.trim())
                .map_err(|e| format!("Invalid screenshot {label}: {e}"))?;
            let entry = format!("screenshots/{label}.png");
            zip_entry(&mut zip, &entry, &png, options)?;
        }

        zip.finish().map_err(|e| e.to_string())?;
    }

    Ok((zip_bytes, debug_package_filename()))
}

pub fn create_debug_package(input: &DebugPackageBuildInput) -> Result<DebugPackageExport, String> {
    let (bytes, filename) = build_debug_package_zip(input)?;
    let path = write_debug_package(&bytes, &filename)?;
    Ok(DebugPackageExport {
        filename,
        path: path.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_machine::LiveState;

    fn tiny_png_b64() -> String {
        base64::engine::general_purpose::STANDARD.encode([
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ])
    }

    fn sample_input(screenshots: Vec<DebugScreenshotInput>) -> DebugPackageBuildInput {
        DebugPackageBuildInput {
            screenshots,
            scanner_running: false,
            live: LiveState::idle(),
            current_run_id: None,
            has_resumable_run: false,
        }
    }

    #[test]
    fn debug_package_contains_manifest_log_and_screenshots() {
        let shots = vec![
            DebugScreenshotInput {
                label: "dashboard".into(),
                png_base64: tiny_png_b64(),
            },
            DebugScreenshotInput {
                label: "settings".into(),
                png_base64: tiny_png_b64(),
            },
        ];

        let (bytes, _) = build_debug_package_zip(&sample_input(shots)).expect("zip");
        let cursor = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("archive");

        assert!(archive.by_name(MANIFEST_ENTRY).is_ok());
        assert!(archive.by_name(SYSTEM_INFO_ENTRY).is_ok());
        assert!(archive.by_name(SETTINGS_ENTRY).is_ok());
        assert!(archive.by_name(RUNTIME_ENTRY).is_ok());
        assert!(archive.by_name(PATHS_ENTRY).is_ok());
        assert!(archive.by_name(DATABASE_SUMMARY_ENTRY).is_ok());
        assert!(archive.by_name(WINDOWS_ENTRY).is_ok());
        assert!(archive.by_name(TARGET_WINDOW_ENTRY).is_ok());
        assert!(archive.by_name("screenshots/dashboard.png").is_ok());
        assert!(archive.by_name("screenshots/settings.png").is_ok());
    }

    #[test]
    fn debug_package_without_screenshots_still_builds() {
        let (bytes, _) = build_debug_package_zip(&sample_input(vec![])).expect("zip");
        let cursor = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("archive");
        assert!(archive.by_name(MANIFEST_ENTRY).is_ok());
        assert!(archive.by_name(SETTINGS_ENTRY).is_ok());
    }

    #[test]
    fn redact_settings_hides_ntfy_topic() {
        let mut settings = Settings::default();
        settings.notify_ntfy_enabled = true;
        settings.notify_ntfy_topic = "my-secret-topic".into();
        let redacted = redact_settings(settings);
        assert_eq!(redacted.notify_ntfy_topic, "[redacted]");
        assert!(redacted.notify_ntfy_enabled);
    }
}
