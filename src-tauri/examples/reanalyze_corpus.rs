//! Re-run OCR on all frames in fixtures/captured/ and print hit-rate stats.

use wavetrace_lib::fixture_capture::{self, LIVE_COIN_HIT_RATE_MIN};

fn main() {
    let report = fixture_capture::reanalyze_all_captures().expect("reanalyze");
    println!("total={}", report.total);
    println!(
        "coin_rate: {}/{} (eligible {})",
        report.coin_rate_hits, report.rate_eligible, report.rate_eligible
    );
    if report.rate_eligible > 0 {
        let hit_rate = report.coin_rate_hits as f64 / report.rate_eligible as f64;
        println!(
            "hit rate: {:.0}% (threshold {:.0}%)",
            hit_rate * 100.0,
            LIVE_COIN_HIT_RATE_MIN * 100.0
        );
    }
    println!(
        "labeled: pass={}/{} fail={}",
        report.labeled_pass, report.labeled, report.labeled_fail
    );
    for fail in &report.failures {
        println!("FAIL: {fail}");
    }
}
