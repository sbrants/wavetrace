//! Save live window captures into `fixtures/captured/` for OCR regression.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::{capture, fields, parser::CoinReading, state_machine::GameMode};

/// Minimum coin-rate detection rate for captured frames.
pub const LIVE_COIN_HIT_RATE_MIN: f64 = 0.80;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureExpect {
    pub tier: Option<u32>,
    pub wave: Option<u32>,
    pub coin_per_minute_raw: Option<String>,
    pub coin_per_minute: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureOcr {
    pub coin_lines: Vec<String>,
    pub tier_wave_lines: Vec<String>,
    pub mode_lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureClassified {
    pub tier: Option<u32>,
    pub wave: Option<u32>,
    pub game_mode: String,
    pub coin_reading: String,
    pub coin_per_minute: Option<f64>,
    pub coin_rate_detected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureEntry {
    pub id: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin_crop_file: Option<String>,
    pub captured_at: String,
    pub width: u32,
    pub height: u32,
    pub window_title: String,
    pub ocr: CaptureOcr,
    pub classified: CaptureClassified,
    /// Set manually to enable strict regression checks on this frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expect: Option<CaptureExpect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CaptureManifest {
    pub version: u32,
    pub description: String,
    pub captures: Vec<CaptureEntry>,
}

pub fn captured_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/captured")
}

pub fn manifest_path() -> PathBuf {
    captured_dir().join("manifest.json")
}

pub fn load_manifest() -> CaptureManifest {
    let path = manifest_path();
    if !path.exists() {
        return CaptureManifest {
            version: 1,
            description:
                "Live captures for OCR regression. Set expect on entries for strict tests.".into(),
            captures: Vec::new(),
        };
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_else(|_| CaptureManifest {
        version: 1,
        description: "Live captures for OCR regression.".into(),
        captures: Vec::new(),
    })
}

pub fn save_manifest(manifest: &CaptureManifest) -> Result<(), String> {
    let dir = captured_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())?;
    std::fs::write(manifest_path(), json).map_err(|e| e.to_string())
}

/// Remove every capture and delete all PNGs in `fixtures/captured/`.
pub fn clear_all_captures() -> Result<usize, String> {
    let dir = captured_dir();
    let manifest = load_manifest();
    let count = manifest.captures.len();
    if dir.exists() {
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("png") {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    save_manifest(&CaptureManifest {
        version: 1,
        description:
            "Live captures for OCR regression. Set expect on entries for strict tests.".into(),
        captures: Vec::new(),
    })?;
    Ok(count)
}

/// Drop captures where coin rate was not detected.
pub fn prune_coin_misses() -> Result<(usize, usize), String> {
    let dir = captured_dir();
    let mut manifest = load_manifest();
    let removed: Vec<_> = manifest
        .captures
        .iter()
        .filter(|e| !e.classified.coin_rate_detected)
        .cloned()
        .collect();
    for entry in &removed {
        let _ = std::fs::remove_file(dir.join(&entry.file));
        if let Some(coin) = &entry.coin_crop_file {
            let _ = std::fs::remove_file(dir.join(coin));
        }
    }
    manifest
        .captures
        .retain(|e| e.classified.coin_rate_detected);
    let kept = manifest.captures.len();
    save_manifest(&manifest)?;
    Ok((removed.len(), kept))
}

fn coin_reading_label(coin: CoinReading) -> (String, Option<f64>, bool) {
    match coin {
        CoinReading::Rate(v) => (format!("Rate({v})"), Some(v), true),
        CoinReading::Total(v) => (format!("Total({v})"), None, false),
        CoinReading::Unreadable => ("Unreadable".into(), None, false),
    }
}

fn game_mode_label(mode: GameMode) -> String {
    match mode {
        GameMode::Normal => "normal",
        GameMode::TotalCoin => "total_coin",
        GameMode::IntroSprint => "intro_sprint",
        GameMode::Tournament => "tournament",
        GameMode::EndOfRun => "end_of_run",
        GameMode::Unknown => "unknown",
    }
    .to_string()
}

pub fn analyze_frame(frame: &RgbaImage, window_title: &str) -> CaptureEntry {
    let fields = fields::ocr_all_fields(frame);
    let input = fields::poll_input_from_fields(&fields);
    let (coin_reading, coin_per_minute, coin_rate_detected) = coin_reading_label(input.coin);

    CaptureEntry {
        id: String::new(),
        file: String::new(),
        coin_crop_file: None,
        captured_at: chrono::Utc::now().to_rfc3339(),
        width: frame.width(),
        height: frame.height(),
        window_title: window_title.to_string(),
        ocr: CaptureOcr {
            coin_lines: coin_relevant_lines(&fields.all_lines),
            tier_wave_lines: fields.all_lines.clone(),
            mode_lines: Vec::new(),
        },
        classified: CaptureClassified {
            tier: input.tier,
            wave: input.wave,
            game_mode: game_mode_label(input.mode),
            coin_reading,
            coin_per_minute,
            coin_rate_detected,
        },
        expect: None,
        notes: None,
    }
}

fn write_png(path: &Path, img: &RgbaImage) -> Result<(), String> {
    img.save(path).map_err(|e| e.to_string())
}

pub fn capture_once(window_title: &str, label_detected: bool) -> Result<CaptureEntry, String> {
    let frame = capture::capture_by_title(window_title).ok_or_else(|| {
        format!(
            "Window not found or too small: \"{window_title}\". \
             Pick the emulator window in Settings (needs ~450×900+ pixels)."
        )
    })?;
    if frame.width() < 400 || frame.height() < 800 {
        return Err(format!(
            "Capture too small ({}×{}). Select the game/emulator window, not a sliver.",
            frame.width(),
            frame.height()
        ));
    }
    let mut entry = analyze_frame(&frame, window_title);
    persist_entry(&frame, &mut entry, label_detected)?;
    Ok(entry)
}

pub fn capture_burst(
    window_title: &str,
    count: usize,
    interval_ms: u64,
    label_detected: bool,
) -> Result<Vec<CaptureEntry>, String> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        match capture_once(window_title, label_detected) {
            Ok(entry) => {
                eprintln!(
                    "[{}/{count}] {} coin_rate={} coin_lines={:?}",
                    i + 1,
                    entry.id,
                    entry.classified.coin_rate_detected,
                    entry.ocr.coin_lines
                );
                out.push(entry);
            }
            Err(e) if i == 0 => return Err(e),
            Err(e) => eprintln!("capture {i} skipped: {e}"),
        }
        if i + 1 < count {
            thread::sleep(Duration::from_millis(interval_ms));
        }
    }
    Ok(out)
}

fn persist_entry(
    frame: &RgbaImage,
    entry: &mut CaptureEntry,
    label_detected: bool,
) -> Result<(), String> {
    let dir = captured_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let stamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let id = format!("{stamp}_{:03}", next_sequence(&dir, &stamp.to_string()));
    let file = format!("{id}.png");

    write_png(&dir.join(&file), frame)?;

    entry.id = id;
    entry.file = file;

    if label_detected {
        entry.expect = auto_expect_from_classified(&entry.classified);
    }

    let mut manifest = load_manifest();
    manifest.captures.push(entry.clone());
    save_manifest(&manifest)
}

fn next_sequence(dir: &Path, stamp: &str) -> u32 {
    let mut max = 0u32;
    let prefix = format!("{stamp}_");
    if let Ok(read) = std::fs::read_dir(dir) {
        for ent in read.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            if name.starts_with(&prefix) && name.ends_with(".png") && !name.contains("_coin") {
                if let Some(seq) = name
                    .strip_prefix(&prefix)
                    .and_then(|s| s.strip_suffix(".png"))
                {
                    if let Ok(n) = seq.parse::<u32>() {
                        max = max.max(n);
                    }
                }
            }
        }
    }
    max + 1
}

#[derive(Debug, Clone)]
pub struct CorpusReport {
    pub total: usize,
    pub coin_rate_hits: usize,
    pub coin_rate_misses: usize,
    pub labeled: usize,
    pub labeled_pass: usize,
    pub labeled_fail: usize,
    pub failures: Vec<String>,
}

pub fn capture_expect_from(
    tier: Option<u32>,
    wave: Option<u32>,
    coin_per_minute_raw: Option<String>,
    coin_per_minute: Option<f64>,
) -> CaptureExpect {
    CaptureExpect {
        tier,
        wave,
        coin_per_minute_raw,
        coin_per_minute,
    }
}

/// Build `expect` from classified OCR when tier, wave, and coin rate are all detected.
pub fn auto_expect_from_classified(classified: &CaptureClassified) -> Option<CaptureExpect> {
    if classified.tier.is_none() || classified.wave.is_none() || !classified.coin_rate_detected {
        return None;
    }
    Some(CaptureExpect {
        tier: classified.tier,
        wave: classified.wave,
        coin_per_minute_raw: None,
        coin_per_minute: classified.coin_per_minute,
    })
}

fn coin_relevant_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| {
            let lower = line.to_lowercase();
            if lower.contains("/min") {
                return true;
            }
            let t = line.trim();
            !t.contains('$')
                && (t.starts_with('@')
                    || t.starts_with("(Cc)")
                    || t.starts_with("(CC)")
                    || t.starts_with("(cc)")
                    || t.starts_with("C ")
                    || t.starts_with("c "))
        })
        .cloned()
        .collect()
}

/// Re-run OCR on every saved PNG and refresh manifest classified/ocr fields.
pub fn reanalyze_all_captures() -> Result<CorpusReport, String> {
    let dir = captured_dir();
    let mut manifest = load_manifest();
    for entry in &mut manifest.captures {
        let path = dir.join(&entry.file);
        if !path.exists() {
            continue;
        }
        let img = image::open(&path).map_err(|e| e.to_string())?.to_rgba8();
        let fresh = analyze_frame(&img, &entry.window_title);
        entry.ocr = fresh.ocr;
        entry.classified = fresh.classified;
        entry.width = fresh.width;
        entry.height = fresh.height;
        entry.coin_crop_file = None;
    }
    save_manifest(&manifest)?;
    Ok(evaluate_manifest(&manifest))
}

/// Backfill `expect` on entries from their classified values (for manual review).
pub fn label_detected_captures(manifest: &mut CaptureManifest) -> usize {
    let mut labeled = 0usize;
    for entry in &mut manifest.captures {
        if entry.expect.is_some() {
            continue;
        }
        if let Some(expect) = auto_expect_from_classified(&entry.classified) {
            entry.expect = Some(expect);
            labeled += 1;
        }
    }
    labeled
}

pub fn evaluate_manifest(manifest: &CaptureManifest) -> CorpusReport {
    let mut report = CorpusReport {
        total: manifest.captures.len(),
        coin_rate_hits: 0,
        coin_rate_misses: 0,
        labeled: 0,
        labeled_pass: 0,
        labeled_fail: 0,
        failures: Vec::new(),
    };

    for entry in &manifest.captures {
        if entry.classified.coin_rate_detected {
            report.coin_rate_hits += 1;
        } else {
            report.coin_rate_misses += 1;
        }

        let Some(expect) = &entry.expect else {
            continue;
        };
        report.labeled += 1;

        let mut ok = true;
        let mut reasons = Vec::new();

        if expect.coin_per_minute.is_some() {
            match entry.classified.coin_per_minute {
                Some(v) if Some(v) == expect.coin_per_minute => {}
                Some(v) => {
                    ok = false;
                    reasons.push(format!(
                        "coin_per_minute {v} != {:?}",
                        expect.coin_per_minute
                    ));
                }
                None => {
                    ok = false;
                    reasons.push("coin_per_minute missing".into());
                }
            }
        } else if expect.coin_per_minute.is_none() && entry.classified.coin_rate_detected {
            ok = false;
            reasons.push("coin_rate_detected but expect no rate".into());
        }

        if expect.tier.is_some() && entry.classified.tier != expect.tier {
            ok = false;
            reasons.push(format!(
                "tier {:?} != {:?}",
                entry.classified.tier, expect.tier
            ));
        }
        if expect.wave.is_some() && entry.classified.wave != expect.wave {
            ok = false;
            reasons.push(format!(
                "wave {:?} != {:?}",
                entry.classified.wave, expect.wave
            ));
        }

        if ok {
            report.labeled_pass += 1;
        } else {
            report.labeled_fail += 1;
            report
                .failures
                .push(format!("{}: {}", entry.id, reasons.join("; ")));
        }
    }
    report
}
