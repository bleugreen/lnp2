use opencv::core::{self, Mat, Point, Point2f, Scalar, Size, Size2f, Vector, RotatedRect};
use opencv::imgcodecs;
use opencv::imgproc;
use opencv::prelude::*;

use super::error::VisionError;
use super::types::CameraCalibration;

/// Decode JPEG bytes into an OpenCV Mat.
pub fn decode_frame(jpeg: &[u8]) -> Result<Mat, VisionError> {
    let buf = Vector::<u8>::from_slice(jpeg);
    let mat = imgcodecs::imdecode(&buf, imgcodecs::IMREAD_COLOR)?;
    if mat.empty() {
        return Err(VisionError::Decode("Failed to decode JPEG".into()));
    }
    Ok(mat)
}

/// Convert BGR image to grayscale.
pub fn to_gray(image: &Mat) -> Result<Mat, VisionError> {
    let mut gray = Mat::default();
    imgproc::cvt_color(image, &mut gray, imgproc::COLOR_BGR2GRAY, 0)?;
    Ok(gray)
}

/// Apply Gaussian blur. `ksize` must be odd.
pub fn blur(image: &Mat, ksize: i32) -> Result<Mat, VisionError> {
    let mut blurred = Mat::default();
    imgproc::gaussian_blur(
        image,
        &mut blurred,
        Size::new(ksize, ksize),
        0.0,
        0.0,
        core::BORDER_DEFAULT,
    )?;
    Ok(blurred)
}

/// Adaptive threshold — robust to lighting gradients.
pub fn adaptive_threshold(image: &Mat, block_size: i32, c: f64) -> Result<Mat, VisionError> {
    let mut thresh = Mat::default();
    imgproc::adaptive_threshold(
        image,
        &mut thresh,
        255.0,
        imgproc::ADAPTIVE_THRESH_GAUSSIAN_C,
        imgproc::THRESH_BINARY_INV,
        block_size,
        c,
    )?;
    Ok(thresh)
}

/// Fixed threshold (fallback).
pub fn fixed_threshold(image: &Mat, value: f64) -> Result<Mat, VisionError> {
    let mut thresh = Mat::default();
    imgproc::threshold(image, &mut thresh, value, 255.0, imgproc::THRESH_BINARY_INV)?;
    Ok(thresh)
}

/// Circular mask centered on the image.
pub fn mask_circle(image: &Mat, diameter_px: i32) -> Result<Mat, VisionError> {
    let rows = image.rows();
    let cols = image.cols();
    let mut mask = Mat::zeros(rows, cols, core::CV_8UC1)?.to_mat()?;
    let center = Point::new(cols / 2, rows / 2);
    imgproc::circle(
        &mut mask,
        center,
        diameter_px / 2,
        Scalar::all(255.0),
        -1, // filled
        imgproc::LINE_8,
        0,
    )?;
    let mut result = Mat::default();
    core::bitwise_and(image, image, &mut result, &mask)?;
    Ok(result)
}

/// HSV color range filter — returns binary mask.
pub fn hsv_filter(image: &Mat, range: &super::types::HsvRange) -> Result<Mat, VisionError> {
    let mut hsv = Mat::default();
    imgproc::cvt_color(image, &mut hsv, imgproc::COLOR_BGR2HSV, 0)?;
    let mut mask = Mat::default();
    let lower = Scalar::new(range.h_min, range.s_min, range.v_min, 0.0);
    let upper = Scalar::new(range.h_max, range.s_max, range.v_max, 0.0);
    core::in_range(&hsv, &lower, &upper, &mut mask)?;
    Ok(mask)
}

/// Apply a binary mask to an image.
pub fn apply_mask(image: &Mat, mask: &Mat) -> Result<Mat, VisionError> {
    let mut result = Mat::default();
    core::bitwise_and(image, image, &mut result, mask)?;
    Ok(result)
}

/// Find external contours from a binary image.
pub fn find_contours(binary: &Mat) -> Result<Vector<Vector<Point>>, VisionError> {
    let mut contours = Vector::<Vector<Point>>::new();
    let mut binary_clone = binary.clone();
    imgproc::find_contours(
        &mut binary_clone,
        &mut contours,
        imgproc::RETR_EXTERNAL,
        imgproc::CHAIN_APPROX_SIMPLE,
        Point::new(0, 0),
    )?;
    Ok(contours)
}

/// Filter contours by area (in mm², using calibration to convert).
pub fn filter_contours_by_area(
    contours: &Vector<Vector<Point>>,
    min_mm2: f64,
    max_mm2: f64,
    cal: &CameraCalibration,
) -> Vector<Vector<Point>> {
    let px_per_mm2 = 1.0 / (cal.upp_x * cal.upp_y);
    let min_px2 = min_mm2 * px_per_mm2;
    let max_px2 = max_mm2 * px_per_mm2;

    let mut filtered = Vector::<Vector<Point>>::new();
    for i in 0..contours.len() {
        if let Ok(contour) = contours.get(i) {
            let area = imgproc::contour_area(&contour, false).unwrap_or(0.0);
            if area >= min_px2 && area <= max_px2 {
                filtered.push(contour);
            }
        }
    }
    filtered
}

/// Fit minimum area rectangle to the largest contour.
pub fn fit_min_area_rect(
    contours: &Vector<Vector<Point>>,
) -> Option<RotatedRect> {
    if contours.is_empty() {
        return None;
    }

    let mut best_idx = 0;
    let mut best_area = 0.0f64;
    for i in 0..contours.len() {
        if let Ok(contour) = contours.get(i) {
            let area = imgproc::contour_area(&contour, false).unwrap_or(0.0);
            if area > best_area {
                best_area = area;
                best_idx = i;
            }
        }
    }

    contours
        .get(best_idx)
        .ok()
        .and_then(|c| {
            if c.len() >= 5 {
                imgproc::min_area_rect(&c).ok()
            } else {
                // For fewer than 5 points, use bounding_rect
                let rect = imgproc::bounding_rect(&c).ok()?;
                RotatedRect::new(
                    Point2f::new(
                        rect.x as f32 + rect.width as f32 / 2.0,
                        rect.y as f32 + rect.height as f32 / 2.0,
                    ),
                    Size2f::new(rect.width as f32, rect.height as f32),
                    0.0,
                ).ok()
            }
        })
}

/// Circular symmetry detection for fiducials.
/// Uses HoughCircles to find circular features.
/// Returns (center, diameter_px, score).
pub fn detect_circular_symmetry(
    gray: &Mat,
    min_diameter_px: f64,
    max_diameter_px: f64,
    min_score: f64,
) -> Result<Option<(Point2f, f64, f64)>, VisionError> {
    let mut circles = Mat::default();
    imgproc::hough_circles(
        gray,
        &mut circles,
        imgproc::HOUGH_GRADIENT,
        1.0,                    // dp
        min_diameter_px,        // min_dist between centers
        100.0,                  // param1 (Canny upper threshold)
        min_score * 20.0,       // param2 (accumulator threshold)
        (min_diameter_px / 2.0) as i32,
        (max_diameter_px / 2.0) as i32,
    )?;

    if circles.cols() == 0 {
        return Ok(None);
    }

    // HoughCircles returns a 1xN matrix with 3 channels (x, y, radius)
    let circle: &core::Vec3f = circles.at_2d(0, 0)?;
    let center = Point2f::new(circle[0], circle[1]);
    let diameter = circle[2] as f64 * 2.0;
    let score = 1.0;

    Ok(Some((center, diameter, score)))
}

/// Template matching — returns best match location and score.
pub fn template_match(image: &Mat, template: &Mat) -> Result<(Point, f64), VisionError> {
    let mut result = Mat::default();
    imgproc::match_template(
        image,
        template,
        &mut result,
        imgproc::TM_CCOEFF_NORMED,
        &Mat::default(),
    )?;

    let mut min_val = 0.0f64;
    let mut max_val = 0.0f64;
    let mut min_loc = Point::default();
    let mut max_loc = Point::default();
    core::min_max_loc(
        &result,
        Some(&mut min_val),
        Some(&mut max_val),
        Some(&mut min_loc),
        Some(&mut max_loc),
        &Mat::default(),
    )?;

    // Adjust to center of template
    let tw = template.cols();
    let th = template.rows();
    let center = Point::new(max_loc.x + tw / 2, max_loc.y + th / 2);

    Ok((center, max_val))
}

/// Compute the centroid of a contour.
pub fn contour_centroid(contour: &Vector<Point>) -> Option<Point2f> {
    let moments = imgproc::moments(contour, false).ok()?;
    if moments.m00.abs() < 1e-10 {
        return None;
    }
    Some(Point2f::new(
        (moments.m10 / moments.m00) as f32,
        (moments.m01 / moments.m00) as f32,
    ))
}
