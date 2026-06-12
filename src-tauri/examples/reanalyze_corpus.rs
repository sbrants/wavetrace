//! Re-run OCR on all frames in fixtures/captured/ and print hit-rate stats.

use towerrun_lib::fixture_capture;

fn main() {
    let report = fixture_capture::reanalyze_all_captures().expect("reanalyze");
    println!(
        "total={} coin_rate_hits={} coin_rate_misses={} labeled_pass={}/{}",
        report.total,
        report.coin_rate_hits,
        report.coin_rate_misses,
        report.labeled_pass,
        report.labeled
    );
    for fail in &report.failures {
        println!("FAIL: {fail}");
    }
}
