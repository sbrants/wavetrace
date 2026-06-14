//! Tests against `fixtures/expected.json` and `fixtures/captured/manifest.json`.

use std::path::PathBuf;

use image::RgbaImage;
use serde::Deserialize;

use crate::fields;
use crate::parser::CoinReading;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
}

#[derive(Debug, Deserialize)]
struct ExpectedRoot {
    fixtures: Vec<ExpectedFixture>,
}

#[derive(Debug, Deserialize)]
struct ExpectedFixture {
    file: String,
    kind: String,
    #[serde(default)]
    game_mode: String,
    expect: ExpectedValues,
}

#[derive(Debug, Deserialize)]
struct ExpectedValues {
    tier: Option<u32>,
    wave: Option<u32>,
    coin_per_minute_raw: Option<String>,
    coin_per_minute: Option<f64>,
}

fn load_png(name: &str) -> RgbaImage {
    let path = fixtures_dir().join(name);
    image::open(&path)
        .unwrap_or_else(|_| panic!("fixture missing: {}", path.display()))
        .to_rgba8()
}

fn classify_fixture(img: &RgbaImage) -> crate::state_machine::PollInput {
    let ocr = fields::ocr_all_fields(img);
    fields::poll_input_from_fields(&ocr)
}

fn coin_value(coin: CoinReading) -> Option<f64> {
    match coin {
        CoinReading::Rate(v) => Some(v),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture_capture::{self, CaptureManifest, LIVE_COIN_HIT_RATE_MIN};

    #[test]
    #[cfg(windows)]
    fn expected_json_screenshots_have_ocr_pipeline() {
        let path = fixtures_dir().join("expected.json");
        let root: ExpectedRoot =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("expected.json")).unwrap();

        for fx in root.fixtures.iter().filter(|f| f.kind == "screenshot") {
            if fx.game_mode == "end_of_run" {
                continue;
            }
            let img = load_png(&fx.file);
            let ocr = fields::ocr_all_fields(&img);
            let input = fields::poll_input_from_fields(&ocr);

            if let Some(tier) = fx.expect.tier {
                assert_eq!(input.tier, Some(tier), "tier in {}", fx.file);
            }
            if let Some(wave) = fx.expect.wave {
                assert_eq!(input.wave, Some(wave), "wave in {}", fx.file);
            }
            if let Some(coin) = fx.expect.coin_per_minute {
                assert_eq!(
                    coin_value(input.coin),
                    Some(coin),
                    "coin/min in {} coin_lines={:?} coin={:?}",
                    fx.file,
                    ocr.all_lines,
                    input.coin
                );
            }
        }
    }

    #[test]
    #[cfg(windows)]
    fn seeded_corpus_labeled_expectations_pass() {
        let manifest = fixture_capture::load_manifest();
        let seeded: Vec<_> = manifest
            .captures
            .iter()
            .filter(|e| fixture_capture::is_seeded_entry(e))
            .collect();
        if seeded.is_empty() {
            eprintln!(
                "no seeded fixtures — run: cargo run --example seed_captured_corpus -- --clear-seeded"
            );
            return;
        }

        let with_expect: Vec<_> = seeded.iter().filter(|e| e.expect.is_some()).collect();
        assert!(
            !with_expect.is_empty(),
            "seeded fixtures missing expect labels — re-run seed_captured_corpus"
        );

        let report = fixture_capture::evaluate_manifest(&manifest);
        let seeded_failures: Vec<_> = report
            .failures
            .iter()
            .filter(|f| seeded.iter().any(|e| f.starts_with(&format!("{}:", e.id))))
            .collect();

        assert_eq!(
            seeded_failures.len(),
            0,
            "seeded labeled failures: {:?}",
            seeded_failures
        );
        eprintln!(
            "seeded corpus: total={} labeled={} pass={}",
            seeded.len(),
            with_expect.len(),
            with_expect.len() - seeded_failures.len()
        );
    }

    #[test]
    #[cfg(windows)]
    fn captured_corpus_reports_coin_detection() {
        let manifest: CaptureManifest = fixture_capture::load_manifest();
        if manifest.captures.is_empty() {
            eprintln!(
                "no captured fixtures yet — run: cargo run --example capture_fixtures -- --count 30"
            );
            return;
        }

        let report = fixture_capture::evaluate_manifest(&manifest);
        eprintln!(
            "captured corpus: total={} seeded={} live={}",
            report.total, report.seeded_total, report.live_total
        );
        eprintln!(
            "coin_rate: all={}/{} seeded={}/{} live={}/{}",
            report.coin_rate_hits,
            report.total,
            report.seeded_coin_rate_hits,
            report.seeded_total,
            report.live_coin_rate_hits,
            report.live_total
        );
        eprintln!(
            "labeled: pass={}/{} fail={}",
            report.labeled_pass, report.labeled, report.labeled_fail
        );

        for miss in manifest
            .captures
            .iter()
            .filter(|c| !c.classified.coin_rate_detected)
        {
            let kind = if fixture_capture::is_seeded_entry(miss) {
                "seeded"
            } else {
                "live"
            };
            eprintln!(
                "MISS [{kind}] {} {}x{} coin_lines={:?}",
                miss.id, miss.width, miss.height, miss.ocr.coin_lines
            );
        }
        for fail in &report.failures {
            eprintln!("LABELED FAIL: {fail}");
        }

        if report.labeled_fail > 0 {
            panic!("labeled capture failures: {:?}", report.failures);
        }

        if report.live_total > 0 {
            let live_hit_rate =
                report.live_coin_rate_hits as f64 / report.live_total.max(1) as f64;
            eprintln!(
                "live coin detection hit rate: {:.0}% ({}/{})",
                live_hit_rate * 100.0,
                report.live_coin_rate_hits,
                report.live_total
            );
            if live_hit_rate < LIVE_COIN_HIT_RATE_MIN {
                panic!(
                    "live coin hit rate {:.0}% below {:.0}% threshold ({}/{})",
                    live_hit_rate * 100.0,
                    LIVE_COIN_HIT_RATE_MIN * 100.0,
                    report.live_coin_rate_hits,
                    report.live_total
                );
            }
        }

        if report.seeded_total > 0 {
            let seeded_hit_rate =
                report.seeded_coin_rate_hits as f64 / report.seeded_total.max(1) as f64;
            eprintln!(
                "seeded coin detection hit rate: {:.0}% ({}/{})",
                seeded_hit_rate * 100.0,
                report.seeded_coin_rate_hits,
                report.seeded_total
            );
        }
    }

    #[test]
    #[cfg(windows)]
    fn reanalyze_captured_frames_match_manifest() {
        let manifest = fixture_capture::load_manifest();
        let dir = fixture_capture::captured_dir();
        for entry in &manifest.captures {
            let path = dir.join(&entry.file);
            if !path.exists() {
                continue;
            }
            let img = image::open(&path).expect("capture png").to_rgba8();
            let fresh = fixture_capture::analyze_frame(&img, &entry.window_title);

            assert_eq!(
                fresh.classified.tier, entry.classified.tier,
                "tier mismatch for {}",
                entry.id
            );
            assert_eq!(
                fresh.classified.wave, entry.classified.wave,
                "wave mismatch for {}",
                entry.id
            );
            assert_eq!(
                fresh.classified.coin_per_minute, entry.classified.coin_per_minute,
                "coin_per_minute mismatch for {} coin_lines={:?}",
                entry.id, fresh.ocr.coin_lines
            );
            assert_eq!(
                fresh.classified.coin_rate_detected, entry.classified.coin_rate_detected,
                "coin_rate_detected mismatch for {}",
                entry.id
            );
            assert_eq!(
                fresh.classified.coin_rate_detected,
                fresh.classified.coin_per_minute.is_some(),
                "coin flag consistent for {}",
                entry.id
            );
        }
    }
}
