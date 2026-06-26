//! Non-production visual branding (debug / `tauri dev` builds).

use tauri::{image::Image, App, Manager};

const ORANGE: [u8; 4] = [255, 136, 0, 255];

/// True for debug builds (`cargo tauri dev`, `cargo build` without `--release`).
pub fn is_dev_build() -> bool {
    cfg!(debug_assertions)
}

pub fn window_title() -> &'static str {
    if is_dev_build() {
        "WaveTrace (Dev)"
    } else {
        "WaveTrace"
    }
}

pub fn tooltip_prefix() -> &'static str {
    if is_dev_build() { "[Dev] " } else { "" }
}

/// Load the bundle icon, applying an orange border in dev builds.
pub fn load_icon(app: &App) -> Image<'static> {
    let base = app
        .default_window_icon()
        .expect("missing default window icon");
    if is_dev_build() {
        dev_marked_icon(&base)
    } else {
        owned_icon(&base)
    }
}

/// Set taskbar + tray icons and window title for the main window.
pub fn apply_branding(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let icon = load_icon(app);
    if let Some(window) = app.get_webview_window("main") {
        window.set_icon(icon.clone())?;
        window.set_title(window_title())?;
    }
    Ok(())
}

fn owned_icon(base: &Image<'_>) -> Image<'static> {
    Image::new_owned(base.rgba().to_vec(), base.width(), base.height())
}

fn dev_marked_icon(base: &Image<'_>) -> Image<'static> {
    let width = base.width();
    let height = base.height();
    let mut rgba = base.rgba().to_vec();
    paint_dev_marker(&mut rgba, width, height);
    Image::new_owned(rgba, width, height)
}

/// Orange border + bottom-right corner ribbon so dev builds stand out in the taskbar.
fn paint_dev_marker(rgba: &mut [u8], width: u32, height: u32) {
    let w = width as usize;
    let h = height as usize;
    let border = (width / 10).max(2) as usize;
    let band_h = (height as f32 * 0.28).ceil() as usize;

    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let on_border =
                x < border || x >= w - border || y < border || y >= h - border;
            let in_band = y >= h - band_h;
            let in_ribbon = in_band && x >= w.saturating_sub(band_h * 2);
            if on_border || in_ribbon {
                blend_pixel(&mut rgba[i..i + 4], ORANGE, if on_border { 1.0 } else { 0.92 });
            }
        }
    }
}

fn blend_pixel(dst: &mut [u8], src: [u8; 4], alpha: f32) {
    let a = alpha * (src[3] as f32 / 255.0);
    for c in 0..3 {
        dst[c] = ((dst[c] as f32) * (1.0 - a) + (src[c] as f32) * a).round() as u8;
    }
    dst[3] = 255;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_marker_touches_corners() {
        let mut rgba = vec![0u8; 32 * 32 * 4];
        for px in rgba.chunks_mut(4) {
            px[0] = 40;
            px[1] = 120;
            px[2] = 200;
            px[3] = 255;
        }
        paint_dev_marker(&mut rgba, 32, 32);
        assert_eq!(&rgba[0..4], &ORANGE);
        let br = (31 * 32 + 31) * 4;
        assert!(rgba[br] > 200);
    }
}
