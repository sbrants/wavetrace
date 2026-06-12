//! Burst-capture game window frames into fixtures/captured/ for OCR regression.
//!
//! Usage:
//!   cargo run --example capture_fixtures -- --count 30 --interval 500
//!   cargo run --example capture_fixtures -- --count 50 --title "The Tower"

use towerrun_lib::{capture, db, fixture_capture, settings};

fn main() {
    let mut count: usize = 30;
    let mut interval_ms: u64 = 500;
    let mut title: Option<String> = None;
    let mut list_windows = false;
    let mut clear_live = false;

    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--count" | "-n" => {
                i += 1;
                count = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(30);
            }
            "--interval" | "-i" => {
                i += 1;
                interval_ms = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(500);
            }
            "--title" | "-t" => {
                i += 1;
                title = args.get(i).cloned();
            }
            "--list-windows" | "-l" => list_windows = true,
            "--clear-live" => clear_live = true,
            "--help" | "-h" => {
                println!(
                    "capture_fixtures — save frames to fixtures/captured/\n\n\
                     Options:\n\
                       --count, -n <N>       frames to capture (default 30)\n\
                       --interval, -i <ms>   delay between frames (default 500)\n\
                       --title, -t <text>    window title substring\n\
                       --clear-live          remove prior live captures (keep seeded)\n\
                       --list-windows, -l    show open windows and exit\n"
                );
                return;
            }
            other => eprintln!("unknown arg: {other}"),
        }
        i += 1;
    }

    if list_windows {
        println!("Open windows (with probe capture size when title matches filter):");
        let filter = title.as_deref().unwrap_or("");
        for w in capture::list_windows() {
            let mut size = String::from("—");
            if filter.is_empty() || w.title.to_lowercase().contains(&filter.to_lowercase()) {
                size = capture::probe_window(&w.title)
                    .map(|p| format!("{}×{} ({})", p.width, p.height, p.method))
                    .unwrap_or_else(|| "capture failed".into());
            }
            println!("  {} ({}) [{}]", w.title, w.app_name, size);
        }
        return;
    }

    let window_title = match title {
        Some(t) => t,
        None => {
            let conn = db::open().expect("open db");
            settings::resolve_target_window(&conn)
                .expect("configure target window in Settings first")
                .title_substring
        }
    };

    if clear_live {
        let n = fixture_capture::clear_live_captures().expect("clear live captures");
        println!("Cleared {n} prior live capture(s).");
    }

    println!(
        "Capturing {count} frames every {interval_ms}ms from \"{window_title}\"..."
    );
    println!("Output: {}", fixture_capture::captured_dir().display());

    let entries =
        fixture_capture::capture_burst(&window_title, count, interval_ms).expect("capture burst");

    let hits = entries
        .iter()
        .filter(|e| e.classified.coin_rate_detected)
        .count();
    println!(
        "Done. saved={} coin_rate_detected={}/{} ({:.0}%)",
        entries.len(),
        hits,
        entries.len(),
        100.0 * hits as f64 / entries.len().max(1) as f64
    );
    println!("Manifest: {}", fixture_capture::manifest_path().display());
    println!("Run tests: cargo test captured_corpus -- --nocapture");
}
