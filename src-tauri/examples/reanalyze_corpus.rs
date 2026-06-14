//! Re-run OCR on all frames in fixtures/captured/ and print hit-rate stats.

use wavetrace_lib::fixture_capture::{self, LIVE_COIN_HIT_RATE_MIN};

fn main() {
    let report = fixture_capture::reanalyze_all_captures().expect("reanalyze");
    println!(
        "total={} seeded={} live={}",
        report.total, report.seeded_total, report.live_total
    );
    println!(
        "coin_rate: all={}/{} seeded={}/{} live={}/{}",
        report.coin_rate_hits,
        report.total,
        report.seeded_coin_rate_hits,
        report.seeded_total,
        report.live_coin_rate_hits,
        report.live_total
    );
    if report.live_total > 0 {
        let live_rate = report.live_coin_rate_hits as f64 / report.live_total as f64;
        println!(
            "live hit rate: {:.0}% (threshold {:.0}%)",
            live_rate * 100.0,
            LIVE_COIN_HIT_RATE_MIN * 100.0
        );
    }
    if report.seeded_total > 0 {
        let seeded_rate = report.seeded_coin_rate_hits as f64 / report.seeded_total as f64;
        println!("seeded hit rate: {:.0}%", seeded_rate * 100.0);
    }
    println!(
        "labeled: pass={}/{} fail={}",
        report.labeled_pass, report.labeled, report.labeled_fail
    );
    for fail in &report.failures {
        println!("FAIL: {fail}");
    }
}
