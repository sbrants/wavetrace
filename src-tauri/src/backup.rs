//! Local backup bundle: `manifest.json` + `wavetrace.db` in a zip file.

use std::fs::{self, File};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zip::read::ZipArchive;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::db;

pub const FORMAT_VERSION: u32 = 1;
const DB_ENTRY: &str = "wavetrace.db";
const MANIFEST_ENTRY: &str = "manifest.json";

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupManifest {
    pub format_version: u32,
    pub app_version: String,
    pub created_at: String,
    pub run_count: i64,
    pub snapshot_count: i64,
}

#[derive(Debug, Serialize)]
pub struct BackupExport {
    pub filename: String,
    pub data_base64: String,
    pub run_count: i64,
    pub snapshot_count: i64,
}

#[derive(Debug, Serialize)]
pub struct BackupRestore {
    pub run_count: i64,
    pub snapshot_count: i64,
    pub safety_copy_path: Option<String>,
    pub backup_created_at: Option<String>,
    pub backup_app_version: Option<String>,
}

pub fn backup_filename() -> String {
    format!("wavetrace-backup-{}.zip", Utc::now().format("%Y-%m-%dT%H-%M-%S"))
}

fn count_rows(conn: &Connection) -> rusqlite::Result<(i64, i64)> {
    let runs: i64 = conn.query_row("SELECT COUNT(*) FROM runs", [], |r| r.get(0))?;
    let snapshots: i64 = conn.query_row("SELECT COUNT(*) FROM snapshots", [], |r| r.get(0))?;
    Ok((runs, snapshots))
}

fn vacuum_snapshot(conn: &Connection, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        fs::remove_file(dest).map_err(|e| e.to_string())?;
    }
    conn.execute(
        "VACUUM INTO ?",
        rusqlite::params![dest.to_string_lossy().as_ref()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn is_sqlite_file(path: &Path) -> Result<(), String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut header = [0u8; 16];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    if &header[..15] != b"SQLite format 3" {
        return Err("Backup database is not a valid SQLite file.".into());
    }
    Ok(())
}

pub fn create_backup_zip() -> Result<(Vec<u8>, BackupManifest), String> {
    let conn = db::open().map_err(|e| e.to_string())?;
    let (run_count, snapshot_count) = count_rows(&conn).map_err(|e| e.to_string())?;

    let manifest = BackupManifest {
        format_version: FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: Utc::now().to_rfc3339(),
        run_count,
        snapshot_count,
    };

    let tmp_db = db::app_data_dir().join(format!(".backup-{}.db", Uuid::new_v4()));
    vacuum_snapshot(&conn, &tmp_db)?;

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

        zip.start_file(DB_ENTRY, options)
            .map_err(|e| e.to_string())?;
        let mut db_file = File::open(&tmp_db).map_err(|e| e.to_string())?;
        std::io::copy(&mut db_file, &mut zip).map_err(|e| e.to_string())?;

        zip.finish().map_err(|e| e.to_string())?;
    }

    fs::remove_file(&tmp_db).ok();
    Ok((zip_bytes, manifest))
}

pub fn restore_backup_zip(bytes: &[u8]) -> Result<BackupRestore, String> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|e| format!("Invalid zip: {e}"))?;

    let manifest: BackupManifest = {
        let mut file = archive
            .by_name(MANIFEST_ENTRY)
            .map_err(|_| "Backup zip is missing manifest.json.".to_string())?;
        let mut json = String::new();
        file.read_to_string(&mut json).map_err(|e| e.to_string())?;
        serde_json::from_str(&json).map_err(|e| format!("Invalid manifest.json: {e}"))?
    };

    if manifest.format_version != FORMAT_VERSION {
        return Err(format!(
            "Unsupported backup format version {} (expected {FORMAT_VERSION}).",
            manifest.format_version
        ));
    }

    let tmp_extract = db::app_data_dir().join(format!(".restore-{}.db", Uuid::new_v4()));
    {
        let mut file = archive
            .by_name(DB_ENTRY)
            .map_err(|_| "Backup zip is missing wavetrace.db.".to_string())?;
        let mut out = File::create(&tmp_extract).map_err(|e| e.to_string())?;
        std::io::copy(&mut file, &mut out).map_err(|e| e.to_string())?;
    }

    is_sqlite_file(&tmp_extract)?;
    // Apply migrations on the extracted copy before swapping files in.
    {
        let conn = Connection::open(&tmp_extract).map_err(|e| e.to_string())?;
        db::migrate(&conn).map_err(|e| e.to_string())?;
    }

    let dest = db::database_path();
    let safety_copy_path = safety_copy_current_db(&dest)?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp_extract, &dest).map_err(|e| {
        if let Some(ref safety) = safety_copy_path {
            restore_safety_copy(&dest, safety);
        }
        e.to_string()
    })?;

    let conn = db::open().map_err(|e| e.to_string())?;
    let (run_count, snapshot_count) = count_rows(&conn).map_err(|e| e.to_string())?;

    Ok(BackupRestore {
        run_count,
        snapshot_count,
        safety_copy_path: safety_copy_path.map(|p| p.display().to_string()),
        backup_created_at: Some(manifest.created_at),
        backup_app_version: Some(manifest.app_version),
    })
}

fn safety_copy_current_db(dest: &Path) -> Result<Option<PathBuf>, String> {
    if !dest.exists() {
        return Ok(None);
    }
    let backups_dir = db::app_data_dir().join("backups");
    fs::create_dir_all(&backups_dir).map_err(|e| e.to_string())?;
    let stamp = Utc::now().format("%Y-%m-%dT%H-%M-%S");
    let safety = backups_dir.join(format!("wavetrace-pre-restore-{stamp}.db"));
    fs::copy(dest, &safety).map_err(|e| e.to_string())?;
    Ok(Some(safety))
}

fn restore_safety_copy(dest: &Path, safety: &Path) {
    if safety.exists() {
        let _ = fs::copy(safety, dest);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Cursor, Write};
    use std::fs::File;

    use chrono::Utc;
    use rusqlite::Connection;
    use uuid::Uuid;
    use zip::write::SimpleFileOptions;
    use zip::{ZipArchive, ZipWriter};

    use super::{
        count_rows, vacuum_snapshot, BackupManifest, DB_ENTRY, FORMAT_VERSION, MANIFEST_ENTRY,
    };
    use crate::db;

    #[test]
    fn backup_round_trip_in_temp_dir() {
        let base = std::env::temp_dir().join(format!("wavetrace-backup-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&base).unwrap();
        let db_path = base.join("wavetrace.db");

        let conn = Connection::open(&db_path).unwrap();
        db::migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO runs (id, started_at, run_type) VALUES ('r1', '2026-01-01', 'farming')",
            [],
        )
        .unwrap();

        // Point app data at our temp dir by writing through database_path override pattern:
        // use vacuum + zip helpers directly on this connection.
        let (run_count, snapshot_count) = count_rows(&conn).unwrap();
        assert_eq!(run_count, 1);

        let manifest = BackupManifest {
            format_version: FORMAT_VERSION,
            app_version: "test".into(),
            created_at: Utc::now().to_rfc3339(),
            run_count,
            snapshot_count,
        };
        let tmp_db = base.join("snapshot.db");
        vacuum_snapshot(&conn, &tmp_db).unwrap();

        let manifest_json = serde_json::to_vec_pretty(&manifest).unwrap();
        let mut zip_bytes = Vec::new();
        {
            let cursor = Cursor::new(&mut zip_bytes);
            let mut zip = ZipWriter::new(cursor);
            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file(MANIFEST_ENTRY, options).unwrap();
            zip.write_all(&manifest_json).unwrap();
            zip.start_file(DB_ENTRY, options).unwrap();
            let mut db_file = File::open(&tmp_db).unwrap();
            std::io::copy(&mut db_file, &mut zip).unwrap();
            zip.finish().unwrap();
        }

        let cursor = Cursor::new(&zip_bytes);
        let mut archive = ZipArchive::new(cursor).unwrap();
        let mut file = archive.by_name(DB_ENTRY).unwrap();
        let restored = base.join("restored.db");
        let mut out = File::create(&restored).unwrap();
        std::io::copy(&mut file, &mut out).unwrap();

        let restored_conn = Connection::open(&restored).unwrap();
        let (runs, _) = count_rows(&restored_conn).unwrap();
        assert_eq!(runs, 1);

        fs::remove_dir_all(base).ok();
    }
}
