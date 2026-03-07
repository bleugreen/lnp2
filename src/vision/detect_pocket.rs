use tracing::debug;

use super::context::VisionContext;
use super::cv;
use super::error::VisionError;
use super::ml::{self, SharedSession};
use super::types::{CameraCalibration, Detection, DetectionMethod, RegionPx, VisionConfig};

/// Detect a pocket (feeder slot) in the frame.
///
/// Strategy:
/// 1. If ML model available and preferred → try ML first
/// 2. CV fallback: adaptive threshold → contours → min area rect → offset from center
pub fn detect_pocket(
    jpeg: &[u8],
    expected_size_mm: (f64, f64),
    cal: &CameraCalibration,
    config: &VisionConfig,
    model: Option<&SharedSession>,
) -> Result<Detection, VisionError> {
    let mut ctx = VisionContext::new(config);
    let image = cv::decode_frame(jpeg)?;
    ctx.checkpoint("input", &image);

    let center_x = cal.width as f64 / 2.0;
    let center_y = cal.height as f64 / 2.0;

    // ML path
    if let Some(session) = model {
        if config.prefer_ml {
            let boxes = ml::detect_ml(&image, session, config.confidence_threshold, &mut ctx)?;
            if let Some(best) = boxes.first() {
                let offset_x_mm = (best.x - center_x) * cal.upp_x;
                let offset_y_mm = (best.y - center_y) * cal.upp_y;
                ctx.log(format!(
                    "ML pocket: offset=({:.3}, {:.3})mm conf={:.3}",
                    offset_x_mm, offset_y_mm, best.confidence
                ));
                return Ok(Detection {
                    offset_x_mm,
                    offset_y_mm,
                    rotation_deg: 0.0,
                    confidence: best.confidence,
                    method: DetectionMethod::OnnxModel {
                        model_name: "yolov8".into(),
                    },
                    region_px: Some(RegionPx {
                        x: best.x, y: best.y,
                        width: best.width, height: best.height,
                        rotation_deg: 0.0,
                    }),
                });
            }
            ctx.log("ML: no pocket detected, falling back to CV");
        }
    }

    // CV path
    let gray = cv::to_gray(&image)?;
    ctx.checkpoint("grayscale", &gray);

    let blurred = cv::blur(&gray, config.blur_ksize)?;
    ctx.checkpoint("blur", &blurred);

    let thresh = cv::adaptive_threshold(&blurred, config.adaptive_block_size, config.adaptive_c)?;
    ctx.checkpoint("adaptive_threshold", &thresh);

    // Apply circular mask if configured
    let binary = if let Some(diameter) = config.mask_diameter_px {
        let masked = cv::mask_circle(&thresh, diameter)?;
        ctx.checkpoint("masked", &masked);
        masked
    } else {
        thresh
    };

    let contours = cv::find_contours(&binary)?;
    ctx.log(format!("CV: found {} contours", contours.len()));

    // Filter by expected size ±50%
    let expected_area_mm2 = expected_size_mm.0 * expected_size_mm.1;
    let min_area = expected_area_mm2 * 0.5;
    let max_area = expected_area_mm2 * 1.5;
    let filtered = cv::filter_contours_by_area(&contours, min_area, max_area, cal);
    ctx.log(format!(
        "CV: {} contours after area filter ({:.2}–{:.2} mm²)",
        filtered.len(), min_area, max_area
    ));

    // If no size-matched contours, try with config-wide area bounds
    let search_contours = if filtered.is_empty() {
        cv::filter_contours_by_area(
            &contours,
            config.contour_area_min_mm2,
            config.contour_area_max_mm2,
            cal,
        )
    } else {
        filtered
    };

    let rect = cv::fit_min_area_rect(&search_contours)
        .ok_or(VisionError::NoDetection)?;

    let offset_x_mm = (rect.center.x as f64 - center_x) * cal.upp_x;
    let offset_y_mm = (rect.center.y as f64 - center_y) * cal.upp_y;

    // Estimate confidence from area match
    let rect_area_mm2 = (rect.size.width as f64 * cal.upp_x) * (rect.size.height as f64 * cal.upp_y);
    let area_ratio = if expected_area_mm2 > 0.0 {
        (rect_area_mm2 / expected_area_mm2).min(expected_area_mm2 / rect_area_mm2)
    } else {
        0.5
    };
    let confidence = area_ratio.clamp(0.0, 1.0);

    debug!(
        "CV pocket: offset=({:.3}, {:.3})mm, rect_area={:.2}mm², confidence={:.3}",
        offset_x_mm, offset_y_mm, rect_area_mm2, confidence
    );

    Ok(Detection {
        offset_x_mm,
        offset_y_mm,
        rotation_deg: 0.0,
        confidence,
        method: DetectionMethod::AdaptiveContour,
        region_px: Some(RegionPx {
            x: rect.center.x as f64,
            y: rect.center.y as f64,
            width: rect.size.width as f64,
            height: rect.size.height as f64,
            rotation_deg: rect.angle as f64,
        }),
    })
}
