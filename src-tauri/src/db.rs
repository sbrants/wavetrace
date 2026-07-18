//! SQLite layer per Goal.md "Data model (SQLite MVP)".

use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub struct Db(pub Mutex<Connection>);

#[derive(Debug, Serialize)]
pub struct RunRow {
    pub id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub run_type: String,
    pub peak_tier: Option<i64>,
    pub final_wave: Option<i64>,
    pub avg_coin_per_minute: Option<f64>,
    pub snapshot_count: i64,
    pub comment: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct SnapshotRow {
    pub id: String,
    pub wave: i64,
    pub tier: Option<i64>,
    pub coin_per_minute: Option<f64>,
    pub recorded_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct WaveSkipRow {
    pub id: String,
    pub at_wave: i64,
    pub skipped_count: i64,
    pub skip_multiplier: Option<i64>,
    pub coin_per_minute: Option<f64>,
    pub recorded_at: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct RunFilter {
    pub run_type: Option<String>,
    pub min_wave: Option<i64>,
    pub min_tier: Option<i64>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

pub fn app_data_dir() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    let new_dir = base.join("wavetrace");
    if !new_dir.exists() {
        for legacy in ["wavewatch", "towerrun"] {
            let old_dir = base.join(legacy);
            if old_dir.exists() {
                if std::fs::rename(&old_dir, &new_dir).is_err() {
                    std::fs::create_dir_all(&old_dir).ok();
                    return old_dir;
                }
                break;
            }
        }
    }
    std::fs::create_dir_all(&new_dir).ok();
    new_dir
}

pub fn database_path() -> PathBuf {
    let dir = app_data_dir();
    let preferred = dir.join("wavetrace.db");
    for candidate in ["wavetrace.db", "wavewatch.db", "towerrun.db"] {
        let path = dir.join(candidate);
        if path.exists() {
            return path;
        }
    }
    preferred
}

const APP_LOG_BASENAME: &str = "wavetrace.log";
const LEGACY_LOG_BASENAME: &str = "scanner.log";

pub fn app_log_path() -> PathBuf {
    let logs_dir = app_data_dir().join("logs");
    migrate_legacy_app_log(&logs_dir);
    logs_dir.join(APP_LOG_BASENAME)
}

/// Renames `scanner.log` (+ rotated segments) to `wavetrace.log` on first access.
fn migrate_legacy_app_log(logs_dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(logs_dir);
    let legacy = logs_dir.join(LEGACY_LOG_BASENAME);
    let current = logs_dir.join(APP_LOG_BASENAME);
    if !current.exists() && legacy.exists() {
        let _ = std::fs::rename(&legacy, &current);
    }
    for i in 1..=APP_LOG_ROTATED_FILES {
        let legacy_rot = logs_dir.join(format!("{LEGACY_LOG_BASENAME}.{i}"));
        let current_rot = logs_dir.join(format!("{APP_LOG_BASENAME}.{i}"));
        if legacy_rot.exists() && !current_rot.exists() {
            let _ = std::fs::rename(&legacy_rot, &current_rot);
        }
    }
}

/// Per-file size before rotating to `wavetrace.log.1`, `wavetrace.log.2`, …
const APP_LOG_MAX_BYTES: u64 = 20 * 1024 * 1024;
/// `wavetrace.log` plus this many rotated segments (`wavetrace.log.1` …).
const APP_LOG_ROTATED_FILES: u32 = 9;

fn maybe_rotate_app_log(logs_dir: &std::path::Path) {
    maybe_rotate_app_log_with_limits(logs_dir, APP_LOG_MAX_BYTES, APP_LOG_ROTATED_FILES);
}

fn maybe_rotate_app_log_with_limits(
    logs_dir: &std::path::Path,
    max_bytes: u64,
    max_rotated: u32,
) {
    let path = logs_dir.join(APP_LOG_BASENAME);
    let Ok(meta) = std::fs::metadata(&path) else {
        return;
    };
    if meta.len() <= max_bytes {
        return;
    }

    let oldest = logs_dir.join(format!("{APP_LOG_BASENAME}.{max_rotated}"));
    let _ = std::fs::remove_file(&oldest);

    for i in (1..max_rotated).rev() {
        let from = logs_dir.join(format!("{APP_LOG_BASENAME}.{i}"));
        let to = logs_dir.join(format!("{APP_LOG_BASENAME}.{}", i + 1));
        if from.exists() {
            let _ = std::fs::remove_file(&to);
            let _ = std::fs::rename(&from, &to);
        }
    }

    let first = logs_dir.join(format!("{APP_LOG_BASENAME}.1"));
    let _ = std::fs::remove_file(&first);
    let _ = std::fs::rename(&path, &first);
}

pub fn append_app_log(msg: &str) {
    use std::io::Write;

    let logs_dir = app_data_dir().join("logs");
    migrate_legacy_app_log(&logs_dir);
    let _ = std::fs::create_dir_all(&logs_dir);
    maybe_rotate_app_log(&logs_dir);
    let path = logs_dir.join(APP_LOG_BASENAME);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        writeln!(f, "{} {msg}", chrono::Utc::now().to_rfc3339()).ok();
    }
}

/// How the app was installed, inferred from the resolved app data path.
pub fn detect_install_kind(app_data_dir: &std::path::Path) -> &'static str {
    let path = app_data_dir.to_string_lossy();
    #[cfg(target_os = "windows")]
    {
        if path.contains("\\Packages\\") && path.contains("\\LocalCache\\Roaming") {
            return "microsoft_store";
        }
        return "windows_direct";
    }
    #[cfg(target_os = "macos")]
    {
        if path.contains("/Containers/") {
            return "mac_sandbox";
        }
        return "mac_direct";
    }
    #[cfg(target_os = "linux")]
    {
        if path.contains("/.var/app/") {
            return "linux_flatpak";
        }
        return "linux_direct";
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        "unknown"
    }
}

pub fn open() -> rusqlite::Result<Connection> {
    let conn = Connection::open(database_path())?;
    migrate(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS runs (
            id          TEXT PRIMARY KEY,
            started_at  TEXT NOT NULL,
            ended_at    TEXT,
            run_type    TEXT NOT NULL DEFAULT 'farming',
            peak_tier   INTEGER,
            final_wave  INTEGER,
            comment     TEXT
        );
        CREATE TABLE IF NOT EXISTS snapshots (
            id              TEXT PRIMARY KEY,
            run_id          TEXT NOT NULL REFERENCES runs(id),
            wave            INTEGER NOT NULL,
            tier            INTEGER,
            coin_per_minute REAL,
            recorded_at     TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_snapshots_run ON snapshots(run_id);
        CREATE TABLE IF NOT EXISTS wave_skips (
            id              TEXT PRIMARY KEY,
            run_id          TEXT NOT NULL REFERENCES runs(id),
            at_wave         INTEGER NOT NULL,
            skipped_count   INTEGER NOT NULL,
            coin_per_minute REAL,
            recorded_at     TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_wave_skips_run ON wave_skips(run_id);
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;
    let _ = conn.execute("ALTER TABLE runs ADD COLUMN comment TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE wave_skips ADD COLUMN skip_multiplier INTEGER",
        [],
    );
    conn.execute(
        "UPDATE runs SET run_type = 'farming' WHERE run_type = 'normal'",
        [],
    )?;
    Ok(())
}

/// Close every run that still has no `ended_at` (at most one should be open).
/// Uses snapshot aggregates when explicit final stats were not supplied.
pub fn end_open_runs(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("SELECT id FROM runs WHERE ended_at IS NULL")?;
    let ids: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for id in ids {
        let (final_wave, peak_tier) = snapshot_stats(conn, &id)?;
        end_run(conn, &id, final_wave, peak_tier)?;
    }
    Ok(())
}

pub(crate) fn snapshot_stats(
    conn: &Connection,
    run_id: &str,
) -> rusqlite::Result<(Option<i64>, Option<i64>)> {
    conn.query_row(
        "SELECT MAX(wave), MAX(tier) FROM snapshots WHERE run_id = ?1",
        params![run_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
}

/// Open run to resume after stopping the scanner (most recent if several).
pub fn latest_open_run(conn: &Connection) -> rusqlite::Result<Option<(String, String)>> {
    conn.query_row(
        "SELECT id, run_type FROM runs WHERE ended_at IS NULL ORDER BY started_at DESC LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(other),
    })
}

pub fn start_run(conn: &Connection, run_type: &str) -> rusqlite::Result<String> {
    end_open_runs(conn)?;
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO runs (id, started_at, run_type) VALUES (?1, ?2, ?3)",
        params![id, Utc::now().to_rfc3339(), run_type],
    )?;
    Ok(id)
}

pub fn end_run(
    conn: &Connection,
    run_id: &str,
    final_wave: Option<i64>,
    peak_tier: Option<i64>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE runs SET ended_at = ?2, final_wave = ?3, peak_tier = ?4 WHERE id = ?1",
        params![run_id, Utc::now().to_rfc3339(), final_wave, peak_tier],
    )?;
    Ok(())
}

pub fn insert_snapshot(
    conn: &Connection,
    run_id: &str,
    wave: i64,
    tier: Option<i64>,
    coin_per_minute: Option<f64>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO snapshots (id, run_id, wave, tier, coin_per_minute, recorded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            Uuid::new_v4().to_string(),
            run_id,
            wave,
            tier,
            coin_per_minute,
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(())
}

pub fn insert_wave_skip(
    conn: &Connection,
    run_id: &str,
    at_wave: i64,
    skipped_count: i64,
    skip_multiplier: Option<i64>,
    coin_per_minute: Option<f64>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO wave_skips (id, run_id, at_wave, skipped_count, skip_multiplier, coin_per_minute, recorded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            Uuid::new_v4().to_string(),
            run_id,
            at_wave,
            skipped_count,
            skip_multiplier,
            coin_per_minute,
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(())
}

pub fn list_runs(conn: &Connection, filter: &RunFilter) -> rusqlite::Result<Vec<RunRow>> {
    let mut sql = String::from(
        "SELECT r.id, r.started_at, r.ended_at, r.run_type, r.peak_tier, r.final_wave,
                (SELECT AVG(coin_per_minute) FROM snapshots s WHERE s.run_id = r.id),
                (SELECT COUNT(*) FROM snapshots s WHERE s.run_id = r.id),
                r.comment
         FROM runs r WHERE 1=1",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(rt) = &filter.run_type {
        sql.push_str(" AND r.run_type = ?");
        args.push(Box::new(rt.clone()));
    }
    if let Some(w) = filter.min_wave {
        sql.push_str(" AND r.final_wave >= ?");
        args.push(Box::new(w));
    }
    if let Some(t) = filter.min_tier {
        sql.push_str(" AND r.peak_tier >= ?");
        args.push(Box::new(t));
    }
    if let Some(d) = &filter.date_from {
        sql.push_str(" AND r.started_at >= ?");
        args.push(Box::new(d.clone()));
    }
    if let Some(d) = &filter.date_to {
        sql.push_str(" AND r.started_at <= ?");
        args.push(Box::new(d.clone()));
    }
    sql.push_str(" ORDER BY r.started_at DESC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(args.iter().map(|a| a.as_ref())),
        |row| {
            Ok(RunRow {
                id: row.get(0)?,
                started_at: row.get(1)?,
                ended_at: row.get(2)?,
                run_type: row.get(3)?,
                peak_tier: row.get(4)?,
                final_wave: row.get(5)?,
                avg_coin_per_minute: row.get(6)?,
                snapshot_count: row.get(7)?,
                comment: row.get(8)?,
            })
        },
    )?;
    rows.collect()
}

pub fn set_run_comment(conn: &Connection, run_id: &str, comment: &str) -> rusqlite::Result<()> {
    let trimmed = comment.trim();
    let value: Option<&str> = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    };
    conn.execute(
        "UPDATE runs SET comment = ?2 WHERE id = ?1",
        params![run_id, value],
    )?;
    Ok(())
}

pub fn set_run_type(conn: &Connection, run_id: &str, run_type: &str) -> rusqlite::Result<()> {
    if crate::state_machine::RunType::try_from_db_str(run_type).is_none() {
        return Err(rusqlite::Error::InvalidParameterName(run_type.to_string()));
    }
    let updated = conn.execute(
        "UPDATE runs SET run_type = ?2 WHERE id = ?1",
        params![run_id, run_type],
    )?;
    if updated == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CombineRunsError {
    TooFewRuns,
    RunNotFound(String),
    OpenRun(String),
    WavesNotStrictlyIncreasing {
        prev_wave: i64,
        wave: i64,
        run_index: usize,
    },
    NoSnapshots,
}

impl std::fmt::Display for CombineRunsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooFewRuns => write!(f, "Select at least two runs to combine"),
            Self::RunNotFound(id) => write!(f, "Run not found: {id}"),
            Self::OpenRun(id) => write!(f, "Cannot combine an ongoing run: {id}"),
            Self::NoSnapshots => write!(f, "Selected runs have no snapshots to combine"),
            Self::WavesNotStrictlyIncreasing {
                prev_wave,
                wave,
                run_index,
            } => write!(
                f,
                "Waves must be strictly increasing when runs are ordered by start time \
                 (run {run_index}: wave {wave} follows wave {prev_wave})"
            ),
        }
    }
}

struct SourceRun {
    id: String,
    started_at: String,
    ended_at: String,
    run_type: String,
    comment: Option<String>,
}

fn merge_run_comments(comments: &[Option<String>]) -> Option<String> {
    let parts: Vec<String> = comments
        .iter()
        .filter_map(|c| {
            c.as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        })
        .collect();
    match parts.len() {
        0 => None,
        1 => Some(parts[0].clone()),
        _ => Some(parts.join(" · ")),
    }
}

fn combine_run_type(source_runs: &[SourceRun]) -> String {
    if source_runs.iter().any(|r| r.run_type == "tournament") {
        return "tournament".to_string();
    }
    let mut types: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for run in source_runs {
        types.insert(run.run_type.as_str());
    }
    if types.len() == 1 {
        return source_runs[0].run_type.clone();
    }
    let dissonance: Vec<&str> = source_runs
        .iter()
        .map(|r| r.run_type.as_str())
        .filter(|t| t.starts_with("dissonance_"))
        .collect();
    if !dissonance.is_empty() && dissonance.iter().all(|t| *t == dissonance[0]) {
        return dissonance[0].to_string();
    }
    "farming".to_string()
}

/// Merge multiple ended runs into one when their snapshot waves form a strictly
/// increasing sequence (runs ordered by `started_at`, snapshots by `wave` within each run).
/// Source runs are deleted; returns the new run id.
pub fn combine_runs(conn: &Connection, run_ids: &[String]) -> Result<String, CombineRunsError> {
    let mut unique_ids: Vec<String> = Vec::new();
    for id in run_ids {
        if !unique_ids.iter().any(|existing| existing == id) {
            unique_ids.push(id.clone());
        }
    }
    if unique_ids.len() < 2 {
        return Err(CombineRunsError::TooFewRuns);
    }

    let mut source_runs: Vec<SourceRun> = Vec::with_capacity(unique_ids.len());
    for id in &unique_ids {
        let row = conn
            .query_row(
                "SELECT id, started_at, ended_at, run_type, comment FROM runs WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => CombineRunsError::RunNotFound(id.clone()),
                _ => CombineRunsError::RunNotFound(id.clone()),
            })?;
        let ended_at = row.2.ok_or_else(|| CombineRunsError::OpenRun(id.clone()))?;
        source_runs.push(SourceRun {
            id: row.0,
            started_at: row.1,
            ended_at,
            run_type: row.3,
            comment: row.4,
        });
    }

    source_runs.sort_by(|a, b| a.started_at.cmp(&b.started_at));

    let mut combined_snapshots: Vec<SnapshotRow> = Vec::new();
    for (run_index, run) in source_runs.iter().enumerate() {
        let snaps = run_snapshots(conn, &run.id)
            .map_err(|_| CombineRunsError::RunNotFound(run.id.clone()))?;
        for snap in snaps {
            if let Some(prev) = combined_snapshots.last() {
                if snap.wave <= prev.wave {
                    return Err(CombineRunsError::WavesNotStrictlyIncreasing {
                        prev_wave: prev.wave,
                        wave: snap.wave,
                        run_index: run_index + 1,
                    });
                }
            }
            combined_snapshots.push(snap);
        }
    }

    if combined_snapshots.is_empty() {
        return Err(CombineRunsError::NoSnapshots);
    }

    let started_at = source_runs.first().unwrap().started_at.clone();
    let ended_at = source_runs
        .iter()
        .map(|r| r.ended_at.as_str())
        .max()
        .unwrap()
        .to_string();
    let run_type = combine_run_type(&source_runs);
    let peak_tier = combined_snapshots.iter().filter_map(|s| s.tier).max();
    let final_wave = combined_snapshots.last().map(|s| s.wave);
    let comment = merge_run_comments(
        &source_runs
            .iter()
            .map(|r| r.comment.clone())
            .collect::<Vec<_>>(),
    );

    let new_id = Uuid::new_v4().to_string();
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| CombineRunsError::RunNotFound(e.to_string()))?;

    tx.execute(
        "INSERT INTO runs (id, started_at, ended_at, run_type, peak_tier, final_wave, comment)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![new_id, started_at, ended_at, run_type, peak_tier, final_wave, comment],
    )
    .map_err(|e| CombineRunsError::RunNotFound(e.to_string()))?;

    for snap in &combined_snapshots {
        tx.execute(
            "INSERT INTO snapshots (id, run_id, wave, tier, coin_per_minute, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                Uuid::new_v4().to_string(),
                new_id,
                snap.wave,
                snap.tier,
                snap.coin_per_minute,
                snap.recorded_at,
            ],
        )
        .map_err(|e| CombineRunsError::RunNotFound(e.to_string()))?;
    }

    for run in &source_runs {
        let skips = run_wave_skips(&tx, &run.id)
            .map_err(|e| CombineRunsError::RunNotFound(e.to_string()))?;
        for skip in skips {
            tx.execute(
                "INSERT INTO wave_skips (id, run_id, at_wave, skipped_count, skip_multiplier, coin_per_minute, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    Uuid::new_v4().to_string(),
                    new_id,
                    skip.at_wave,
                    skip.skipped_count,
                    skip.skip_multiplier,
                    skip.coin_per_minute,
                    skip.recorded_at,
                ],
            )
            .map_err(|e| CombineRunsError::RunNotFound(e.to_string()))?;
        }
    }

    for run in &source_runs {
        tx.execute("DELETE FROM wave_skips WHERE run_id = ?1", params![run.id])
            .map_err(|e| CombineRunsError::RunNotFound(e.to_string()))?;
        tx.execute("DELETE FROM snapshots WHERE run_id = ?1", params![run.id])
            .map_err(|e| CombineRunsError::RunNotFound(e.to_string()))?;
        tx.execute("DELETE FROM runs WHERE id = ?1", params![run.id])
            .map_err(|e| CombineRunsError::RunNotFound(e.to_string()))?;
    }

    tx.commit()
        .map_err(|e| CombineRunsError::RunNotFound(e.to_string()))?;

    Ok(new_id)
}

pub fn delete_runs(conn: &Connection, run_ids: &[String]) -> rusqlite::Result<usize> {
    let mut deleted = 0usize;
    for id in run_ids {
        conn.execute("DELETE FROM wave_skips WHERE run_id = ?1", params![id])?;
        conn.execute("DELETE FROM snapshots WHERE run_id = ?1", params![id])?;
        let n = conn.execute("DELETE FROM runs WHERE id = ?1", params![id])?;
        deleted += n;
    }
    Ok(deleted)
}

pub fn delete_snapshots(conn: &Connection, snapshot_ids: &[String]) -> rusqlite::Result<usize> {
    use std::collections::HashSet;

    if snapshot_ids.is_empty() {
        return Ok(0);
    }

    let mut run_ids = HashSet::new();
    for id in snapshot_ids {
        if let Ok(run_id) = conn.query_row(
            "SELECT run_id FROM snapshots WHERE id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        ) {
            run_ids.insert(run_id);
        }
    }

    let mut deleted = 0usize;
    for id in snapshot_ids {
        deleted += conn.execute("DELETE FROM snapshots WHERE id = ?1", params![id])?;
    }

    for run_id in run_ids {
        sync_run_snapshot_stats(conn, &run_id)?;
    }

    Ok(deleted)
}

pub fn delete_snapshot(conn: &Connection, snapshot_id: &str) -> rusqlite::Result<Option<String>> {
    let run_id: Option<String> = conn
        .query_row(
            "SELECT run_id FROM snapshots WHERE id = ?1",
            params![snapshot_id],
            |row| row.get(0),
        )
        .ok();
    let deleted = delete_snapshots(conn, &[snapshot_id.to_string()])?;
    if deleted == 0 {
        Ok(None)
    } else {
        Ok(run_id)
    }
}

fn sync_run_snapshot_stats(conn: &Connection, run_id: &str) -> rusqlite::Result<()> {
    let (final_wave, peak_tier) = snapshot_stats(conn, run_id)?;
    conn.execute(
        "UPDATE runs SET final_wave = ?2, peak_tier = ?3 WHERE id = ?1",
        params![run_id, final_wave, peak_tier],
    )?;
    Ok(())
}

pub fn run_snapshots(conn: &Connection, run_id: &str) -> rusqlite::Result<Vec<SnapshotRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, wave, tier, coin_per_minute, recorded_at
         FROM snapshots WHERE run_id = ?1 ORDER BY wave ASC",
    )?;
    let rows = stmt.query_map(params![run_id], |row| {
        Ok(SnapshotRow {
            id: row.get(0)?,
            wave: row.get(1)?,
            tier: row.get(2)?,
            coin_per_minute: row.get(3)?,
            recorded_at: row.get(4)?,
        })
    })?;
    rows.collect()
}

/// Max snapshots sent to the UI chart (avoids WebView OOM on long runs).
/// Keep in sync with `MAX_CHART_POINTS` in `src/chartData.ts`.
pub const CHART_SNAPSHOT_LIMIT: usize = 5000;

/// Max skip markers sent to the UI chart (sampled independently of snapshots).
/// Keep in sync with `MAX_CHART_SKIPS` in `src/chartData.ts`.
pub const CHART_SKIP_LIMIT: usize = 5000;

pub fn snapshot_count(conn: &Connection, run_id: &str) -> rusqlite::Result<usize> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM snapshots WHERE run_id = ?1",
        params![run_id],
        |row| row.get(0),
    )?;
    Ok(count.max(0) as usize)
}

pub fn wave_skip_count(conn: &Connection, run_id: &str) -> rusqlite::Result<usize> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM wave_skips WHERE run_id = ?1",
        params![run_id],
        |row| row.get(0),
    )?;
    Ok(count.max(0) as usize)
}

fn downsample_slice<T: Clone>(items: &[T], max_points: usize) -> Vec<T> {
    if items.len() <= max_points || max_points <= 1 {
        return items.to_vec();
    }
    let last = items.len() - 1;
    (0..max_points)
        .map(|i| {
            let idx = (i * last) / (max_points - 1);
            items[idx].clone()
        })
        .collect()
}

pub fn downsample_snapshot_rows(
    rows: &[SnapshotRow],
    max_points: usize,
) -> Vec<SnapshotRow> {
    downsample_slice(rows, max_points)
}

pub fn downsample_wave_skip_rows(
    rows: &[WaveSkipRow],
    max_points: usize,
) -> Vec<WaveSkipRow> {
    downsample_slice(rows, max_points)
}

pub fn run_snapshots_for_chart(
    conn: &Connection,
    run_id: &str,
    max_points: usize,
) -> rusqlite::Result<(usize, Vec<SnapshotRow>)> {
    let all = run_snapshots(conn, run_id)?;
    let total = all.len();
    Ok((total, downsample_snapshot_rows(&all, max_points)))
}

pub fn run_wave_skips_for_chart(
    conn: &Connection,
    run_id: &str,
    max_points: usize,
) -> rusqlite::Result<(usize, Vec<WaveSkipRow>)> {
    let all = run_wave_skips(conn, run_id)?;
    let total = all.len();
    Ok((total, downsample_wave_skip_rows(&all, max_points)))
}

/// +1 wave jumps from the full snapshot series, downsampled for the chart axis.
pub fn chart_normal_jump_waves(
    conn: &Connection,
    run_id: &str,
    max_points: usize,
) -> rusqlite::Result<Vec<i64>> {
    let all = run_snapshots(conn, run_id)?;
    let jumps: Vec<i64> = all
        .windows(2)
        .filter(|w| {
            let prev = w[0].wave;
            let wave = w[1].wave;
            wave > prev && wave - prev == 1
        })
        .map(|w| w[1].wave)
        .collect();
    Ok(downsample_slice(&jumps, max_points))
}

pub fn run_wave_skips(conn: &Connection, run_id: &str) -> rusqlite::Result<Vec<WaveSkipRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, at_wave, skipped_count, skip_multiplier, coin_per_minute, recorded_at
         FROM wave_skips WHERE run_id = ?1 ORDER BY at_wave ASC, recorded_at ASC",
    )?;
    let rows = stmt.query_map(params![run_id], |row| {
        Ok(WaveSkipRow {
            id: row.get(0)?,
            at_wave: row.get(1)?,
            skipped_count: row.get(2)?,
            skip_multiplier: row.get(3)?,
            coin_per_minute: row.get(4)?,
            recorded_at: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn delete_wave_skips(conn: &Connection, wave_skip_ids: &[String]) -> rusqlite::Result<usize> {
    if wave_skip_ids.is_empty() {
        return Ok(0);
    }
    let mut deleted = 0usize;
    for id in wave_skip_ids {
        deleted += conn.execute("DELETE FROM wave_skips WHERE id = ?1", params![id])?;
    }
    Ok(deleted)
}

pub fn delete_wave_skip(conn: &Connection, wave_skip_id: &str) -> rusqlite::Result<bool> {
    Ok(delete_wave_skips(conn, &[wave_skip_id.to_string()])? > 0)
}

pub fn get_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(other),
    })
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsample_snapshot_rows_keeps_endpoints() {
        let total = CHART_SNAPSHOT_LIMIT * 2;
        let rows: Vec<SnapshotRow> = (0..total as i64)
            .map(|i| SnapshotRow {
                id: format!("s{i}"),
                wave: i,
                tier: Some(1),
                coin_per_minute: Some(i as f64),
                recorded_at: "t".into(),
            })
            .collect();
        let out = downsample_snapshot_rows(&rows, CHART_SNAPSHOT_LIMIT);
        assert_eq!(out.len(), CHART_SNAPSHOT_LIMIT);
        assert_eq!(out.first().unwrap().wave, 0);
        assert_eq!(out.last().unwrap().wave, total as i64 - 1);
    }

    #[test]
    fn downsample_wave_skip_rows_keeps_endpoints() {
        let total = CHART_SKIP_LIMIT * 2;
        let rows: Vec<WaveSkipRow> = (0..total as i64)
            .map(|i| WaveSkipRow {
                id: format!("w{i}"),
                at_wave: i * 10,
                skipped_count: 2,
                skip_multiplier: None,
                coin_per_minute: None,
                recorded_at: "t".into(),
            })
            .collect();
        let out = downsample_wave_skip_rows(&rows, CHART_SKIP_LIMIT);
        assert_eq!(out.len(), CHART_SKIP_LIMIT);
        assert_eq!(out.first().unwrap().at_wave, 0);
        assert_eq!(out.last().unwrap().at_wave, (total as i64 - 1) * 10);
    }

    #[test]
    fn latest_open_run_returns_most_recent_open() {
        let conn = open_in_memory().unwrap();
        assert!(latest_open_run(&conn).unwrap().is_none());

        let id1 = start_run(&conn, "farming").unwrap();
        end_run(&conn, &id1, Some(3), Some(10)).unwrap();
        let id2 = start_run(&conn, "tournament").unwrap();

        let open = latest_open_run(&conn).unwrap().expect("open run");
        assert_eq!(open.0, id2);
        assert_eq!(open.1, "tournament");
    }

    #[test]
    fn delete_snapshots_batch_recomputes_run_stats() {
        let conn = open_in_memory().unwrap();
        let run_id = start_run(&conn, "farming").unwrap();
        insert_snapshot(&conn, &run_id, 1, Some(10), Some(100.0)).unwrap();
        insert_snapshot(&conn, &run_id, 2, Some(11), Some(200.0)).unwrap();
        insert_snapshot(&conn, &run_id, 3, Some(11), Some(300.0)).unwrap();
        end_run(&conn, &run_id, Some(3), Some(11)).unwrap();

        let snaps = run_snapshots(&conn, &run_id).unwrap();
        let ids: Vec<String> = snaps
            .iter()
            .filter(|s| s.wave == 1 || s.wave == 3)
            .map(|s| s.id.clone())
            .collect();
        assert_eq!(delete_snapshots(&conn, &ids).unwrap(), 2);

        let runs = list_runs(&conn, &RunFilter::default()).unwrap();
        assert_eq!(runs[0].snapshot_count, 1);
        assert_eq!(runs[0].final_wave, Some(2));
        assert_eq!(runs[0].avg_coin_per_minute, Some(200.0));
    }

    #[test]
    fn delete_snapshot_recomputes_run_stats() {
        let conn = open_in_memory().unwrap();
        let run_id = start_run(&conn, "farming").unwrap();
        insert_snapshot(&conn, &run_id, 1, Some(10), Some(100.0)).unwrap();
        insert_snapshot(&conn, &run_id, 2, Some(11), Some(200.0)).unwrap();
        insert_snapshot(&conn, &run_id, 5, Some(12), Some(300.0)).unwrap();
        end_run(&conn, &run_id, Some(5), Some(12)).unwrap();

        let outlier = run_snapshots(&conn, &run_id)
            .unwrap()
            .into_iter()
            .find(|s| s.wave == 2)
            .expect("wave 2");
        delete_snapshot(&conn, &outlier.id).unwrap();

        let runs = list_runs(&conn, &RunFilter::default()).unwrap();
        assert_eq!(runs[0].snapshot_count, 2);
        assert_eq!(runs[0].final_wave, Some(5));
        assert_eq!(runs[0].peak_tier, Some(12));
        assert_eq!(runs[0].avg_coin_per_minute, Some(200.0));

        let snaps = run_snapshots(&conn, &run_id).unwrap();
        assert_eq!(snaps.len(), 2);
        assert!(snaps.iter().all(|s| s.wave != 2));
    }

    #[test]
    fn start_run_ends_previous_open_run() {
        let conn = open_in_memory().unwrap();
        let id1 = start_run(&conn, "farming").unwrap();
        insert_snapshot(&conn, &id1, 1, Some(10), Some(100.0)).unwrap();
        insert_snapshot(&conn, &id1, 5, Some(12), Some(200.0)).unwrap();

        let id2 = start_run(&conn, "farming").unwrap();
        assert_ne!(id1, id2);

        let runs = list_runs(&conn, &RunFilter::default()).unwrap();
        let run1 = runs.iter().find(|r| r.id == id1).unwrap();
        let run2 = runs.iter().find(|r| r.id == id2).unwrap();
        assert!(run1.ended_at.is_some());
        assert_eq!(run1.final_wave, Some(5));
        assert_eq!(run1.peak_tier, Some(12));
        assert!(run2.ended_at.is_none());
    }

    #[test]
    fn run_lifecycle_roundtrip() {
        let conn = open_in_memory().unwrap();
        let id = start_run(&conn, "tournament").unwrap();
        insert_snapshot(&conn, &id, 1, Some(17), None).unwrap();
        insert_snapshot(&conn, &id, 2, Some(17), Some(500.0)).unwrap();
        end_run(&conn, &id, Some(2), Some(17)).unwrap();

        let runs = list_runs(&conn, &RunFilter::default()).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run_type, "tournament");
        assert_eq!(runs[0].final_wave, Some(2));
        assert_eq!(runs[0].snapshot_count, 2);

        let filtered = list_runs(
            &conn,
            &RunFilter {
                run_type: Some("farming".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(filtered.is_empty());

        let snaps = run_snapshots(&conn, &id).unwrap();
        assert_eq!(snaps.len(), 2);
        assert_eq!(snaps[1].coin_per_minute, Some(500.0));
    }

    #[test]
    fn delete_runs_removes_snapshots() {
        let conn = open_in_memory().unwrap();
        let id = start_run(&conn, "farming").unwrap();
        insert_snapshot(&conn, &id, 1, Some(10), Some(100.0)).unwrap();
        delete_runs(&conn, &[id.clone()]).unwrap();
        assert!(list_runs(&conn, &RunFilter::default()).unwrap().is_empty());
        assert!(run_snapshots(&conn, &id).unwrap().is_empty());
    }

    #[test]
    fn run_comment_roundtrip() {
        let conn = open_in_memory().unwrap();
        let id = start_run(&conn, "farming").unwrap();
        set_run_comment(&conn, &id, "  good run  ").unwrap();
        let runs = list_runs(&conn, &RunFilter::default()).unwrap();
        assert_eq!(runs[0].comment.as_deref(), Some("good run"));
        set_run_comment(&conn, &id, "").unwrap();
        let runs = list_runs(&conn, &RunFilter::default()).unwrap();
        assert_eq!(runs[0].comment, None);
    }

    #[test]
    fn run_type_roundtrip() {
        let conn = open_in_memory().unwrap();
        let id = start_run(&conn, "farming").unwrap();
        set_run_type(&conn, &id, "dissonance_utility").unwrap();
        let runs = list_runs(&conn, &RunFilter::default()).unwrap();
        assert_eq!(runs[0].run_type, "dissonance_utility");
        assert!(set_run_type(&conn, &id, "invalid").is_err());
    }

    #[test]
    fn list_runs_filters_by_date_range() {
        let conn = open_in_memory().unwrap();
        let old = start_run(&conn, "farming").unwrap();
        conn.execute(
            "UPDATE runs SET started_at = '2026-01-15T12:00:00Z' WHERE id = ?1",
            params![old],
        )
        .unwrap();
        end_run(&conn, &old, Some(10), Some(5)).unwrap();

        let recent = start_run(&conn, "farming").unwrap();
        conn.execute(
            "UPDATE runs SET started_at = '2026-06-10T08:30:00Z' WHERE id = ?1",
            params![recent],
        )
        .unwrap();
        end_run(&conn, &recent, Some(20), Some(8)).unwrap();

        let june = list_runs(
            &conn,
            &RunFilter {
                date_from: Some("2026-06-01T00:00:00Z".into()),
                date_to: Some("2026-06-30T23:59:59Z".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(june.len(), 1);
        assert_eq!(june[0].id, recent);

        let jan = list_runs(
            &conn,
            &RunFilter {
                date_from: Some("2026-01-01T00:00:00Z".into()),
                date_to: Some("2026-01-31T23:59:59Z".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(jan.len(), 1);
        assert_eq!(jan[0].id, old);
    }

    #[test]
    fn settings_roundtrip() {
        let conn = open_in_memory().unwrap();
        assert_eq!(get_setting(&conn, "poll_interval_ms").unwrap(), None);
        set_setting(&conn, "poll_interval_ms", "1000").unwrap();
        set_setting(&conn, "poll_interval_ms", "500").unwrap();
        assert_eq!(
            get_setting(&conn, "poll_interval_ms").unwrap(),
            Some("500".into())
        );
    }

    fn insert_closed_run(
        conn: &Connection,
        started_at: &str,
        ended_at: &str,
        run_type: &str,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO runs (id, started_at, ended_at, run_type) VALUES (?1, ?2, ?3, ?4)",
            params![id, started_at, ended_at, run_type],
        )
        .unwrap();
        id
    }

    fn insert_snapshot_at(
        conn: &Connection,
        run_id: &str,
        wave: i64,
        tier: Option<i64>,
        coin: Option<f64>,
        recorded_at: &str,
    ) {
        conn.execute(
            "INSERT INTO snapshots (id, run_id, wave, tier, coin_per_minute, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                Uuid::new_v4().to_string(),
                run_id,
                wave,
                tier,
                coin,
                recorded_at
            ],
        )
        .unwrap();
    }

    #[test]
    fn combine_runs_merges_increasing_waves() {
        let conn = open_in_memory().unwrap();
        let id1 = insert_closed_run(
            &conn,
            "2024-01-01T10:00:00Z",
            "2024-01-01T10:30:00Z",
            "farming",
        );
        insert_snapshot_at(
            &conn,
            &id1,
            1,
            Some(10),
            Some(100.0),
            "2024-01-01T10:01:00Z",
        );
        insert_snapshot_at(
            &conn,
            &id1,
            2,
            Some(10),
            Some(120.0),
            "2024-01-01T10:02:00Z",
        );

        let id2 = insert_closed_run(
            &conn,
            "2024-01-01T11:00:00Z",
            "2024-01-01T11:30:00Z",
            "farming",
        );
        insert_snapshot_at(
            &conn,
            &id2,
            3,
            Some(11),
            Some(150.0),
            "2024-01-01T11:01:00Z",
        );
        insert_snapshot_at(
            &conn,
            &id2,
            4,
            Some(11),
            Some(160.0),
            "2024-01-01T11:02:00Z",
        );

        let combined_id = combine_runs(&conn, &[id2.clone(), id1.clone()]).unwrap();
        assert_ne!(combined_id, id1);
        assert_ne!(combined_id, id2);

        let runs = list_runs(&conn, &RunFilter::default()).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, combined_id);
        assert_eq!(runs[0].started_at, "2024-01-01T10:00:00Z");
        assert_eq!(runs[0].ended_at.as_deref(), Some("2024-01-01T11:30:00Z"));
        assert_eq!(runs[0].peak_tier, Some(11));
        assert_eq!(runs[0].final_wave, Some(4));
        assert_eq!(runs[0].snapshot_count, 4);

        let snaps = run_snapshots(&conn, &combined_id).unwrap();
        assert_eq!(snaps.len(), 4);
        assert_eq!(snaps[0].wave, 1);
        assert_eq!(snaps[3].wave, 4);
        assert_eq!(snaps[3].coin_per_minute, Some(160.0));
    }

    #[test]
    fn combine_runs_rejects_wave_reset() {
        let conn = open_in_memory().unwrap();
        let id1 = insert_closed_run(
            &conn,
            "2024-01-01T10:00:00Z",
            "2024-01-01T10:30:00Z",
            "farming",
        );
        insert_snapshot_at(
            &conn,
            &id1,
            1,
            Some(10),
            Some(100.0),
            "2024-01-01T10:01:00Z",
        );
        insert_snapshot_at(
            &conn,
            &id1,
            50,
            Some(12),
            Some(200.0),
            "2024-01-01T10:02:00Z",
        );

        let id2 = insert_closed_run(
            &conn,
            "2024-01-01T11:00:00Z",
            "2024-01-01T11:30:00Z",
            "farming",
        );
        insert_snapshot_at(
            &conn,
            &id2,
            1,
            Some(10),
            Some(100.0),
            "2024-01-01T11:01:00Z",
        );

        let err = combine_runs(&conn, &[id1, id2]).unwrap_err();
        assert!(matches!(
            err,
            CombineRunsError::WavesNotStrictlyIncreasing { .. }
        ));
        assert_eq!(list_runs(&conn, &RunFilter::default()).unwrap().len(), 2);
    }

    #[test]
    fn combine_runs_rejects_duplicate_waves() {
        let conn = open_in_memory().unwrap();
        let id1 = insert_closed_run(
            &conn,
            "2024-01-01T10:00:00Z",
            "2024-01-01T10:30:00Z",
            "farming",
        );
        insert_snapshot_at(
            &conn,
            &id1,
            1,
            Some(10),
            Some(100.0),
            "2024-01-01T10:01:00Z",
        );
        insert_snapshot_at(
            &conn,
            &id1,
            5,
            Some(10),
            Some(120.0),
            "2024-01-01T10:02:00Z",
        );

        let id2 = insert_closed_run(
            &conn,
            "2024-01-01T11:00:00Z",
            "2024-01-01T11:30:00Z",
            "farming",
        );
        insert_snapshot_at(
            &conn,
            &id2,
            5,
            Some(11),
            Some(150.0),
            "2024-01-01T11:01:00Z",
        );

        assert!(matches!(
            combine_runs(&conn, &[id1, id2]).unwrap_err(),
            CombineRunsError::WavesNotStrictlyIncreasing { .. }
        ));
    }

    #[test]
    fn combine_runs_rejects_open_run() {
        let conn = open_in_memory().unwrap();
        let id1 = start_run(&conn, "farming").unwrap();
        insert_snapshot(&conn, &id1, 1, Some(10), Some(100.0)).unwrap();
        let id2 = insert_closed_run(
            &conn,
            "2024-01-01T11:00:00Z",
            "2024-01-01T11:30:00Z",
            "farming",
        );
        insert_snapshot_at(
            &conn,
            &id2,
            2,
            Some(10),
            Some(120.0),
            "2024-01-01T11:01:00Z",
        );

        assert!(matches!(
            combine_runs(&conn, &[id1, id2]).unwrap_err(),
            CombineRunsError::OpenRun(_)
        ));
    }

    #[test]
    fn combine_runs_uses_tournament_if_any_source_is_tournament() {
        let conn = open_in_memory().unwrap();
        let id1 = insert_closed_run(
            &conn,
            "2024-01-01T10:00:00Z",
            "2024-01-01T10:30:00Z",
            "farming",
        );
        insert_snapshot_at(&conn, &id1, 1, Some(17), None, "2024-01-01T10:01:00Z");

        let id2 = insert_closed_run(
            &conn,
            "2024-01-01T11:00:00Z",
            "2024-01-01T11:30:00Z",
            "tournament",
        );
        insert_snapshot_at(
            &conn,
            &id2,
            2,
            Some(17),
            Some(500.0),
            "2024-01-01T11:01:00Z",
        );

        let combined_id = combine_runs(&conn, &[id1, id2]).unwrap();
        let runs = list_runs(&conn, &RunFilter::default()).unwrap();
        assert_eq!(runs[0].run_type, "tournament");
        assert_eq!(run_snapshots(&conn, &combined_id).unwrap().len(), 2);
    }

    #[test]
    fn combine_runs_keeps_single_comment() {
        let conn = open_in_memory().unwrap();
        let id1 = insert_closed_run(
            &conn,
            "2024-01-01T10:00:00Z",
            "2024-01-01T10:30:00Z",
            "farming",
        );
        insert_snapshot_at(
            &conn,
            &id1,
            1,
            Some(10),
            Some(100.0),
            "2024-01-01T10:01:00Z",
        );
        set_run_comment(&conn, &id1, "morning farm").unwrap();

        let id2 = insert_closed_run(
            &conn,
            "2024-01-01T11:00:00Z",
            "2024-01-01T11:30:00Z",
            "farming",
        );
        insert_snapshot_at(
            &conn,
            &id2,
            2,
            Some(11),
            Some(150.0),
            "2024-01-01T11:01:00Z",
        );

        let combined_id = combine_runs(&conn, &[id1, id2]).unwrap();
        let runs = list_runs(&conn, &RunFilter::default()).unwrap();
        assert_eq!(runs[0].id, combined_id);
        assert_eq!(runs[0].comment.as_deref(), Some("morning farm"));
    }

    #[test]
    fn combine_runs_merges_two_comments_in_start_order() {
        let conn = open_in_memory().unwrap();
        let id1 = insert_closed_run(
            &conn,
            "2024-01-01T10:00:00Z",
            "2024-01-01T10:30:00Z",
            "farming",
        );
        insert_snapshot_at(
            &conn,
            &id1,
            1,
            Some(10),
            Some(100.0),
            "2024-01-01T10:01:00Z",
        );
        set_run_comment(&conn, &id1, "first half").unwrap();

        let id2 = insert_closed_run(
            &conn,
            "2024-01-01T11:00:00Z",
            "2024-01-01T11:30:00Z",
            "farming",
        );
        insert_snapshot_at(
            &conn,
            &id2,
            2,
            Some(11),
            Some(150.0),
            "2024-01-01T11:01:00Z",
        );
        set_run_comment(&conn, &id2, "second half").unwrap();

        let combined_id = combine_runs(&conn, &[id2, id1]).unwrap();
        let runs = list_runs(&conn, &RunFilter::default()).unwrap();
        assert_eq!(runs[0].id, combined_id);
        assert_eq!(runs[0].comment.as_deref(), Some("first half · second half"));
    }

    #[test]
    fn app_log_rotation_keeps_numbered_segments() {
        use std::io::Write;

        let dir = std::env::temp_dir().join(format!(
            "wavetrace_log_rotate_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let write_chunk = |name: &str, label: &str| {
            let path = dir.join(name);
            let mut f = std::fs::File::create(&path).unwrap();
            write!(f, "{label}{}", "x".repeat(1100)).unwrap();
        };

        write_chunk("wavetrace.log", "active-");
        maybe_rotate_app_log_with_limits(&dir, 1000, 2);
        assert!(!dir.join("wavetrace.log").exists());
        assert!(dir.join("wavetrace.log.1").exists());

        write_chunk("wavetrace.log", "active2-");
        maybe_rotate_app_log_with_limits(&dir, 1000, 2);
        assert!(dir.join("wavetrace.log.1").exists());
        assert!(dir.join("wavetrace.log.2").exists());
        assert!(!dir.join("wavetrace.log.3").exists());

        write_chunk("wavetrace.log", "active3-");
        maybe_rotate_app_log_with_limits(&dir, 1000, 2);
        let third = std::fs::read_to_string(dir.join("wavetrace.log.2")).unwrap();
        assert!(third.starts_with("active2-"));
        assert!(!dir.join("wavetrace.log.3").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_legacy_scanner_log_filename() {
        let dir = std::env::temp_dir().join(format!(
            "wavetrace_log_migrate_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("scanner.log"), "legacy\n").unwrap();
        std::fs::write(dir.join("scanner.log.1"), "rotated\n").unwrap();

        migrate_legacy_app_log(&dir);

        assert!(!dir.join("scanner.log").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("wavetrace.log")).unwrap(),
            "legacy\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("wavetrace.log.1")).unwrap(),
            "rotated\n"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn detect_install_kind_microsoft_store_path() {
        let path = std::path::Path::new(
            r"C:\Users\test\AppData\Local\Packages\Meringue.WaveTrace_q6e7nywx05438\LocalCache\Roaming\wavetrace",
        );
        assert_eq!(detect_install_kind(path), "microsoft_store");
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn detect_install_kind_windows_direct_path() {
        let path = std::path::Path::new(r"C:\Users\test\AppData\Roaming\wavetrace");
        assert_eq!(detect_install_kind(path), "windows_direct");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn detect_install_kind_mac_sandbox_path() {
        let path = std::path::Path::new(
            "/Users/test/Library/Containers/com.wavetrace.app/Data/Library/Application Support/wavetrace",
        );
        assert_eq!(detect_install_kind(path), "mac_sandbox");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn detect_install_kind_mac_direct_path() {
        let path =
            std::path::Path::new("/Users/test/Library/Application Support/wavetrace");
        assert_eq!(detect_install_kind(path), "mac_direct");
    }
}
