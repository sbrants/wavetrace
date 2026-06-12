//! Seed `fixtures/captured/` from bundled screenshot fixtures + scaled variants.
//!
//! Usage:
//!   cargo run --example seed_captured_corpus
//!   cargo run --example seed_captured_corpus -- --clear

use std::path::PathBuf;

use image::RgbaImage;
use serde::Deserialize;
use towerrun_lib::fixture_capture::{self, CaptureManifest};

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
    let clear = std::env::args().any(|a| a == "--clear");
    let dir = fixture_capture::captured_dir();
    std::fs::create_dir_all(&dir).expect("create captured dir");

    if clear {
        for ent in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            if name.ends_with(".png") {
                let _ = std::fs::remove_file(ent.path());
            }
        }
        fixture_capture::save_manifest(&CaptureManifest {
            version: 1,
            description: "Live captures for OCR regression. Set expect on entries for strict tests.".into(),
            captures: Vec::new(),
        })
        .expect("reset manifest");
        println!("Cleared {}", dir.display());
    }

    let expected_path = fixtures_dir().join("expected.json");
    let root: ExpectedRoot =
        serde_json::from_str(&std::fs::read_to_string(&expected_path).expect("expected.json"))
            .expect("parse expected.json");

    let mut manifest = fixture_capture::load_manifest();
    let mut added = 0usize;

    let scales = [(978, 2084), (1080, 2280), (720, 1560)];

    for fx in root.fixtures.iter().filter(|f| f.kind == "screenshot") {
        let img = load_png(&fx.file);
        let mut variants = vec![(img.clone(), fx.file.clone())];
        for (w, h) in scales {
            let (scaled, name) = scale_to(&img, w, h, &fx.file);
            variants.push((scaled, name));
        }

        for (frame, file_name) in variants {
            if manifest.captures.iter().any(|c| c.file == file_name) {
                continue;
            }
            let path = dir.join(&file_name);
            frame.save(&path).expect("save png");

            let mut entry = fixture_capture::analyze_frame(&frame, "seeded_fixture");
            entry.file = file_name.clone();
            entry.id = file_name.trim_end_matches(".png").to_string();
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
        "Seeded {added} frames into {} (total={})",
        dir.display(),
        manifest.captures.len()
    );
    println!(
        "coin_rate_hits={}/{} labeled_pass={}/{}",
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
