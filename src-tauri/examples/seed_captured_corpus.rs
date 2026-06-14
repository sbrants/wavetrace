//! Seed `fixtures/captured/` from bundled screenshot fixtures + scaled variants.
//!
//! Usage:
//!   cargo run --example seed_captured_corpus
//!   cargo run --example seed_captured_corpus -- --clear-seeded

use std::path::PathBuf;

use image::RgbaImage;
use serde::Deserialize;
use towerrun_lib::fixture_capture::{self, capture_expect_from};

#[derive(Debug, Deserialize)]
struct ExpectedRoot {
    fixtures: Vec<ExpectedFixture>,
}

#[derive(Debug, Deserialize)]
struct ExpectedFixture {
    file: String,
    kind: String,
    expect: ExpectedValues,
}

#[derive(Debug, Deserialize)]
struct ExpectedValues {
    tier: Option<u32>,
    wave: Option<u32>,
    coin_per_minute_raw: Option<String>,
    coin_per_minute: Option<f64>,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
}

fn load_png(name: &str) -> RgbaImage {
    image::open(fixtures_dir().join(name))
        .unwrap_or_else(|e| panic!("missing fixture {name}: {e}"))
        .to_rgba8()
}

fn scale_to(img: &RgbaImage, w: u32, h: u32, tag: &str) -> (RgbaImage, String) {
    let scaled = image::imageops::resize(img, w, h, image::imageops::FilterType::Triangle);
    let stem = tag.replace(".png", "");
    (scaled, format!("{stem}_{w}x{h}.png"))
}

fn main() {
    let clear_seeded = std::env::args().any(|a| a == "--clear-seeded" || a == "--clear");
    let dir = fixture_capture::captured_dir();
    std::fs::create_dir_all(&dir).expect("create captured dir");

    if clear_seeded {
        let n = fixture_capture::clear_seeded_captures().expect("clear seeded captures");
        println!("Cleared {n} seeded capture(s) from {}", dir.display());
    }

    let expected_path = fixtures_dir().join("expected.json");
    let root: ExpectedRoot =
        serde_json::from_str(&std::fs::read_to_string(&expected_path).expect("expected.json"))
            .expect("parse expected.json");

    let mut manifest = fixture_capture::load_manifest();
    let mut added = 0usize;
    let mut updated = 0usize;

    let scales = [(978, 2084), (1080, 2280), (720, 1560)];

    for fx in root.fixtures.iter().filter(|f| f.kind == "screenshot") {
        let img = load_png(&fx.file);
        let mut variants = vec![(img.clone(), fx.file.clone())];
        for (w, h) in scales {
            let (scaled, name) = scale_to(&img, w, h, &fx.file);
            variants.push((scaled, name));
        }

        let expect = capture_expect_from(
            fx.expect.tier,
            fx.expect.wave,
            fx.expect.coin_per_minute_raw.clone(),
            fx.expect.coin_per_minute,
        );

        for (frame, file_name) in variants {
            if let Some(existing) = manifest.captures.iter_mut().find(|c| c.file == file_name) {
                existing.expect = Some(expect.clone());
                existing.notes = Some(format!(
                    "Seeded from {} (tier={:?} wave={:?} coin={:?})",
                    fx.file, fx.expect.tier, fx.expect.wave, fx.expect.coin_per_minute
                ));
                updated += 1;
                continue;
            }

            let path = dir.join(&file_name);
            frame.save(&path).expect("save png");

            let mut entry = fixture_capture::analyze_frame(&frame, "seeded_fixture");
            entry.file = file_name.clone();
            entry.id = file_name.trim_end_matches(".png").to_string();
            entry.expect = Some(expect.clone());
            entry.notes = Some(format!(
                "Seeded from {} (tier={:?} wave={:?} coin={:?})",
                fx.file, fx.expect.tier, fx.expect.wave, fx.expect.coin_per_minute
            ));
            manifest.captures.push(entry);
            added += 1;
        }
    }

    fixture_capture::save_manifest(&manifest).expect("save manifest");
    let report = fixture_capture::evaluate_manifest(&manifest);
    println!(
        "Seeded {added} new + {updated} updated in {} (total={} seeded={} live={})",
        dir.display(),
        manifest.captures.len(),
        report.seeded_total,
        report.live_total
    );
    println!(
        "coin_rate: seeded={}/{} all={}/{} labeled_pass={}/{}",
        report.seeded_coin_rate_hits,
        report.seeded_total,
        report.coin_rate_hits,
        report.total,
        report.labeled_pass,
        report.labeled
    );
    for fail in &report.failures {
        println!("FAIL: {fail}");
    }
    println!("Run: cargo test captured_corpus -- --nocapture");
}
