//! Tests against `fixtures/reference.json` and `fixtures/captured/manifest.json`.

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use std::path::PathBuf;

    use image::RgbaImage;
    use serde::Deserialize;

    use crate::fields;
    use crate::fixture_capture::{self, CaptureManifest, LIVE_COIN_HIT_RATE_MIN};
    use crate::parser::CoinReading;
    use crate::state_machine::GameMode;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
    }

    #[derive(Debug, Deserialize)]
    struct ReferenceRoot {
        fixtures: Vec<ReferenceFixture>,
    }

    #[derive(Debug, Deserialize)]
    struct ReferenceFixture {
        file: String,
        game_mode: String,
        expect: ReferenceExpect,
    }

    #[derive(Debug, Deserialize)]
    struct ReferenceExpect {
        tier: Option<u32>,
        wave: Option<u32>,
        coin_per_minute: Option<f64>,
    }

    fn load_reference_png(name: &str) -> RgbaImage {
        let path = fixtures_dir().join(name);
        image::open(&path)
            .unwrap_or_else(|_| panic!("reference fixture missing: {}", path.display()))
            .to_rgba8()
    }

    fn game_mode_from_str(mode: &str) -> GameMode {
        match mode {
            "normal" => GameMode::Normal,
            "total_coin" => GameMode::TotalCoin,
            "intro_sprint" => GameMode::IntroSprint,
            "tournament" => GameMode::Tournament,
            "end_of_run" => GameMode::EndOfRun,
            _ => GameMode::Unknown,
        }
    }

    fn coin_rate_value(coin: CoinReading) -> Option<f64> {
        match coin {
            CoinReading::Rate(v) => Some(v),
            _ => None,
        }
    }

    #[test]
    fn reference_screenshots_have_ocr_pipeline() {
        let path = fixtures_dir().join("reference.json");
        let root: ReferenceRoot =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("reference.json")).unwrap();

        for fx in &root.fixtures {
            let img = load_reference_png(&fx.file);
            let ocr = fields::ocr_all_fields(&img);
            let input = fields::poll_input_from_fields(&ocr);

            assert_eq!(
                input.mode,
                game_mode_from_str(&fx.game_mode),
                "game_mode in {} lines={:?}",
                fx.file,
                ocr.all_lines
            );

            if let Some(tier) = fx.expect.tier {
                assert_eq!(input.tier, Some(tier), "tier in {} lines={:?}", fx.file, ocr.all_lines);
            }
            if let Some(wave) = fx.expect.wave {
                assert_eq!(input.wave, Some(wave), "wave in {} lines={:?}", fx.file, ocr.all_lines);
            }
            if let Some(coin) = fx.expect.coin_per_minute {
                assert_eq!(
                    coin_rate_value(input.coin),
                    Some(coin),
                    "coin/min in {} coin={:?} lines={:?}",
                    fx.file,
                    input.coin,
                    ocr.all_lines
                );
            } else {
                assert!(
                    !matches!(input.coin, CoinReading::Rate(_)),
                    "expected no coin rate in {} coin={:?}",
                    fx.file,
                    input.coin
                );
            }
        }
    }

    #[test]
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
            "captured corpus: total={} rate_eligible={}",
            report.total, report.rate_eligible
        );
        eprintln!(
            "coin_rate: {}/{}",
            report.coin_rate_hits, report.rate_eligible
        );
        eprintln!(
            "labeled: pass={}/{} fail={}",
            report.labeled_pass, report.labeled, report.labeled_fail
        );

        for miss in manifest
            .captures
            .iter()
            .filter(|c| fixture_capture::counts_toward_coin_hit_rate(&c.classified.game_mode))
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

        if report.labeled_fail > 0 {
            panic!("labeled capture failures: {:?}", report.failures);
        }

        if report.rate_eligible > 0 {
            let hit_rate = report.coin_rate_hits as f64 / report.rate_eligible.max(1) as f64;
            eprintln!(
                "coin detection hit rate: {:.0}% ({}/{})",
                hit_rate * 100.0,
                report.coin_rate_hits,
                report.rate_eligible
            );
            if hit_rate < LIVE_COIN_HIT_RATE_MIN {
                panic!(
                    "coin hit rate {:.0}% below {:.0}% threshold ({}/{})",
                    hit_rate * 100.0,
                    LIVE_COIN_HIT_RATE_MIN * 100.0,
                    report.coin_rate_hits,
                    report.rate_eligible
                );
            }
        }
    }

    #[test]
    fn reanalyze_captured_frames_match_manifest() {
        let manifest = fixture_capture::load_manifest();
        let dir = fixture_capture::captured_dir();
        for entry in &manifest.captures {
            let path = dir.join(&entry.file);
            assert!(
                path.exists(),
                "capture png missing for {}: {}",
                entry.id,
                path.display()
            );
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
                fresh.classified.game_mode, entry.classified.game_mode,
                "game_mode mismatch for {}",
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
