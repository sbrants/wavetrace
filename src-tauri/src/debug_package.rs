//! Zip bundle for support: manifest, latest app log, and UI screenshots.

use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;

use base64::Engine;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::db;

pub const FORMAT_VERSION: u32 = 1;
const MANIFEST_ENTRY: &str = "manifest.json";
const LOG_ENTRY: &str = "logs/wavetrace.log";
const SYSTEM_INFO_ENTRY: &str = "system-info.txt";

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
    pub screenshot_labels: Vec<String>,
    pub log_included: bool,
}

#[derive(Debug, Serialize)]
pub struct DebugPackageExport {
    pub filename: String,
    pub path: String,
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

fn read_latest_log() -> Option<Vec<u8>> {
    let path = db::app_log_path();
    if !path.exists() {
        return None;
    }
    let mut file = File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(bytes)
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

pub fn build_debug_package_zip(
    screenshots: &[DebugScreenshotInput],
) -> Result<(Vec<u8>, String), String> {
    if screenshots.is_empty() {
        return Err("At least one screenshot is required.".into());
    }

    let app_data = db::app_data_dir();
    let install_kind = db::detect_install_kind(&app_data).to_string();
    let log_bytes = read_latest_log();
    let screenshot_labels: Vec<String> = screenshots
        .iter()
        .map(|s| sanitize_label(&s.label))
        .collect();

    let manifest = DebugPackageManifest {
        format_version: FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: Utc::now().to_rfc3339(),
        install_kind: install_kind.clone(),
        os: std::env::consts::OS,
        screenshot_labels: screenshot_labels.clone(),
        log_included: log_bytes.is_some(),
    };

    let system_info = format!(
        "WaveTrace {}\nOS: {}\nInstall: {}\nCreated: {}\nLog path: {}\n",
        manifest.app_version,
        manifest.os,
        install_kind,
        manifest.created_at,
        db::app_log_path().display(),
    );

    let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    let mut zip_bytes = Vec::new();
    {
        let cursor = Cursor::new(&mut zip_bytes);
        let mut zip = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file(MANIFEST_ENTRY, options)
            .map_err(|e| e.to_string())?;
        zip.write_all(&manifest_json)
            .map_err(|e| e.to_string())?;

        zip.start_file(SYSTEM_INFO_ENTRY, options)
            .map_err(|e| e.to_string())?;
        zip.write_all(system_info.as_bytes())
            .map_err(|e| e.to_string())?;

        if let Some(bytes) = &log_bytes {
            zip.start_file(LOG_ENTRY, options)
                .map_err(|e| e.to_string())?;
            zip.write_all(bytes).map_err(|e| e.to_string())?;
        }

        for (shot, label) in screenshots.iter().zip(screenshot_labels.iter()) {
            let png = base64::engine::general_purpose::STANDARD
                .decode(shot.png_base64.trim())
                .map_err(|e| format!("Invalid screenshot {label}: {e}"))?;
            let entry = format!("screenshots/{label}.png");
            zip.start_file(&entry, options)
                .map_err(|e| e.to_string())?;
            zip.write_all(&png).map_err(|e| e.to_string())?;
        }

        zip.finish().map_err(|e| e.to_string())?;
    }

    Ok((zip_bytes, debug_package_filename()))
}

pub fn create_debug_package(
    screenshots: &[DebugScreenshotInput],
) -> Result<DebugPackageExport, String> {
    let (bytes, filename) = build_debug_package_zip(screenshots)?;
    let path = write_debug_package(&bytes, &filename)?;
    Ok(DebugPackageExport {
        filename,
        path: path.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_package_contains_manifest_log_and_screenshots() {
        let tiny_png = base64::engine::general_purpose::STANDARD.encode([
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ]);

        let shots = vec![
            DebugScreenshotInput {
                label: "dashboard".into(),
                png_base64: tiny_png.clone(),
            },
            DebugScreenshotInput {
                label: "settings".into(),
                png_base64: tiny_png,
            },
        ];

        let (bytes, _) = build_debug_package_zip(&shots).expect("zip");
        let cursor = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("archive");

        assert!(archive.by_name(MANIFEST_ENTRY).is_ok());
        assert!(archive.by_name(SYSTEM_INFO_ENTRY).is_ok());
        assert!(archive.by_name("screenshots/dashboard.png").is_ok());
        assert!(archive.by_name("screenshots/settings.png").is_ok());
    }
}
