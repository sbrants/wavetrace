//! Backfill `expect` on live captures from classified OCR (for manual review).
//!
//! Usage:
//!   cargo run --example label_corpus
//!   cargo run --example label_corpus -- --dry-run

use towerrun_lib::fixture_capture;

fn main() {
    let dry_run = std::env::args().any(|a| a == "--dry-run");
    let mut manifest = fixture_capture::load_manifest();
    let before = manifest
        .captures
        .iter()
        .filter(|e| e.expect.is_some())
        .count();

    let labeled = fixture_capture::label_detected_captures(&mut manifest);

    if dry_run {
        println!("dry-run: would label {labeled} entries (already labeled: {before})");
        return;
    }

    fixture_capture::save_manifest(&manifest).expect("save manifest");
    let report = fixture_capture::evaluate_manifest(&manifest);
    println!(
        "Labeled {labeled} entries (total labeled now {})",
        before + labeled
    );
    println!(
        "corpus: seeded={} live={} labeled_pass={}/{}",
        report.seeded_total,
        report.live_total,
        report.labeled_pass,
        report.labeled
    );
    for fail in &report.failures {
        println!("FAIL: {fail}");
    }
}
