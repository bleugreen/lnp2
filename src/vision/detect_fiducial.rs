use opencv::core::Mat;
use tracing::debug;

use super::context::VisionContext;
use super::cv;
use super::error::VisionError;
use super::types::{CameraCalibration, Detection, DetectionMethod, RegionPx, VisionConfig};

/// Detect a fiducial marker in the frame.
///
/// Strategy:
/// 1. Circular symmetry detection (HoughCircles)
/// 2. Template matching fallback if template provided
pub fn detect_fiducial(
    jpeg: &[u8],
    cal: &CameraCalibration,
    min_diameter_mm: f64,
    max_diameter_mm: f64,
    template: Option<&Mat>,
) -> Result<Detection, VisionError> {
    let config = VisionConfig::default();
    let mut ctx = VisionContext::new(&config);

    let image = cv::decode_frame(jpeg)?;
    ctx.checkpoint("input", &image);

    let center_x = cal.width as f64 / 2.0;
    let center_y = cal.height as f64 / 2.0;

    let gray = cv::to_gray(&image)?;
    ctx.checkpoint("grayscale", &gray);

    let blurred = cv::blur(&gray, 9)?;
    ctx.checkpoint("blur", &blurred);

    // Convert mm to pixels
    let min_diameter_px = min_diameter_mm / cal.upp_x;
    let max_diameter_px = max_diameter_mm / cal.upp_x;

    // Try circular symmetry detection
    if let Some((circ_center, diameter_px, score)) =
        cv::detect_circular_symmetry(&blurred, min_diameter_px, max_diameter_px, 1.0)?
    {
        let offset_x_mm = (circ_center.x as f64 - center_x) * cal.upp_x;
        let offset_y_mm = (circ_center.y as f64 - center_y) * cal.upp_y;

        debug!(
            "Fiducial (circle): offset=({:.3}, {:.3})mm, diameter={:.1}px",
            offset_x_mm, offset_y_mm, diameter_px
        );

        return Ok(Detection {
            offset_x_mm,
            offset_y_mm,
            rotation_deg: 0.0,
            confidence: score.clamp(0.0, 1.0),
            method: DetectionMethod::CircularSymmetry,
            region_px: Some(RegionPx {
                x: circ_center.x as f64,
                y: circ_center.y as f64,
                width: diameter_px,
                height: diameter_px,
                rotation_deg: 0.0,
            }),
        });
    }

    ctx.log("Circular symmetry detection failed, trying template match");

    // Template matching fallback
    if let Some(tmpl) = template {
        let (match_center, score) = cv::template_match(&gray, tmpl)?;

        let offset_x_mm = (match_center.x as f64 - center_x) * cal.upp_x;
        let offset_y_mm = (match_center.y as f64 - center_y) * cal.upp_y;

        debug!(
            "Fiducial (template): offset=({:.3}, {:.3})mm, score={:.3}",
            offset_x_mm, offset_y_mm, score
        );

        if score > 0.5 {
            return Ok(Detection {
                offset_x_mm,
                offset_y_mm,
                rotation_deg: 0.0,
                confidence: score,
                method: DetectionMethod::TemplateMatch,
                region_px: None,
            });
        }
    }

    Err(VisionError::NoDetection)
}
