//! OCR via the built-in Windows OCR engine (Windows.Media.Ocr).
//!
//! Chosen over Tesseract per Goal.md's allowance for platform-native OCR:
//! no external binaries, well under the 500ms latency target.

#[cfg(windows)]
use std::cell::RefCell;

#[cfg(windows)]
use windows::Media::Ocr::OcrEngine;

// Reuse one OCR engine per thread — creating it every call costs ~100ms+.
#[cfg(windows)]
thread_local! {
    static ENGINE: RefCell<Option<OcrEngine>> = RefCell::new(None);
}

#[cfg(windows)]
fn ocr_engine() -> Result<OcrEngine, String> {
    ENGINE.with(|cell| {
        let mut slot = cell.borrow_mut();
        if let Some(engine) = slot.as_ref() {
            return Ok(engine.clone());
        }
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()
            .map_err(|e| format!("Windows OCR engine init failed: {e}"))?;
        *slot = Some(engine.clone());
        Ok(engine)
    })
}

/// Upscale small crops so Windows OCR can read game UI text reliably.
fn upscale_for_ocr(img: &image::RgbaImage) -> image::RgbaImage {
    const MIN_WIDTH: u32 = 480;
    if img.width() >= MIN_WIDTH {
        return img.clone();
    }
    let scale = MIN_WIDTH as f32 / img.width() as f32;
    let new_w = MIN_WIDTH;
    let new_h = ((img.height() as f32) * scale).round() as u32;
    image::imageops::resize(
        img,
        new_w,
        new_h.max(1),
        image::imageops::FilterType::Lanczos3,
    )
}

/// Stronger upscale for the tiny coin/min text before binarization: aim for a
/// larger minimum width and at least a modest zoom, capped to bound work.
fn upscale_strong(img: &image::RgbaImage) -> image::RgbaImage {
    const MIN_WIDTH: u32 = 720;
    let to_min = MIN_WIDTH as f32 / img.width().max(1) as f32;
    let scale = to_min.clamp(1.0, 4.0);
    if (scale - 1.0).abs() < f32::EPSILON {
        return img.clone();
    }
    let new_w = ((img.width() as f32) * scale).round() as u32;
    let new_h = ((img.height() as f32) * scale).round() as u32;
    image::imageops::resize(
        img,
        new_w.max(1),
        new_h.max(1),
        image::imageops::FilterType::Lanczos3,
    )
}

/// Otsu's method: pick the gray level that maximizes inter-class variance.
fn otsu_level(gray: &image::GrayImage) -> u8 {
    let mut hist = [0u32; 256];
    for p in gray.pixels() {
        hist[p[0] as usize] += 1;
    }
    let total = (gray.width() * gray.height()) as f64;
    if total == 0.0 {
        return 128;
    }
    let sum: f64 = (0..256).map(|i| i as f64 * hist[i] as f64).sum();
    let mut sum_b = 0.0;
    let mut w_b = 0.0;
    let mut max_var = -1.0;
    let mut threshold = 0u8;
    for (i, &h) in hist.iter().enumerate() {
        w_b += h as f64;
        if w_b == 0.0 {
            continue;
        }
        let w_f = total - w_b;
        if w_f == 0.0 {
            break;
        }
        sum_b += i as f64 * h as f64;
        let m_b = sum_b / w_b;
        let m_f = (sum - sum_b) / w_f;
        let var = w_b * w_f * (m_b - m_f) * (m_b - m_f);
        if var > max_var {
            max_var = var;
            threshold = i as u8;
        }
    }
    threshold
}

/// Binarize to crisp dark-text-on-light, which the Windows OCR engine reads
/// far more reliably than light glyphs over a busy translucent panel. The text
/// is the minority class, so whichever side of the Otsu threshold is smaller is
/// rendered black and the rest white (auto-handles either polarity).
fn binarize_for_ocr(img: &image::RgbaImage) -> image::RgbaImage {
    let gray = image::imageops::grayscale(img);
    let level = otsu_level(&gray);
    let above = gray.pixels().filter(|p| p[0] > level).count();
    let total = (gray.width() * gray.height()) as usize;
    let text_is_above = above * 2 <= total;

    let mut out = image::RgbaImage::new(gray.width(), gray.height());
    for (x, y, p) in gray.enumerate_pixels() {
        let is_text = (p[0] > level) == text_is_above;
        let v = if is_text { 0 } else { 255 };
        out.put_pixel(x, y, image::Rgba([v, v, v, 255]));
    }
    out
}

#[cfg(windows)]
fn ocr_lines_raw(img: &image::RgbaImage) -> Result<Vec<String>, String> {
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Storage::Streams::DataWriter;

    let engine = ocr_engine()?;
    let (w, h) = (img.width(), img.height());
    // RGBA -> BGRA
    let mut bgra = Vec::with_capacity((w * h * 4) as usize);
    for px in img.pixels() {
        bgra.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }

    let run = || -> windows::core::Result<Vec<String>> {
        let writer = DataWriter::new()?;
        writer.WriteBytes(&bgra)?;
        let buffer = writer.DetachBuffer()?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &buffer,
            BitmapPixelFormat::Bgra8,
            w as i32,
            h as i32,
        )?;
        let op = engine.RecognizeAsync(&bitmap)?;
        // Block until the async OCR finishes (we are on a worker thread).
        while op.Status()? == windows_future::AsyncStatus::Started {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let result = op.GetResults()?;
        let mut lines = Vec::new();
        for line in result.Lines()? {
            lines.push(line.Text()?.to_string());
        }
        Ok(lines)
    };
    run().map_err(|e| format!("Windows OCR failed: {e}"))
}

/// Plain OCR: upscale only. Used for large regions (tier/wave panel, mode
/// strip) where the text is already legible and binarization risks fragmenting
/// glyphs (e.g. "Tier 14" misreading after thresholding).
#[cfg(windows)]
pub fn ocr_lines(img: &image::RgbaImage) -> Result<Vec<String>, String> {
    ocr_lines_raw(&upscale_for_ocr(img))
}

/// Sharpened OCR for tiny, low-contrast targets (the coin/min row). Tries a
/// binarized pass first, then falls back to the plain upscaled crop when the
/// binarized pass reads nothing.
#[cfg(windows)]
pub fn ocr_lines_binarized(img: &image::RgbaImage) -> Result<Vec<String>, String> {
    let upscaled = upscale_strong(img);
    let binary = binarize_for_ocr(&upscaled);
    let lines = ocr_lines_raw(&binary)?;
    if !lines.is_empty() {
        return Ok(lines);
    }
    ocr_lines_raw(&upscaled)
}

#[cfg(not(windows))]
pub fn ocr_lines(_img: &image::RgbaImage) -> Result<Vec<String>, String> {
    Err("OCR is only implemented on Windows in Phase 1 (see Goal.md phases)".into())
}

#[cfg(not(windows))]
pub fn ocr_lines_binarized(_img: &image::RgbaImage) -> Result<Vec<String>, String> {
    Err("OCR is only implemented on Windows in Phase 1 (see Goal.md phases)".into())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Manual check against the fixture anchors. Run with:
    /// `cargo test --release -- --ignored ocr_fixture`
    /// Results depend on the installed Windows OCR language pack, so this is
    /// not part of the default test suite.
    #[test]
    #[ignore]
    fn ocr_fixture_images() {
        for name in [
            "Wave_and_Tier.png",
            "Coin_per_minute.png",
            "expected_state_full_game.png",
            "total_coin.png",
            "intro_sprint.png",
            "tournament.png",
            "end_of_run.png",
        ] {
            let path = format!("{}/../fixtures/{}", env!("CARGO_MANIFEST_DIR"), name);
            let img = image::open(&path).expect("fixture exists").to_rgba8();
            let lines = ocr_lines(&img).expect("ocr runs");
            println!("--- {name} ---");
            for l in &lines {
                println!("  {l}");
            }
        }
    }
}
