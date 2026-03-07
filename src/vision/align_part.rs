use opencv::core::Mat;
use tracing::debug;

use crate::config::PackageConfig;

use super::context::VisionContext;
use super::cv;
use super::error::VisionError;
use super::ml::{self, SharedSession};
use super::types::{
    AlignmentResult, CameraCalibration, Detection, DetectionMethod, PadDetection, RegionPx,
    VisionConfig,
};

/// Align a part using bottom camera.
///
/// Dual-path strategy:
/// - Path A (pad-aware): Detect individual pads, solve rigid transform
/// - Path B (MinAreaRect): Fallback for simple/unknown geometry
/// - Path C (ML): ONNX model if available
///
/// Returns the best result.
pub fn align_part(
    jpeg: &[u8],
    package: &PackageConfig,
    cal: &CameraCalibration,
    config: &VisionConfig,
    model: Option<&SharedSession>,
) -> Result<AlignmentResult, VisionError> {
    let mut ctx = VisionContext::new(config);
    let image = cv::decode_frame(jpeg)?;
    ctx.checkpoint("input", &image);

    let center_x = cal.width as f64 / 2.0;
    let center_y = cal.height as f64 / 2.0;

    // Try ML first if preferred
    let ml_result = if let Some(session) = model {
        if config.prefer_ml {
            match ml::detect_ml(&image, session, config.confidence_threshold, &mut ctx) {
                Ok(boxes) if !boxes.is_empty() => {
                    let best = &boxes[0];
                    Some(AlignmentResult {
                        detection: Detection {
                            offset_x_mm: (best.x - center_x) * cal.upp_x,
                            offset_y_mm: (best.y - center_y) * cal.upp_y,
                            rotation_deg: best.angle.unwrap_or(0.0),
                            confidence: best.confidence,
                            method: DetectionMethod::OnnxModel {
                                model_name: "yolov8".into(),
                            },
                            region_px: Some(RegionPx {
                                x: best.x, y: best.y,
                                width: best.width, height: best.height,
                                rotation_deg: best.angle.unwrap_or(0.0),
                            }),
                        },
                        pad_detections: Vec::new(),
                    })
                }
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    };

    // CV preprocessing (shared by Path A and B)
    let preprocessed = preprocess_for_alignment(&image, config, &mut ctx)?;

    // Path A: Pad-aware alignment
    let pad_result = if package.pads.len() >= 2 {
        match pad_aware_alignment(&preprocessed, package, cal, config, &mut ctx) {
            Ok(r) => Some(r),
            Err(e) => {
                ctx.log(format!("Pad-aware alignment failed: {}", e));
                None
            }
        }
    } else {
        None
    };

    // Path B: MinAreaRect fallback
    let rect_result = match min_area_rect_alignment(&preprocessed, cal, config, &mut ctx) {
        Ok(r) => Some(r),
        Err(e) => {
            ctx.log(format!("MinAreaRect fallback failed: {}", e));
            None
        }
    };

    // Pick best result
    // Prefer pad-aware if it has high agreement, otherwise ML, otherwise MinAreaRect
    if let Some(ref pad) = pad_result {
        if let DetectionMethod::PadAlignment { agreement, .. } = pad.detection.method {
            if agreement > 0.9 && pad.detection.confidence > 0.7 {
                return Ok(pad.clone());
            }
        }
    }

    if let Some(ml_res) = ml_result {
        if ml_res.detection.confidence > 0.8 {
            return Ok(ml_res);
        }
    }

    if let Some(ref pad) = pad_result {
        if pad.detection.confidence > 0.5 {
            return Ok(pad.clone());
        }
    }

    rect_result.ok_or(VisionError::NoDetection)
}

fn preprocess_for_alignment(
    image: &Mat,
    config: &VisionConfig,
    ctx: &mut VisionContext,
) -> Result<Mat, VisionError> {
    // HSV filter if configured
    let working = if let Some(ref hsv) = config.hsv_filter {
        let mask = cv::hsv_filter(image, hsv)?;
        ctx.checkpoint("hsv_mask", &mask);
        let filtered = cv::apply_mask(image, &mask)?;
        ctx.checkpoint("hsv_filtered", &filtered);
        cv::to_gray(&filtered)?
    } else {
        cv::to_gray(image)?
    };
    ctx.checkpoint("gray", &working);

    let blurred = cv::blur(&working, config.blur_ksize)?;
    ctx.checkpoint("blur", &blurred);

    let thresh = cv::adaptive_threshold(&blurred, config.adaptive_block_size, config.adaptive_c)?;
    ctx.checkpoint("threshold", &thresh);

    // Apply circular mask if configured
    let result = if let Some(diameter) = config.mask_diameter_px {
        let masked = cv::mask_circle(&thresh, diameter)?;
        ctx.checkpoint("masked", &masked);
        masked
    } else {
        thresh
    };

    Ok(result)
}

fn pad_aware_alignment(
    binary: &Mat,
    package: &PackageConfig,
    cal: &CameraCalibration,
    config: &VisionConfig,
    ctx: &mut VisionContext,
) -> Result<AlignmentResult, VisionError> {
    let center_x = cal.width as f64 / 2.0;
    let center_y = cal.height as f64 / 2.0;

    let contours = cv::find_contours(binary)?;
    let filtered = cv::filter_contours_by_area(
        &contours,
        config.contour_area_min_mm2,
        config.contour_area_max_mm2,
        cal,
    );

    ctx.log(format!(
        "Pad alignment: {} contours after area filter",
        filtered.len()
    ));

    if filtered.is_empty() {
        return Err(VisionError::NoDetection);
    }

    // Compute centroids of detected contours
    let mut detected_centroids: Vec<opencv::core::Point2f> = Vec::new();
    for i in 0..filtered.len() {
        if let Ok(contour) = filtered.get(i) {
            if let Some(centroid) = cv::contour_centroid(&contour) {
                detected_centroids.push(centroid);
            }
        }
    }

    if detected_centroids.len() < 2 {
        return Err(VisionError::NoDetection);
    }

    // Expected pad positions in pixel space (relative to image center)
    let expected_px: Vec<(f64, f64)> = package
        .pads
        .iter()
        .map(|p| (center_x + p.x / cal.upp_x, center_y + p.y / cal.upp_y))
        .collect();

    // Simple nearest-neighbor matching
    let mut pad_detections = Vec::new();
    let mut matched_detected: Vec<bool> = vec![false; detected_centroids.len()];

    for (pad_idx, pad) in package.pads.iter().enumerate() {
        let exp_px = expected_px[pad_idx];

        // Find closest unmatched detected centroid
        let mut best_dist = f64::MAX;
        let mut best_idx = None;
        for (det_idx, centroid) in detected_centroids.iter().enumerate() {
            if matched_detected[det_idx] {
                continue;
            }
            let dx = centroid.x as f64 - exp_px.0;
            let dy = centroid.y as f64 - exp_px.1;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist < best_dist {
                best_dist = dist;
                best_idx = Some(det_idx);
            }
        }

        if let Some(det_idx) = best_idx {
            matched_detected[det_idx] = true;
            let det = &detected_centroids[det_idx];
            let det_x_mm = (det.x as f64 - center_x) * cal.upp_x;
            let det_y_mm = (det.y as f64 - center_y) * cal.upp_y;
            let error = ((det_x_mm - pad.x).powi(2) + (det_y_mm - pad.y).powi(2)).sqrt();

            pad_detections.push(PadDetection {
                pad_name: pad.name.clone(),
                detected_x_mm: det_x_mm,
                detected_y_mm: det_y_mm,
                expected_x_mm: pad.x,
                expected_y_mm: pad.y,
                error_mm: error,
            });
        }
    }

    if pad_detections.len() < 2 {
        return Err(VisionError::NoDetection);
    }

    // Solve rigid body transform from expected → detected positions
    let (offset_x, offset_y, rotation) = solve_rigid_transform(&pad_detections);

    // Compute agreement: inverse of mean per-pad error normalized
    let mean_error: f64 =
        pad_detections.iter().map(|p| p.error_mm).sum::<f64>() / pad_detections.len() as f64;
    let agreement = 1.0 / (1.0 + mean_error * 10.0);

    let confidence = agreement;
    let pad_count = pad_detections.len();

    debug!(
        "Pad alignment: offset=({:.3}, {:.3})mm, rot={:.2}°, agreement={:.3}, {} pads matched",
        offset_x, offset_y, rotation, agreement, pad_count
    );

    Ok(AlignmentResult {
        detection: Detection {
            offset_x_mm: offset_x,
            offset_y_mm: offset_y,
            rotation_deg: rotation,
            confidence,
            method: DetectionMethod::PadAlignment {
                pad_count,
                agreement,
            },
            region_px: None,
        },
        pad_detections,
    })
}

fn min_area_rect_alignment(
    binary: &Mat,
    cal: &CameraCalibration,
    config: &VisionConfig,
    ctx: &mut VisionContext,
) -> Result<AlignmentResult, VisionError> {
    let center_x = cal.width as f64 / 2.0;
    let center_y = cal.height as f64 / 2.0;

    let contours = cv::find_contours(binary)?;
    let filtered = cv::filter_contours_by_area(
        &contours,
        config.contour_area_min_mm2,
        config.contour_area_max_mm2,
        cal,
    );

    let rect = cv::fit_min_area_rect(&filtered).ok_or(VisionError::NoDetection)?;

    let offset_x_mm = (rect.center.x as f64 - center_x) * cal.upp_x;
    let offset_y_mm = (rect.center.y as f64 - center_y) * cal.upp_y;

    // OpenCV angle: -90 to 0 for minAreaRect
    let mut angle = rect.angle as f64;
    // Normalize to -45..45 range (components are usually closer to 0° or 90°)
    if angle < -45.0 {
        angle += 90.0;
    }

    ctx.log(format!(
        "MinAreaRect: offset=({:.3}, {:.3})mm, angle={:.2}°",
        offset_x_mm, offset_y_mm, angle
    ));

    Ok(AlignmentResult {
        detection: Detection {
            offset_x_mm,
            offset_y_mm,
            rotation_deg: angle,
            confidence: 0.6,
            method: DetectionMethod::MinAreaRect,
            region_px: Some(RegionPx {
                x: rect.center.x as f64,
                y: rect.center.y as f64,
                width: rect.size.width as f64,
                height: rect.size.height as f64,
                rotation_deg: rect.angle as f64,
            }),
        },
        pad_detections: Vec::new(),
    })
}

/// Solve for the rigid body transform (translation + rotation) that maps
/// expected pad positions to detected pad positions.
fn solve_rigid_transform(pads: &[PadDetection]) -> (f64, f64, f64) {
    if pads.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    if pads.len() == 1 {
        return (
            pads[0].detected_x_mm - pads[0].expected_x_mm,
            pads[0].detected_y_mm - pads[0].expected_y_mm,
            0.0,
        );
    }

    // Compute centroids of expected and detected
    let n = pads.len() as f64;
    let exp_cx: f64 = pads.iter().map(|p| p.expected_x_mm).sum::<f64>() / n;
    let exp_cy: f64 = pads.iter().map(|p| p.expected_y_mm).sum::<f64>() / n;
    let det_cx: f64 = pads.iter().map(|p| p.detected_x_mm).sum::<f64>() / n;
    let det_cy: f64 = pads.iter().map(|p| p.detected_y_mm).sum::<f64>() / n;

    // Solve for rotation using cross-covariance
    let mut sum_sin = 0.0;
    let mut sum_cos = 0.0;
    for p in pads {
        let ex = p.expected_x_mm - exp_cx;
        let ey = p.expected_y_mm - exp_cy;
        let dx = p.detected_x_mm - det_cx;
        let dy = p.detected_y_mm - det_cy;

        sum_cos += ex * dx + ey * dy;
        sum_sin += ex * dy - ey * dx;
    }

    let rotation_rad = sum_sin.atan2(sum_cos);
    let rotation_deg = rotation_rad.to_degrees();

    // Translation = detected centroid - rotated expected centroid
    let cos_r = rotation_rad.cos();
    let sin_r = rotation_rad.sin();
    let rotated_exp_cx = cos_r * exp_cx - sin_r * exp_cy;
    let rotated_exp_cy = sin_r * exp_cx + cos_r * exp_cy;
    let tx = det_cx - rotated_exp_cx;
    let ty = det_cy - rotated_exp_cy;

    (tx, ty, rotation_deg)
}
