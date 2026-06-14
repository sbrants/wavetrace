//! Tests against `fixtures/captured/manifest.json`.

#[cfg(test)]
mod tests {
    use crate::fixture_capture::{self, CaptureManifest, LIVE_COIN_HIT_RATE_MIN};

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
            "captured corpus: total={}",
            report.total
        );
        eprintln!(
            "coin_rate: {}/{}",
            report.coin_rate_hits, report.total
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

        if report.total > 0 {
            let hit_rate = report.coin_rate_hits as f64 / report.total.max(1) as f64;
            eprintln!(
                "coin detection hit rate: {:.0}% ({}/{})",
                hit_rate * 100.0,
                report.coin_rate_hits,
                report.total
            );
            if hit_rate < LIVE_COIN_HIT_RATE_MIN {
                panic!(
                    "coin hit rate {:.0}% below {:.0}% threshold ({}/{})",
                    hit_rate * 100.0,
                    LIVE_COIN_HIT_RATE_MIN * 100.0,
                    report.coin_rate_hits,
                    report.total
                );
            }
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
