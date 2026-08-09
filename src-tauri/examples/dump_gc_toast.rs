//! One-shot: capture The Tower and dump GC toast preprocess previews.
//!
//!   cargo run --example dump_gc_toast --release

use std::path::PathBuf;

use wavetrace_lib::capture;
use wavetrace_lib::ocr;
use wavetrace_lib::settings::TargetWindow;

fn main() {
    let target = TargetWindow {
        title_substring: "The Tower".into(),
        process_name: String::new(),
        user_selected: false,
    };
    let frame = capture::capture_target(&target).unwrap_or_else(|| {
        eprintln!("Could not capture a window matching 'The Tower'.");
        std::process::exit(1);
    });
    eprintln!(
        "captured {}x{}",
        frame.width(),
        frame.height()
    );

    let exit_y = match ocr::ocr_full_frame_located(&frame) {
        Ok(lines) => {
            let y = ocr::exit_battle_bottom_norm(&lines);
            eprintln!("exit_y={y:?}");
            y
        }
        Err(e) => {
            eprintln!("full-frame OCR failed ({e}); using default corridor");
            None
        }
    };

    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("gc_toast_preview");
    let (y, h) = ocr::dump_gc_toast_previews(&frame, exit_y, &out).expect("dump failed");
    eprintln!(
        "wrote previews to {} (corridor y={y:.3} h={h:.3})",
        out.display()
    );
}
