use opencv::imgproc;
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

    let expected_area_mm2 = expected_size_mm.0 * expected_size_mm.1;
    let px_per_mm2 = 1.0 / (cal.upp_x * cal.upp_y);
    let expected_area_px = expected_area_mm2 * px_per_mm2;

    // Score each contour: prefer close to center + close to expected area
    let mut best_score = f64::NEG_INFINITY;
    let mut best_contour = None;

    for i in 0..contours.len() {
        let contour = match contours.get(i) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let area_px = imgproc::contour_area(&contour, false).unwrap_or(0.0);

        // Skip tiny noise and oversized contours (background, tape body)
        if area_px < config.contour_area_min_mm2 * px_per_mm2 {
            continue;
        }
        if expected_area_px > 0.0 && area_px > expected_area_px * 5.0 {
            continue;
        }

        let centroid = match cv::contour_centroid(&contour) {
            Some(c) => c,
            None => continue,
        };

        // Distance from image center (normalized by image diagonal)
        let dx = centroid.x as f64 - center_x;
        let dy = centroid.y as f64 - center_y;
        let dist = (dx * dx + dy * dy).sqrt();
        let diag = (center_x * center_x + center_y * center_y).sqrt();
        let center_score = 1.0 - (dist / diag).min(1.0); // 1.0 = at center, 0.0 = at corner

        // Area match score: ratio closer to 1.0 is better
        let area_ratio = if expected_area_px > 0.0 {
            (area_px / expected_area_px).min(expected_area_px / area_px)
        } else {
            0.5
        };

        // Combined score: heavily weight center proximity, moderate weight on area match
        let score = center_score * 0.6 + area_ratio * 0.4;

        if score > best_score {
            best_score = score;
            best_contour = Some(contour);
        }
    }

    let contour = best_contour.ok_or(VisionError::NoDetection)?;

    // Fit rect to the single best contour
    let rect = if contour.len() >= 5 {
        imgproc::min_area_rect(&contour).map_err(VisionError::from)?
    } else {
        let br = imgproc::bounding_rect(&contour).map_err(VisionError::from)?;
        opencv::core::RotatedRect::new(
            opencv::core::Point2f::new(
                br.x as f32 + br.width as f32 / 2.0,
                br.y as f32 + br.height as f32 / 2.0,
            ),
            opencv::core::Size2f::new(br.width as f32, br.height as f32),
            0.0,
        )?
    };

    let offset_x_mm = (rect.center.x as f64 - center_x) * cal.upp_x;
    let offset_y_mm = (rect.center.y as f64 - center_y) * cal.upp_y;

    // Confidence from area match
    let rect_area_mm2 = (rect.size.width as f64 * cal.upp_x) * (rect.size.height as f64 * cal.upp_y);
    let area_ratio = if expected_area_mm2 > 0.0 {
        (rect_area_mm2 / expected_area_mm2).min(expected_area_mm2 / rect_area_mm2)
    } else {
        0.5
    };
    let confidence = (best_score * 0.5 + area_ratio * 0.5).clamp(0.0, 1.0);

    ctx.log(format!(
        "CV: best contour score={:.3}, area={:.1}mm² (expected {:.1}mm²), confidence={:.3}",
        best_score, rect_area_mm2, expected_area_mm2, confidence
    ));

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
