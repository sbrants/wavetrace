//! Full-frame OCR: Windows.Media.Ocr on Windows, Tesseract elsewhere.

#[cfg(windows)]
use std::cell::{Cell, RefCell};
#[cfg(windows)]
use std::ptr;
#[cfg(windows)]
use std::slice;
use std::sync::{Mutex, OnceLock};
#[cfg(windows)]
use std::time::Duration;
use std::time::Instant;

use image::{imageops, RgbaImage};

#[cfg(windows)]
use windows::{
    core::Interface,
    Graphics::Imaging::{BitmapBufferAccessMode, BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::{OcrEngine, OcrResult},
    Win32::System::WinRT::{IMemoryBufferByteAccess, RoInitialize, RO_INIT_MULTITHREADED},
};
#[cfg(windows)]
use windows_future::AsyncStatus;

/// Give up on a hung WinRT RecognizeAsync so the scanner thread can keep polling.
#[cfg(windows)]
const OCR_RECOGNIZE_TIMEOUT: Duration = Duration::from_secs(8);

/// Serialize OCR across scanner + Settings probe (engines are per-thread).
#[cfg(windows)]
static OCR_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(not(windows))]
static TESSDATA_INIT: OnceLock<()> = OnceLock::new();

#[cfg(not(windows))]
fn ensure_tesseract_paths() {
    TESSDATA_INIT.get_or_init(|| {
        if std::env::var_os("TESSDATA_PREFIX").is_some() {
            return;
        }
        #[cfg(target_os = "macos")]
        if let Ok(exe) = std::env::current_exe() {
            if let Some(resources) = exe
                .parent()
                .and_then(|macos| macos.parent())
                .map(|contents| contents.join("Resources"))
            {
                let tessdata = resources.join("tessdata");
                if tessdata.is_dir() {
                    std::env::set_var("TESSDATA_PREFIX", tessdata);
                }
            }
        }
    });
}

/// OCR the entire capture and return every non-empty text line discovered.
#[cfg(windows)]
pub fn ocr_full_frame(img: &RgbaImage) -> Result<Vec<String>, String> {
    Ok(ocr_full_frame_located(img)?
        .into_iter()
        .map(|l| l.text)
        .collect())
}

/// Full-frame OCR with line positions (normalized 0..1), used to anchor the GC strip.
#[cfg(windows)]
pub fn ocr_full_frame_located(img: &RgbaImage) -> Result<Vec<LocatedLine>, String> {
    let dynamic = prepare_image(img);
    let rgba = dynamic.to_rgba8();
    let result = recognize_rgba8(&rgba)?;
    located_lines_from_result(&result, rgba.height())
}

#[derive(Debug, Clone)]
pub struct LocatedLine {
    pub text: String,
    /// Top of the line as a fraction of image height, when Windows OCR provides bounds.
    pub y_norm: Option<f32>,
    /// Bottom of the line as a fraction of image height.
    pub bottom_norm: Option<f32>,
}

/// Floating Golden Combo toast corridor (not a fixed HUD line).
/// The toast spawns mid-battle (lower when skip/other toasts stack), rises, then fades.
/// We OCR a tall left-biased strip so one capture can catch it anywhere on that path.
const GC_BAND_X: f32 = 0.0;
const GC_BAND_W: f32 = 0.90;
/// 2× is enough for Windows OCR on the toast; 3× Lanczos was ~1s+ of prep per poll.
const GC_BAND_UPSCALE: u32 = 2;
const GC_BAND_MAX_WIDTH: u32 = 1000;
/// How far below Exit Battle the toast may still be spawning / rising.
/// Keep short: a tall band ingested Wave Skip / enemy-health OCR (`A 446`) as fake carets.
const GC_TOAST_BELOW_EXIT: f32 = 0.18;
/// Assumed Exit Battle bottom when OCR has not locked it (menu folded, cold start, or
/// the Exit line was misread). Live locks on the usual emulator layout sit ~0.35; the
/// old fallback band at y=0.14 covered [0.14, 0.36] and missed the toast zone under
/// Exit ([~0.35, ~0.53]), so unanchored GC polls had ~0% hits.
const GC_TOAST_DEFAULT_EXIT: f32 = 0.35;
/// Skip GC OCR when the yellow mask has almost no ink (empty corridor).
/// Live toast hits were ≥~860 ink; raising from 400 skips sparse FX crumbs.
const GC_YELLOW_MIN_INK_PIXELS: u32 = 800;
/// Skip GC OCR when ink is huge — battlefield gold FX, not a toast-sized glyph run.
/// Live hits topped out ~6.7k; above ~7k was miss/FX.
const GC_YELLOW_MAX_INK_PIXELS: u32 = 7_000;
/// Color backup only when yellow OCR is blank *and* ink is in the live toast cluster.
/// Hits sit ~3.5k–6.5k; blank+out-of-band usually means FX crumbs, not a missed toast.
const GC_COLOR_MIN_INK_PIXELS: u32 = 3_500;
const GC_COLOR_MAX_INK_PIXELS: u32 = 6_500;

/// Result of the dedicated Golden Combo toast OCR path (may skip OCR on ink gate).
#[derive(Debug, Clone)]
pub struct GoldenComboBandOcr {
    pub lines: Vec<String>,
    /// Non-zero pixels in the native-resolution yellow mask.
    pub ink_pixels: u32,
    /// `"-"`, `"ink"` (too little), or `"busy"` (too much FX).
    pub skip: &'static str,
    pub yellow_ms: u64,
    pub color_ms: u64,
}

impl Default for GoldenComboBandOcr {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            ink_pixels: 0,
            skip: "-",
            yellow_ms: 0,
            color_ms: 0,
        }
    }
}

impl GoldenComboBandOcr {
    pub fn gc_ms(&self) -> u64 {
        self.yellow_ms.saturating_add(self.color_ms)
    }

    pub fn skipped(&self) -> bool {
        self.skip != "-"
    }
}

/// Dedicated OCR pass for the floating Golden Combo toast.
/// Full-frame Windows OCR downscales and often misses the short-lived yellow line;
/// this crop is upscaled (yellow isolation, optional color) over the toast travel path.
pub fn ocr_golden_combo_band(img: &RgbaImage) -> GoldenComboBandOcr {
    ocr_golden_combo_band_anchored(img, None)
}

/// Like [`ocr_golden_combo_band`]. When `exit_bottom_norm` is known, the corridor
/// runs from just under Exit Battle downward through the toast spawn/rise zone
/// (skip and other popups push the start position lower).
pub fn ocr_golden_combo_band_anchored(
    img: &RgbaImage,
    exit_bottom_norm: Option<f32>,
) -> GoldenComboBandOcr {
    let gate_started = Instant::now();
    let (y, h) = toast_corridor(exit_bottom_norm);
    let crop = crop_norm_region(img, GC_BAND_X, y, GC_BAND_W, h);
    let mask = gc_band_yellow_mask(&crop);
    let ink_pixels = count_yellow_ink(&mask);
    if ink_pixels < GC_YELLOW_MIN_INK_PIXELS {
        return GoldenComboBandOcr {
            lines: Vec::new(),
            ink_pixels,
            skip: "ink",
            // Gate cost (mask) only — keep visibility in gc_y when skipped.
            yellow_ms: gate_started.elapsed().as_millis() as u64,
            color_ms: 0,
        };
    }
    if ink_pixels > GC_YELLOW_MAX_INK_PIXELS {
        return GoldenComboBandOcr {
            lines: Vec::new(),
            ink_pixels,
            skip: "busy",
            yellow_ms: gate_started.elapsed().as_millis() as u64,
            color_ms: 0,
        };
    }

    let mut lines = Vec::new();
    // One dilated yellow pass — crush cyan/battlefield, thicken thin glyphs (`^`, `x0.NN`).
    // Color is a rare backup: live logs show yellow finds ~96% of GC-like bands while
    // color ran on every miss. Only try color when yellow OCR returns no text at all.
    let yellow_started = Instant::now();
    let yellow = prepare_gc_band_yellow_from_mask(mask);
    if let Ok(extra) = ocr_prepared_rgba(&yellow) {
        push_unique_lines(&mut lines, extra);
    }
    let yellow_ms = yellow_started.elapsed().as_millis() as u64;

    let mut color_ms = 0u64;
    let yellow_blank = lines.iter().all(|l| l.trim().is_empty());
    let toast_ink =
        ink_pixels >= GC_COLOR_MIN_INK_PIXELS && ink_pixels <= GC_COLOR_MAX_INK_PIXELS;
    if yellow_blank && toast_ink {
        let color_started = Instant::now();
        let color = upscale_rgba(&crop, GC_BAND_UPSCALE, GC_BAND_MAX_WIDTH);
        if let Ok(extra) = ocr_prepared_rgba(&color) {
            push_unique_lines(&mut lines, extra);
        }
        color_ms = color_started.elapsed().as_millis() as u64;
    }

    let filtered: Vec<String> = lines
        .into_iter()
        .filter(|l| !is_gc_band_poison_line(l))
        .collect();
    GoldenComboBandOcr {
        lines: filtered,
        ink_pixels,
        skip: "-",
        yellow_ms,
        color_ms,
    }
}

fn is_gc_band_poison_line(line: &str) -> bool {
    let t = line.to_lowercase();
    let compact: String = t.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    compact.contains("waveskip")
        || compact.contains("skipped")
        || compact.contains("enemyhealth")
        || compact.contains("enemylevelskip")
        || compact.contains("enemyattack")
        || compact.contains("healthlevel")
        || compact.contains("attacklevel")
        || compact.contains("utilityupgrade")
        || compact.contains("attackupgrade")
        || compact.contains("executed")
        || compact.contains("exitbattle")
        || t.contains("wave skipped")
        || t.contains("enemy health")
        || t.contains("enemy level skip")
        || t.contains("enemy attack")
        || (t.contains("tier ") && !t.contains("gold"))
}

fn toast_corridor(exit_bottom_norm: Option<f32>) -> (f32, f32) {
    // Cover from Exit down through the rise path (toast moves upward toward Exit).
    // No lock → same geometry around [`GC_TOAST_DEFAULT_EXIT`] so a folded menu or a
    // cold start still aims at the toast, not at empty HUD above it.
    let bottom = exit_bottom_norm.unwrap_or(GC_TOAST_DEFAULT_EXIT);
    let y = bottom.clamp(0.08, 0.40);
    let h = GC_TOAST_BELOW_EXIT.min(0.55 - y).max(0.16);
    (y, h)
}

/// Write raw crop + yellow-isolated + color-upscaled GC toast previews for debugging.
/// Returns the corridor `(y, h)` used.
pub fn dump_gc_toast_previews(
    img: &RgbaImage,
    exit_bottom_norm: Option<f32>,
    out_dir: &std::path::Path,
) -> Result<(f32, f32), String> {
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let (y, h) = toast_corridor(exit_bottom_norm);
    let crop = crop_norm_region(img, GC_BAND_X, y, GC_BAND_W, h);
    let yellow = prepare_gc_band_yellow_rgba(&crop);
    let color = upscale_rgba(&crop, GC_BAND_UPSCALE, GC_BAND_MAX_WIDTH);

    crop.save(out_dir.join("gc_toast_raw_crop.png"))
        .map_err(|e| format!("save raw crop: {e}"))?;
    yellow
        .save(out_dir.join("gc_toast_yellow_preprocessed.png"))
        .map_err(|e| format!("save yellow: {e}"))?;
    color
        .save(out_dir.join("gc_toast_color_upscaled.png"))
        .map_err(|e| format!("save color: {e}"))?;
    Ok((y, h))
}

fn count_yellow_ink(mask: &image::GrayImage) -> u32 {
    mask.pixels().filter(|p| p[0] > 0).count() as u32
}

/// Bottom of the Exit Battle control as a fraction of frame height, if OCR saw it.
pub fn exit_battle_bottom_norm(lines: &[LocatedLine]) -> Option<f32> {
    let mut best: Option<f32> = None;
    for line in lines {
        let lower = line.text.to_lowercase();
        let compact: String = lower.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        let hit = (compact.contains("exit") && compact.contains("batt"))
            || compact.contains("exitbattle")
            || (lower.contains("exit") && lower.contains("battle"));
        if !hit {
            continue;
        }
        if let Some(b) = line.bottom_norm.or(line.y_norm) {
            best = Some(best.map_or(b, |p| p.max(b)));
        }
    }
    best
}

fn push_unique_lines(dst: &mut Vec<String>, incoming: Vec<String>) {
    for line in incoming {
        let key = line.to_lowercase();
        if dst.iter().any(|e| e.to_lowercase() == key) {
            continue;
        }
        dst.push(line);
    }
}

fn upscale_rgba(img: &RgbaImage, upscale: u32, max_width: u32) -> RgbaImage {
    if upscale <= 1 {
        return img.clone();
    }
    // Cap the upscale but never downscale: a crop already wider than the cap is OCR'd
    // as-is. `clamp(width, max_width)` panics for those, since min would exceed max.
    let target_w = img
        .width()
        .saturating_mul(upscale)
        .min(max_width)
        .max(img.width());
    if target_w <= img.width() {
        return img.clone();
    }
    let target_h = ((img.height() as f32) * (target_w as f32 / img.width() as f32))
        .round()
        .max(1.0) as u32;
    // CatmullRom is far cheaper than Lanczos3 at toast sizes and good enough for OCR.
    imageops::resize(img, target_w, target_h, imageops::FilterType::CatmullRom)
}

fn ocr_prepared_rgba(img: &RgbaImage) -> Result<Vec<String>, String> {
    #[cfg(windows)]
    {
        let result = recognize_rgba8(img)?;
        return lines_from_result(&result);
    }
    #[cfg(not(windows))]
    {
        ensure_tesseract_paths();
        let gray = imageops::grayscale(img);
        let width = gray.width() as i32;
        let height = gray.height() as i32;
        let text = run_tesseract(
            gray.as_raw(),
            width,
            height,
            1,
            width,
            tesseract::PageSegMode::PsmSingleBlock,
        )?;
        Ok(split_lines(&text))
    }
}

fn crop_norm_region(img: &RgbaImage, x: f32, y: f32, w: f32, h: f32) -> RgbaImage {
    let fw = img.width() as f32;
    let fh = img.height() as f32;
    let x0 = (x * fw).round() as u32;
    let y0 = (y * fh).round() as u32;
    let w_px = ((w * fw).round() as u32)
        .max(1)
        .min(img.width().saturating_sub(x0));
    let h_px = ((h * fh).round() as u32)
        .max(1)
        .min(img.height().saturating_sub(y0));
    imageops::crop_imm(img, x0, y0, w_px, h_px).to_image()
}

/// Keep saturated yellow toast ink; reject cyan trails and warm/near-white FX that
/// previously inflated ink counts so the gate never fired.
fn gc_band_yellow_mask(crop: &RgbaImage) -> image::GrayImage {
    use image::{GrayImage, Luma};

    let (w, h) = crop.dimensions();
    let mut gray = GrayImage::new(w, h);
    for (x, y, p) in crop.enumerate_pixels() {
        let r = p[0];
        let g = p[1];
        let b = p[2];
        let luma = ((u16::from(r) * 30 + u16::from(g) * 59 + u16::from(b) * 11) / 100) as u8;
        let yellow_score = r.saturating_add(g).saturating_sub(b.saturating_mul(2));
        // Cyan arcs are bright with high B — do not treat them as toast ink.
        let not_cyan = b < r.saturating_add(20) && b < g.saturating_add(20) && b < 140;
        // Saturated yellow/gold glyphs only (dropped the near-white HUD catch-all).
        let keep = not_cyan
            && luma >= 100
            && r >= 160
            && g >= 120
            && b <= 110
            && yellow_score >= 90
            && r > b.saturating_add(50)
            && g > b.saturating_add(30);
        let v = if keep {
            luma.max(r).max(g)
        } else {
            0
        };
        gray.put_pixel(x, y, Luma([v]));
    }
    imageops::colorops::contrast(&gray, 55.0)
}

fn gray_to_rgba(gray: &image::GrayImage) -> RgbaImage {
    use image::Rgba;
    let mut out = RgbaImage::new(gray.width(), gray.height());
    for (x, y, p) in gray.enumerate_pixels() {
        let v = p[0];
        out.put_pixel(x, y, Rgba([v, v, v, 255]));
    }
    out
}

fn upscale_gray(gray: image::GrayImage) -> image::GrayImage {
    // See `upscale_rgba`: cap the upscale without ever downscaling, and without
    // panicking on crops that are already wider than the cap.
    let target_w = gray
        .width()
        .saturating_mul(GC_BAND_UPSCALE)
        .min(GC_BAND_MAX_WIDTH)
        .max(gray.width());
    if target_w <= gray.width() {
        return gray;
    }
    let target_h = ((gray.height() as f32) * (target_w as f32 / gray.width() as f32))
        .round()
        .max(1.0) as u32;
    imageops::resize(&gray, target_w, target_h, imageops::FilterType::CatmullRom)
}

fn dilate_gray(gray: &image::GrayImage) -> image::GrayImage {
    use image::{GrayImage, Luma};
    let (w, h) = gray.dimensions();
    let mut out = GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut m = 0u8;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let xx = x as i32 + dx;
                    let yy = y as i32 + dy;
                    if xx >= 0 && yy >= 0 && (xx as u32) < w && (yy as u32) < h {
                        m = m.max(gray.get_pixel(xx as u32, yy as u32)[0]);
                    }
                }
            }
            out.put_pixel(x, y, Luma([m]));
        }
    }
    out
}

fn binarize_gray(gray: &image::GrayImage, threshold: u8) -> image::GrayImage {
    let mut out = gray.clone();
    for p in out.pixels_mut() {
        p[0] = if p[0] >= threshold { 255 } else { 0 };
    }
    out
}

/// Dilated + binarized yellow toast — thickens `^` / thin `x0.NN` for Windows OCR.
fn prepare_gc_band_yellow_rgba(crop: &RgbaImage) -> RgbaImage {
    prepare_gc_band_yellow_from_mask(gc_band_yellow_mask(crop))
}

fn prepare_gc_band_yellow_from_mask(gray: image::GrayImage) -> RgbaImage {
    let gray = upscale_gray(binarize_gray(&dilate_gray(&gray), 80));
    gray_to_rgba(&gray)
}

#[cfg(not(windows))]
pub fn ocr_full_frame(img: &RgbaImage) -> Result<Vec<String>, String> {
    Ok(ocr_full_frame_located(img)?
        .into_iter()
        .map(|l| l.text)
        .collect())
}

#[cfg(not(windows))]
pub fn ocr_full_frame_located(img: &RgbaImage) -> Result<Vec<LocatedLine>, String> {
    ensure_tesseract_paths();
    let mut all_lines = Vec::new();
    let mut any_ok = false;
    for &region in OCR_REGIONS {
        match ocr_region(img, region) {
            Ok(lines) => {
                if !lines.is_empty() {
                    any_ok = true;
                    for text in lines {
                        if all_lines.iter().any(|e: &LocatedLine| e.text == text) {
                            continue;
                        }
                        all_lines.push(LocatedLine {
                            text,
                            y_norm: None,
                            bottom_norm: None,
                        });
                    }
                }
            }
            Err(e) => eprintln!("OCR region {} failed: {e}", region.name),
        }
    }
    if !any_ok || all_lines.is_empty() {
        return Err("Tesseract OCR returned no text".into());
    }
    Ok(all_lines)
}

/// Normalized capture sub-rectangle for a targeted Tesseract pass.
#[cfg(not(windows))]
#[derive(Debug, Clone, Copy)]
struct OcrRegion {
    name: &'static str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    upscale: u32,
}

/// Portrait phone-mirror layout (~1:2.2). Fractions validated on 400×851 captures.
#[cfg(not(windows))]
const OCR_REGIONS: &[OcrRegion] = &[
    OcrRegion {
        name: "coin",
        x: 0.0,
        y: 0.0,
        w: 0.5,
        h: 0.2,
        upscale: 3,
    },
    OcrRegion {
        name: "wave_skip",
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 0.5,
        upscale: 3,
    },
    OcrRegion {
        name: "tier_wave",
        x: 0.5,
        y: 0.5,
        w: 0.5,
        h: 0.5,
        upscale: 3,
    },
];
// Golden Combo is OCR'd separately via `ocr_golden_combo_band` (tighter crop + preprocess).

#[cfg(not(windows))]
impl OcrRegion {
    fn to_pixels(self, frame_w: u32, frame_h: u32) -> (u32, u32, u32, u32) {
        let x = (self.x * frame_w as f32).round() as u32;
        let y = (self.y * frame_h as f32).round() as u32;
        let w = (self.w * frame_w as f32)
            .round()
            .min(frame_w.saturating_sub(x) as f32) as u32;
        let h = (self.h * frame_h as f32)
            .round()
            .min(frame_h.saturating_sub(y) as f32) as u32;
        (x, y, w.max(1), h.max(1))
    }
}

#[cfg(not(windows))]
fn crop_region(img: &RgbaImage, region: OcrRegion) -> RgbaImage {
    let (x, y, w, h) = region.to_pixels(img.width(), img.height());
    imageops::crop_imm(img, x, y, w, h).to_image()
}

#[cfg(not(windows))]
fn ocr_region(img: &RgbaImage, region: OcrRegion) -> Result<Vec<String>, String> {
    let crop = crop_region(img, region);
    let gray = prepare_region_for_tesseract(&crop, region.upscale);
    let width = gray.width() as i32;
    let height = gray.height() as i32;
    let bytes_per_line = width;
    let text = run_tesseract(
        gray.as_raw(),
        width,
        height,
        1,
        bytes_per_line,
        tesseract::PageSegMode::PsmSingleBlock,
    )?;
    Ok(split_lines(&text))
}

/// Grayscale, upscale, and contrast-stretch a cropped HUD region before OCR.
#[cfg(not(windows))]
fn prepare_region_for_tesseract(img: &RgbaImage, upscale: u32) -> image::GrayImage {
    let mut gray = imageops::grayscale(img);
    if upscale > 1 {
        let new_w = gray.width().saturating_mul(upscale);
        let new_h = gray.height().saturating_mul(upscale);
        gray = imageops::resize(&gray, new_w, new_h, imageops::FilterType::Lanczos3);
    }
    imageops::colorops::contrast(&gray, 30.0)
}

/// Run a single Tesseract pass over a raw grayscale frame. Uses the builder API
/// (rather than `tesseract::ocr_from_frame`) so we can hint a source resolution,
/// which keeps the LSTM engine from misjudging glyph scale on upscaled frames.
#[cfg(not(windows))]
fn run_tesseract(
    frame: &[u8],
    width: i32,
    height: i32,
    bytes_per_pixel: i32,
    bytes_per_line: i32,
    psm: tesseract::PageSegMode,
) -> Result<String, String> {
    let mut engine = tesseract::Tesseract::new(None, Some("eng"))
        .map_err(|e| format!("Tesseract init failed: {e}"))?
        .set_frame(frame, width, height, bytes_per_pixel, bytes_per_line)
        .map_err(|e| format!("Tesseract set_frame failed: {e}"))?
        .set_source_resolution(192);
    engine.set_page_seg_mode(psm);
    let mut engine = engine
        .recognize()
        .map_err(|e| format!("Tesseract recognize failed: {e}"))?;
    engine
        .get_text()
        .map_err(|e| format!("Tesseract get_text failed: {e}"))
}

#[cfg(windows)]
fn init_winrt() -> Result<(), String> {
    // RoInitialize is per-thread. A process-wide OnceLock skipped init on the next
    // scanner thread after Stop→New run, so OCR returned empty lines forever while
    // capture/preview still looked fine.
    thread_local! {
        static READY: Cell<bool> = const { Cell::new(false) };
    }
    READY.with(|ready| {
        if ready.get() {
            return Ok(());
        }
        unsafe {
            if let Err(e) = RoInitialize(RO_INIT_MULTITHREADED) {
                // RPC_E_CHANGED_MODE: this thread already has a different apartment.
                const RPC_E_CHANGED_MODE: i32 = -2147417850; // 0x80010106
                if e.code().0 != RPC_E_CHANGED_MODE {
                    return Err(format!("RoInitialize failed: {e}"));
                }
            }
        }
        ready.set(true);
        Ok(())
    })
}

#[cfg(windows)]
fn with_ocr_engine<T>(f: impl FnOnce(&OcrEngine) -> Result<T, String>) -> Result<T, String> {
    thread_local! {
        static ENGINE: RefCell<Option<Result<OcrEngine, String>>> =
            const { RefCell::new(None) };
    }
    ENGINE.with(|slot| {
        if slot.borrow().is_none() {
            let created = (|| {
                init_winrt()?;
                OcrEngine::TryCreateFromUserProfileLanguages()
                    .map_err(|e| format!("Windows OCR engine unavailable: {e}"))
            })();
            *slot.borrow_mut() = Some(created);
        }
        match slot.borrow().as_ref().expect("engine slot set") {
            Ok(engine) => f(engine),
            Err(e) => Err(e.clone()),
        }
    })
}

#[cfg(windows)]
fn recognize_rgba8(img: &RgbaImage) -> Result<OcrResult, String> {
    let _guard = OCR_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|e| format!("OCR mutex poisoned: {e}"))?;
    init_winrt()?;
    let bitmap = rgba_to_software_bitmap(img)?;
    with_ocr_engine(|engine| {
        let op = engine
            .RecognizeAsync(&bitmap)
            .map_err(|e| format!("Windows OCR RecognizeAsync failed: {e}"))?;
        // Stay on this thread (WinRT apartment). Poll status so we can Cancel on hang.
        let deadline = Instant::now() + OCR_RECOGNIZE_TIMEOUT;
        loop {
            match op
                .Status()
                .map_err(|e| format!("Windows OCR status failed: {e}"))?
            {
                AsyncStatus::Completed => {
                    return op
                        .GetResults()
                        .map_err(|e| format!("Windows OCR recognition failed: {e}"));
                }
                AsyncStatus::Error => {
                    return Err("Windows OCR recognition failed".into());
                }
                AsyncStatus::Canceled => {
                    return Err("Windows OCR recognition canceled".into());
                }
                AsyncStatus::Started => {
                    if Instant::now() >= deadline {
                        let _ = op.Cancel();
                        return Err(format!(
                            "Windows OCR timed out after {}s",
                            OCR_RECOGNIZE_TIMEOUT.as_secs()
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                other => {
                    return Err(format!("Windows OCR unexpected status: {other:?}"));
                }
            }
        }
    })
}

#[cfg(windows)]
fn lines_from_result(result: &OcrResult) -> Result<Vec<String>, String> {
    Ok(located_lines_from_result(result, 1)?
        .into_iter()
        .map(|l| l.text)
        .collect())
}

#[cfg(windows)]
fn located_lines_from_result(result: &OcrResult, image_height: u32) -> Result<Vec<LocatedLine>, String> {
    let h = image_height.max(1) as f32;
    let lines = result
        .Lines()
        .map_err(|e| format!("Windows OCR Lines() failed: {e}"))?;
    let count = lines
        .Size()
        .map_err(|e| format!("Windows OCR line count failed: {e}"))?;
    let mut out = Vec::new();
    for i in 0..count {
        let line = lines
            .GetAt(i)
            .map_err(|e| format!("Windows OCR line {i} failed: {e}"))?;
        let text = line
            .Text()
            .map_err(|e| format!("Windows OCR line {i} text failed: {e}"))?
            .to_string();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }

        let (y_norm, bottom_norm) = match line_bounds_norm(&line, h) {
            Ok(b) => b,
            Err(_) => (None, None),
        };
        out.push(LocatedLine {
            text: trimmed.to_string(),
            y_norm,
            bottom_norm,
        });
    }
    if out.is_empty() {
        if let Ok(text) = result.Text() {
            for t in split_lines(&text.to_string()) {
                out.push(LocatedLine {
                    text: t,
                    y_norm: None,
                    bottom_norm: None,
                });
            }
        }
    }
    Ok(out)
}

#[cfg(windows)]
fn line_bounds_norm(
    line: &windows::Media::Ocr::OcrLine,
    image_height: f32,
) -> Result<(Option<f32>, Option<f32>), String> {
    let words = line
        .Words()
        .map_err(|e| format!("Windows OCR Words() failed: {e}"))?;
    let count = words
        .Size()
        .map_err(|e| format!("Windows OCR word count failed: {e}"))?;
    if count == 0 {
        return Ok((None, None));
    }
    let mut top = f32::MAX;
    let mut bottom = f32::MIN;
    for i in 0..count {
        let word = words
            .GetAt(i)
            .map_err(|e| format!("Windows OCR word {i} failed: {e}"))?;
        let rect = word
            .BoundingRect()
            .map_err(|e| format!("Windows OCR BoundingRect failed: {e}"))?;
        top = top.min(rect.Y);
        bottom = bottom.max(rect.Y + rect.Height);
    }
    if !top.is_finite() || !bottom.is_finite() || bottom < top {
        return Ok((None, None));
    }
    Ok((Some(top / image_height), Some(bottom / image_height)))
}

#[cfg(windows)]
fn rgba_to_software_bitmap(img: &RgbaImage) -> Result<SoftwareBitmap, String> {
    let width = img.width() as i32;
    let height = img.height() as i32;
    let expected_len = (width as u64)
        .checked_mul(height as u64)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| "Image dimensions overflow".to_string())?;

    let bitmap = SoftwareBitmap::Create(BitmapPixelFormat::Bgra8, width, height)
        .map_err(|e| format!("SoftwareBitmap::Create failed: {e}"))?;

    {
        let bmp_buf = bitmap
            .LockBuffer(BitmapBufferAccessMode::Write)
            .map_err(|e| format!("LockBuffer failed: {e}"))?;
        let array: IMemoryBufferByteAccess = bmp_buf
            .CreateReference()
            .map_err(|e| format!("CreateReference failed: {e}"))?
            .cast()
            .map_err(|e| format!("IMemoryBufferByteAccess cast failed: {e}"))?;

        let mut data = ptr::null_mut();
        let mut capacity = 0u32;
        unsafe {
            array
                .GetBuffer(&mut data, &mut capacity)
                .map_err(|e| format!("GetBuffer failed: {e}"))?;
        }

        if capacity as u64 != expected_len {
            return Err(format!(
                "SoftwareBitmap buffer size mismatch: expected {expected_len}, got {capacity}"
            ));
        }

        let src = img.as_raw();
        let dst = unsafe { slice::from_raw_parts_mut(data, capacity as usize) };
        for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
    }

    Ok(bitmap)
}

/// Downscale large emulator frames so OCR stays responsive.
#[cfg(windows)]
fn prepare_image(img: &RgbaImage) -> image::DynamicImage {
    const MAX_WIDTH: u32 = 900;
    if img.width() <= MAX_WIDTH {
        return image::DynamicImage::ImageRgba8(img.clone());
    }
    let scale = MAX_WIDTH as f32 / img.width() as f32;
    let new_h = ((img.height() as f32) * scale).round().max(1.0) as u32;
    let resized = imageops::resize(img, MAX_WIDTH, new_h, imageops::FilterType::Triangle);
    image::DynamicImage::ImageRgba8(resized)
}

fn split_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn split_lines_trims_and_drops_blanks() {
        assert_eq!(
            split_lines("  Tier 14\n\nWave 2000\n"),
            vec!["Tier 14".to_string(), "Wave 2000".to_string()]
        );
    }

    #[test]
    fn prepare_gc_band_yellow_upsizes_and_keeps_rgba() {
        let mut img = RgbaImage::new(40, 20);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([220, 200, 40, 255]);
        }
        let out = prepare_gc_band_yellow_rgba(&img);
        assert!(out.width() >= img.width() * 2);
        assert_eq!(out.get_pixel(0, 0)[3], 255);
    }

    /// A 1920px-wide target window makes the band crop wider than `GC_BAND_MAX_WIDTH`,
    /// which used to panic the scanner thread mid-run.
    #[test]
    fn prepare_gc_band_yellow_leaves_crops_wider_than_the_cap_alone() {
        let wide = (GC_BAND_MAX_WIDTH as f32 * 1.8).round() as u32;
        let img = RgbaImage::from_pixel(wide, 20, Rgba([220, 200, 40, 255]));
        let out = prepare_gc_band_yellow_rgba(&img);
        assert_eq!(out.width(), wide);
        assert_eq!(out.height(), 20);
    }

    /// The whole band path on a 1920px-wide frame with toast-sized ink: the size the
    /// tester's emulator window reports, which panicked as soon as the ink gate passed.
    #[test]
    fn golden_combo_band_survives_a_wide_target_window() {
        let mut img = RgbaImage::from_pixel(1920, 2100, Rgba([12, 14, 18, 255]));
        for y in 300..330 {
            for x in 400..500 {
                img.put_pixel(x, y, Rgba([220, 200, 40, 255]));
            }
        }
        let result = ocr_golden_combo_band_anchored(&img, Some(0.10));
        assert_eq!(result.skip, "-", "ink gate should have run the band OCR");
        assert!(result.ink_pixels >= GC_YELLOW_MIN_INK_PIXELS);
        assert!(result.ink_pixels <= GC_YELLOW_MAX_INK_PIXELS);
    }

    #[test]
    fn upscale_rgba_caps_growth_and_never_shrinks() {
        let narrow = RgbaImage::from_pixel(100, 10, Rgba([1, 2, 3, 255]));
        assert_eq!(upscale_rgba(&narrow, 2, 1000).width(), 200);

        let at_cap = RgbaImage::from_pixel(600, 10, Rgba([1, 2, 3, 255]));
        assert_eq!(upscale_rgba(&at_cap, 2, 1000).width(), 1000);

        let over_cap = RgbaImage::from_pixel(1728, 10, Rgba([1, 2, 3, 255]));
        assert_eq!(upscale_rgba(&over_cap, 2, 1000).width(), 1728);
    }

    #[test]
    fn yellow_ink_gate_skips_empty_corridor() {
        let img = RgbaImage::from_pixel(200, 200, Rgba([20, 30, 40, 255]));
        let result = ocr_golden_combo_band_anchored(&img, Some(0.10));
        assert_eq!(result.skip, "ink");
        assert!(result.ink_pixels < GC_YELLOW_MIN_INK_PIXELS);
        assert!(result.lines.is_empty());
        assert_eq!(result.color_ms, 0);
    }

    #[test]
    fn yellow_ink_gate_skips_busy_fx_soup() {
        // Dense saturated yellow in a large corridor — above MAX, treat as FX not toast.
        let img = RgbaImage::from_pixel(400, 400, Rgba([220, 200, 40, 255]));
        let result = ocr_golden_combo_band_anchored(&img, Some(0.10));
        assert_eq!(result.skip, "busy");
        assert!(result.ink_pixels > GC_YELLOW_MAX_INK_PIXELS);
        assert!(result.lines.is_empty());
        assert_eq!(result.color_ms, 0);
    }

    #[test]
    fn yellow_ink_gate_counts_toast_colored_pixels() {
        let mut img = RgbaImage::from_pixel(120, 80, Rgba([10, 10, 10, 255]));
        for y in 30..50 {
            for x in 10..110 {
                img.put_pixel(x, y, Rgba([220, 200, 40, 255]));
            }
        }
        let mask = gc_band_yellow_mask(&img);
        let ink = count_yellow_ink(&mask);
        assert!(
            ink >= GC_YELLOW_MIN_INK_PIXELS,
            "expected toast yellow ink, got {ink}"
        );
        assert!(
            ink <= GC_YELLOW_MAX_INK_PIXELS,
            "toast stripe should stay under busy cap, got {ink}"
        );
    }

    #[test]
    fn yellow_mask_rejects_near_white_warm_noise() {
        let mut img = RgbaImage::from_pixel(80, 40, Rgba([10, 10, 10, 255]));
        for y in 5..35 {
            for x in 5..75 {
                // Warm near-white — old mask kept these; new mask should not.
                img.put_pixel(x, y, Rgba([220, 210, 180, 255]));
            }
        }
        let ink = count_yellow_ink(&gc_band_yellow_mask(&img));
        assert!(
            ink < GC_YELLOW_MIN_INK_PIXELS,
            "near-white warm noise should not count as toast ink, got {ink}"
        );
    }

    #[test]
    fn gc_toast_corridor_covers_rise_path_below_exit() {
        let img = RgbaImage::new(975, 2077);
        let (y, h) = toast_corridor(Some(0.27));
        assert!(y >= 0.20 && y <= 0.30);
        assert!(h >= 0.16 && h <= 0.22);
        let crop = crop_norm_region(&img, GC_BAND_X, y, GC_BAND_W, h);
        assert_eq!(crop.width(), (975.0 * GC_BAND_W).round() as u32);
        assert!(crop.height() > 300);
    }

    #[test]
    fn gc_toast_corridor_default_without_exit_matches_typical_lock() {
        let (y, h) = toast_corridor(None);
        let (y_locked, h_locked) = toast_corridor(Some(GC_TOAST_DEFAULT_EXIT));
        assert_eq!((y, h), (y_locked, h_locked));
        // Must reach the live toast zone under Exit (~0.35), not the old [0.14, 0.36] band.
        assert!((y - 0.35).abs() < 1e-6);
        assert!(y + h >= 0.50);
    }

    #[test]
    fn gc_toast_corridor_default_overlaps_live_exit_lock() {
        // Live monitor: Exit Battle bottom ≈ 0.350. Unanchored fallback must overlap
        // the anchored corridor so a missing Exit line does not zero GC recall.
        let (y_a, h_a) = toast_corridor(Some(0.35036495));
        let (y_d, h_d) = toast_corridor(None);
        let a0 = y_a;
        let a1 = y_a + h_a;
        let d0 = y_d;
        let d1 = y_d + h_d;
        assert!(a0 < d1 && d0 < a1, "default [{d0},{d1}] vs anchored [{a0},{a1}]");
    }

    #[test]
    fn exit_battle_bottom_picks_lowest_match() {
        let lines = vec![
            LocatedLine {
                text: "noise".into(),
                y_norm: Some(0.1),
                bottom_norm: Some(0.12),
            },
            LocatedLine {
                text: "EXIT BATTLE".into(),
                y_norm: Some(0.25),
                bottom_norm: Some(0.27),
            },
            LocatedLine {
                text: "Exit BattIe".into(),
                y_norm: Some(0.26),
                bottom_norm: Some(0.29),
            },
        ];
        assert!((exit_battle_bottom_norm(&lines).unwrap() - 0.29).abs() < 1e-6);
    }

    #[cfg(not(windows))]
    #[test]
    fn region_pixels_match_portrait_layout() {
        let coin = OCR_REGIONS[0];
        assert_eq!(coin.to_pixels(400, 851), (0, 0, 200, 170));
        let tier_wave = OCR_REGIONS[2];
        assert_eq!(tier_wave.to_pixels(400, 851), (200, 426, 200, 425));
    }
}
