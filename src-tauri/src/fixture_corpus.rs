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
    use crate::fixture_capture::{self, CaptureManifest};

    #[test]
    #[cfg(windows)]
    #[ignore = "requires Windows OCR"]
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
            "captured corpus: total={} coin_rate_hits={} coin_rate_misses={} labeled={} pass={} fail={}",
            report.total,
            report.coin_rate_hits,
            report.coin_rate_misses,
            report.labeled,
            report.labeled_pass,
            report.labeled_fail
        );
        for miss in manifest
            .captures
            .iter()
            .filter(|c| !c.classified.coin_rate_detected)
        {
            eprintln!(
                "MISS {} {}x{} coin_lines={:?}",
                miss.id, miss.width, miss.height, miss.ocr.coin_lines
            );
        }
        for fail in &report.failures {
            eprintln!("LABELED FAIL: {fail}");
        }

        let hit_rate = report.coin_rate_hits as f64 / report.total.max(1) as f64;
        eprintln!("coin detection hit rate: {:.0}%", hit_rate * 100.0);

        if report.labeled_fail > 0 {
            panic!("labeled capture failures: {:?}", report.failures);
        }

        let live = manifest
            .captures
            .iter()
            .filter(|c| c.window_title != "seeded_fixture")
            .count();
        if live > 0 {
            let live_hits = manifest
                .captures
                .iter()
                .filter(|c| c.window_title != "seeded_fixture" && c.classified.coin_rate_detected)
                .count();
            eprintln!(
                "live capture coin hit rate: {:.0}% ({}/{})",
                100.0 * live_hits as f64 / live as f64,
                live_hits,
                live
            );
        }
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "requires Windows OCR"]
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
                fresh.classified.coin_rate_detected,
                fresh.classified.coin_per_minute.is_some(),
                "coin flag consistent for {}",
                entry.id
            );
        }
    }
}
