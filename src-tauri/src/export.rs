//! Run and snapshot exports (CSV + ODS workbook).

use std::collections::HashSet;

use chrono::Utc;
use icu_locale_core::locale;
use rusqlite::Connection;
use serde::Serialize;
use spreadsheet_ods::{write_ods_buf, Sheet, WorkBook};

use crate::db::{self, RunFilter, RunRow, SnapshotRow};

#[derive(Debug, Serialize)]
pub struct CsvExportPayload {
    pub filename: String,
    pub content: String,
    pub run_count: usize,
    pub snapshot_count: usize,
}

#[derive(Debug, Serialize)]
pub struct WorkbookExportPayload {
    pub filename: String,
    pub data_base64: String,
    pub run_count: usize,
    pub snapshot_count: usize,
}

pub fn snapshots_csv_filename() -> String {
    format!("wavetrace-snapshots-{}.csv", export_stamp())
}

pub fn workbook_ods_filename() -> String {
    format!("wavetrace-runs-{}.ods", export_stamp())
}

fn export_stamp() -> String {
    Utc::now().format("%Y-%m-%dT%H-%M-%S").to_string()
}

/// Flat CSV: one row per snapshot with parent run metadata.
pub fn export_snapshots_csv(
    conn: &Connection,
    filter: &RunFilter,
) -> rusqlite::Result<(String, usize, usize)> {
    let runs = db::list_runs(conn, filter)?;
    let mut out = String::from(
        "run_id,started_at,ended_at,run_type,peak_tier,final_wave,run_comment,wave,tier,coin_per_minute,recorded_at\n",
    );
    let mut snapshot_count = 0usize;
    for run in &runs {
        let snaps = db::run_snapshots(conn, &run.id)?;
        for snap in &snaps {
            snapshot_count += 1;
            out.push_str(&format_snapshot_csv_row(run, snap));
        }
    }
    Ok((out, runs.len(), snapshot_count))
}

/// ODS workbook bytes: summary sheet plus one sheet per run with its snapshots.
pub fn export_workbook_ods_bytes(
    conn: &Connection,
    filter: &RunFilter,
) -> Result<(Vec<u8>, usize, usize), String> {
    let (mut wb, run_count, snapshot_count) = build_workbook(conn, filter)?;
    let bytes = write_ods_buf(&mut wb, Vec::new()).map_err(|e| e.to_string())?;
    Ok((bytes, run_count, snapshot_count))
}

fn build_workbook(
    conn: &Connection,
    filter: &RunFilter,
) -> Result<(WorkBook, usize, usize), String> {
    let runs = db::list_runs(conn, filter).map_err(|e| e.to_string())?;
    let mut wb = WorkBook::new(locale!("en-US"));
    let mut used_sheet_names = HashSet::new();
    let mut snapshot_count = 0usize;

    let mut summary = Sheet::new("Runs");
    write_runs_summary_header(&mut summary);
    for (i, run) in runs.iter().enumerate() {
        write_runs_summary_row(&mut summary, i as u32 + 1, run);
    }
    wb.push_sheet(summary);
    used_sheet_names.insert("Runs".to_string());

    for (idx, run) in runs.iter().enumerate() {
        let sheet_name = unique_sheet_name(run, idx, &mut used_sheet_names);
        let snaps = db::run_snapshots(conn, &run.id).map_err(|e| e.to_string())?;
        snapshot_count += snaps.len();
        let mut sheet = Sheet::new(&sheet_name);
        write_run_detail_sheet(&mut sheet, run, &snaps);
        wb.push_sheet(sheet);
    }

    Ok((wb, runs.len(), snapshot_count))
}

fn format_snapshot_csv_row(run: &RunRow, snap: &SnapshotRow) -> String {
    format!(
        "{},{},{},{},{},{},{},{},{},{},{}\n",
        csv_escape(&run.id),
        csv_escape(&run.started_at),
        csv_escape(run.ended_at.as_deref().unwrap_or("")),
        csv_escape(&run.run_type),
        run.peak_tier.map(|v| v.to_string()).unwrap_or_default(),
        run.final_wave.map(|v| v.to_string()).unwrap_or_default(),
        csv_escape(run.comment.as_deref().unwrap_or("")),
        snap.wave,
        snap.tier.map(|v| v.to_string()).unwrap_or_default(),
        snap.coin_per_minute
            .map(|v| v.to_string())
            .unwrap_or_default(),
        csv_escape(&snap.recorded_at),
    )
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn write_runs_summary_header(sheet: &mut Sheet) {
    let headers = [
        "id",
        "started_at",
        "ended_at",
        "run_type",
        "peak_tier",
        "final_wave",
        "avg_coin_per_minute",
        "snapshot_count",
        "comment",
    ];
    for (col, header) in headers.iter().enumerate() {
        sheet.set_value(0, col as u32, *header);
    }
}

fn write_runs_summary_row(sheet: &mut Sheet, row: u32, run: &RunRow) {
    sheet.set_value(row, 0, run.id.as_str());
    sheet.set_value(row, 1, run.started_at.as_str());
    sheet.set_value(row, 2, run.ended_at.as_deref().unwrap_or(""));
    sheet.set_value(row, 3, run.run_type.as_str());
    if let Some(t) = run.peak_tier {
        sheet.set_value(row, 4, t as f64);
    }
    if let Some(w) = run.final_wave {
        sheet.set_value(row, 5, w as f64);
    }
    if let Some(c) = run.avg_coin_per_minute {
        sheet.set_value(row, 6, c);
    }
    sheet.set_value(row, 7, run.snapshot_count as f64);
    sheet.set_value(row, 8, run.comment.as_deref().unwrap_or(""));
}

fn write_run_detail_sheet(sheet: &mut Sheet, run: &RunRow, snaps: &[SnapshotRow]) {
    sheet.set_value(0, 0, "Started");
    sheet.set_value(0, 1, run.started_at.as_str());
    sheet.set_value(1, 0, "Ended");
    sheet.set_value(1, 1, run.ended_at.as_deref().unwrap_or("ongoing"));
    sheet.set_value(2, 0, "Type");
    sheet.set_value(2, 1, run.run_type.as_str());
    sheet.set_value(3, 0, "Comment");
    sheet.set_value(3, 1, run.comment.as_deref().unwrap_or(""));

    sheet.set_value(5, 0, "wave");
    sheet.set_value(5, 1, "tier");
    sheet.set_value(5, 2, "coin_per_minute");
    sheet.set_value(5, 3, "recorded_at");

    for (i, snap) in snaps.iter().enumerate() {
        let row = 6 + i as u32;
        sheet.set_value(row, 0, snap.wave as f64);
        if let Some(t) = snap.tier {
            sheet.set_value(row, 1, t as f64);
        }
        if let Some(c) = snap.coin_per_minute {
            sheet.set_value(row, 2, c);
        }
        sheet.set_value(row, 3, snap.recorded_at.as_str());
    }
}

fn unique_sheet_name(run: &RunRow, index: usize, used: &mut HashSet<String>) -> String {
    let date = run.started_at.chars().take(10).collect::<String>();
    let kind = if run.run_type == "tournament" {
        "tournament"
    } else {
        "farming"
    };
    let wave = run.final_wave.unwrap_or(0);
    let tier = run.peak_tier.unwrap_or(0);
    let base = format!("{date} {kind} T{tier} W{wave}");
    let mut name = sanitize_sheet_name(&base);
    if name.is_empty() {
        name = format!("run_{index}");
    }
    if used.contains(&name) {
        let mut n = 2;
        loop {
            let candidate = sanitize_sheet_name(&format!("{name} ({n})"));
            if !used.contains(&candidate) {
                name = candidate;
                break;
            }
            n += 1;
        }
    }
    used.insert(name.clone());
    name
}

fn sanitize_sheet_name(name: &str) -> String {
    let invalid = ['\\', '/', '*', '?', ':', '[', ']'];
    let mut out: String = name
        .chars()
        .map(|c| if invalid.contains(&c) { '_' } else { c })
        .collect();
    if out.len() > 31 {
        out.truncate(31);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn export_snapshots_csv_includes_every_capture() {
        let conn = db::open_in_memory().unwrap();
        let id = db::start_run(&conn, "farming").unwrap();
        db::insert_snapshot(&conn, &id, 1, Some(10), Some(100.0)).unwrap();
        db::insert_snapshot(&conn, &id, 2, Some(11), Some(200.0)).unwrap();
        db::end_run(&conn, &id, Some(2), Some(11)).unwrap();

        let (csv, runs, snaps) = export_snapshots_csv(&conn, &db::RunFilter::default()).unwrap();
        assert_eq!(runs, 1);
        assert_eq!(snaps, 2);
        assert_eq!(csv.matches('\n').count(), 3); // header + 2 rows
        assert!(csv.contains(",1,"));
        assert!(csv.contains(",2,"));
    }

    #[test]
    fn export_workbook_ods_bytes_contains_data() {
        let conn = db::open_in_memory().unwrap();
        let id = db::start_run(&conn, "farming").unwrap();
        db::insert_snapshot(&conn, &id, 3, Some(12), Some(300.0)).unwrap();
        db::end_run(&conn, &id, Some(3), Some(12)).unwrap();

        let (bytes, run_count, snapshot_count) =
            export_workbook_ods_bytes(&conn, &db::RunFilter::default()).unwrap();
        assert_eq!(run_count, 1);
        assert_eq!(snapshot_count, 1);
        assert!(!bytes.is_empty());
    }
}
