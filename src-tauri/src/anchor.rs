//! Anchor (template) matching per Goal.md "OCR and field detection".
//!
//! Normalized cross-correlation over grayscale images. Used by calibration
//! to locate field positions from the user-provided reference crops.

use image::{GrayImage, RgbaImage};
use imageproc::template_matching::{match_template, MatchTemplateMethod};

#[derive(Debug)]
pub struct AnchorMatch {
    pub x: u32,
    pub y: u32,
    pub confidence: f32,
}

/// Default confidence threshold from Goal.md (0.85, configurable).
pub const DEFAULT_THRESHOLD: f32 = 0.85;

pub fn to_gray(img: &RgbaImage) -> GrayImage {
    image::imageops::grayscale(img)
}

/// Locate `template` inside `region`. Returns the best match position and its
/// normalized correlation score. Callers compare against the threshold and
/// fall back to manual bounding boxes below it.
pub fn locate(region: &GrayImage, template: &GrayImage) -> Option<AnchorMatch> {
    if template.width() > region.width() || template.height() > region.height() {
        return None;
    }
    let result = match_template(
        region,
        template,
        MatchTemplateMethod::CrossCorrelationNormalized,
    );
    let mut best = AnchorMatch {
        x: 0,
        y: 0,
        confidence: f32::MIN,
    };
    for (x, y, p) in result.enumerate_pixels() {
        if p[0] > best.confidence {
            best = AnchorMatch {
                x,
                y,
                confidence: p[0],
            };
        }
    }
    Some(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locates_exact_subimage() {
        let path = format!(
            "{}/../fixtures/Wave_and_Tier.png",
            env!("CARGO_MANIFEST_DIR")
        );
        let img = image::open(&path).expect("fixture exists").to_rgba8();
        let gray = to_gray(&img);
        // Cut a patch out of the image and find it again.
        let patch = image::imageops::crop_imm(&gray, 10, 5, 40, 20).to_image();
        let m = locate(&gray, &patch).expect("match found");
        assert_eq!((m.x, m.y), (10, 5));
        assert!(m.confidence > 0.99);
    }
}
