use tracing::{debug, info};

use crate::config::boards::FiducialConfig;
use crate::config::jobs::BoardOrigin;
use crate::state::AppState;
use crate::vision::CameraCalibration;

use super::types::AffineTransform;

#[derive(Debug, thiserror::Error)]
pub enum BoardError {
    #[error("Motion error: {0}")]
    Motion(#[from] crate::motion::MotionError),
    #[error("Vision error: {0}")]
    Vision(#[from] crate::vision::VisionError),
    #[error("No camera configured")]
    NoCamera,
    #[error("No vision engine")]
    NoVision,
    #[error("Fiducial not detected: {0}")]
    FiducialNotFound(String),
    #[error("Need at least 2 fiducials for board registration")]
    InsufficientFiducials,
}

/// Locate fiducials on a board and compute the affine transform.
///
/// Process:
/// 1. For each fiducial: move camera to expected location, capture, detect offset
/// 2. Compute affine transform from expected → actual positions
pub async fn locate_board(
    fiducials: &[FiducialConfig],
    origin: &BoardOrigin,
    state: &AppState,
) -> Result<AffineTransform, BoardError> {
    if fiducials.len() < 2 {
        return Err(BoardError::InsufficientFiducials);
    }

    let camera = state.camera.as_ref().ok_or(BoardError::NoCamera)?;

    // Get top camera config for calibration
    let config = state.config.read().await;
    let cam_config = config
        .cameras
        .get("top")
        .ok_or(BoardError::NoCamera)?;
    let cal = CameraCalibration::from(cam_config);
    drop(config);

    let mut expected = Vec::new();
    let mut actual = Vec::new();

    for fid in fiducials {
        // Expected machine position = board origin + fiducial offset (with rotation)
        let origin_transform =
            AffineTransform::from_translation_rotation(origin.x, origin.y, origin.rotation);
        let (expect_x, expect_y) = origin_transform.transform_point(fid.x, fid.y);

        info!(
            "Locating fiducial {}: expected ({:.3}, {:.3})",
            fid.reference, expect_x, expect_y
        );

        // Move camera to expected position
        state.motion.move_safe(expect_x, expect_y).await?;

        // Capture and detect
        let jpeg = camera
            .capture("top")
            .await
            .map_err(|e| BoardError::Vision(crate::vision::VisionError::Other(e.to_string())))?;

        let detection = crate::vision::detect_fiducial(
            &jpeg,
            &cal,
            fid.diameter_mm * 0.5,
            fid.diameter_mm * 2.0,
            None,
        )?;

        if detection.confidence < 0.3 {
            return Err(BoardError::FiducialNotFound(fid.reference.clone()));
        }

        // Actual position = camera position + detected offset
        let actual_x = expect_x + detection.offset_x_mm;
        let actual_y = expect_y + detection.offset_y_mm;

        info!(
            "Fiducial {} located at ({:.3}, {:.3}), offset=({:.3}, {:.3}), confidence={:.2}",
            fid.reference, actual_x, actual_y, detection.offset_x_mm, detection.offset_y_mm,
            detection.confidence
        );

        expected.push((fid.x, fid.y));
        actual.push((actual_x, actual_y));
    }

    let transform = if expected.len() >= 3 {
        compute_transform_3pt(
            [expected[0], expected[1], expected[2]],
            [actual[0], actual[1], actual[2]],
        )
    } else {
        compute_transform_2pt([expected[0], expected[1]], [actual[0], actual[1]])
    };

    debug!(
        "Board transform: rotation={:.3}°, tx={:.3}, ty={:.3}",
        transform.rotation_deg(),
        transform.tx,
        transform.ty
    );

    Ok(transform)
}

/// Compute rigid body transform (translation + rotation + uniform scale) from 2 point pairs.
pub fn compute_transform_2pt(
    expected: [(f64, f64); 2],
    actual: [(f64, f64); 2],
) -> AffineTransform {
    let (ex0, ey0) = expected[0];
    let (ex1, ey1) = expected[1];
    let (ax0, ay0) = actual[0];
    let (ax1, ay1) = actual[1];

    // Vector from point 0 to point 1 in each space
    let edx = ex1 - ex0;
    let edy = ey1 - ey0;
    let adx = ax1 - ax0;
    let ady = ay1 - ay0;

    // Scale + rotation
    let e_len_sq = edx * edx + edy * edy;
    if e_len_sq < 1e-12 {
        // Degenerate case: fiducials are at the same point
        return AffineTransform::translation(ax0 - ex0, ay0 - ey0);
    }

    // Solve for a, b where: [a -b; b a] * [edx; edy] = [adx; ady]
    let a = (edx * adx + edy * ady) / e_len_sq;
    let b = (edx * ady - edy * adx) / e_len_sq;

    // Translation: actual[0] = [a -b; b a] * expected[0] + [tx; ty]
    let tx = ax0 - (a * ex0 - b * ey0);
    let ty = ay0 - (b * ex0 + a * ey0);

    AffineTransform {
        a,
        b: -b,
        c: b,
        d: a,
        tx,
        ty,
    }
}

/// Compute full affine transform (6 parameters) from 3 point pairs.
pub fn compute_transform_3pt(
    expected: [(f64, f64); 3],
    actual: [(f64, f64); 3],
) -> AffineTransform {
    let (x0, y0) = expected[0];
    let (x1, y1) = expected[1];
    let (x2, y2) = expected[2];
    let (u0, v0) = actual[0];
    let (u1, v1) = actual[1];
    let (u2, v2) = actual[2];

    // Solve: [u] = [a b tx] [x]
    //        [v]   [c d ty] [y]
    //                       [1]
    //
    // Using Cramer's rule on the 3×3 system for each output dimension.
    let det = x0 * (y1 - y2) - x1 * (y0 - y2) + x2 * (y0 - y1);

    if det.abs() < 1e-12 {
        // Degenerate: points are collinear, fall back to 2-point
        return compute_transform_2pt(
            [expected[0], expected[1]],
            [actual[0], actual[1]],
        );
    }

    let inv_det = 1.0 / det;

    // Solve for a, b, tx (maps x,y → u)
    let a = ((u0 * (y1 - y2) - u1 * (y0 - y2) + u2 * (y0 - y1)) * inv_det);
    let b = ((x0 * (u1 - u2) - x1 * (u0 - u2) + x2 * (u0 - u1)) * inv_det);
    let tx = ((x0 * (y1 * u2 - y2 * u1) - x1 * (y0 * u2 - y2 * u0) + x2 * (y0 * u1 - y1 * u0))
        * inv_det);

    // Solve for c, d, ty (maps x,y → v)
    let c = ((v0 * (y1 - y2) - v1 * (y0 - y2) + v2 * (y0 - y1)) * inv_det);
    let d = ((x0 * (v1 - v2) - x1 * (v0 - v2) + x2 * (v0 - v1)) * inv_det);
    let ty = ((x0 * (y1 * v2 - y2 * v1) - x1 * (y0 * v2 - y2 * v0) + x2 * (y0 * v1 - y1 * v0))
        * inv_det);

    AffineTransform {
        a,
        b,
        c,
        d,
        tx,
        ty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_2pt_identity() {
        let t = compute_transform_2pt([(0.0, 0.0), (100.0, 0.0)], [(0.0, 0.0), (100.0, 0.0)]);
        let (x, y) = t.transform_point(50.0, 25.0);
        assert!((x - 50.0).abs() < 0.001);
        assert!((y - 25.0).abs() < 0.001);
    }

    #[test]
    fn test_2pt_translation() {
        let t = compute_transform_2pt([(0.0, 0.0), (100.0, 0.0)], [(10.0, 5.0), (110.0, 5.0)]);
        let (x, y) = t.transform_point(0.0, 0.0);
        assert!((x - 10.0).abs() < 0.001);
        assert!((y - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_2pt_rotation_90() {
        // Expected: (0,0) and (100,0)
        // Actual: (0,0) and (0,100) — board rotated 90° CCW
        let t = compute_transform_2pt([(0.0, 0.0), (100.0, 0.0)], [(0.0, 0.0), (0.0, 100.0)]);
        let (x, y) = t.transform_point(50.0, 0.0);
        assert!(x.abs() < 0.001);
        assert!((y - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_2pt_translation_and_rotation() {
        // Board origin at (100, 50), rotated 0°
        let t = compute_transform_2pt(
            [(5.0, 5.0), (95.0, 55.0)],
            [(105.0, 55.0), (195.0, 105.0)],
        );
        // Point (50, 30) in board space should map to (150, 80) in machine space
        let (x, y) = t.transform_point(50.0, 30.0);
        assert!((x - 150.0).abs() < 0.01);
        assert!((y - 80.0).abs() < 0.01);
    }

    #[test]
    fn test_3pt_identity() {
        let t = compute_transform_3pt(
            [(0.0, 0.0), (100.0, 0.0), (0.0, 100.0)],
            [(0.0, 0.0), (100.0, 0.0), (0.0, 100.0)],
        );
        let (x, y) = t.transform_point(50.0, 50.0);
        assert!((x - 50.0).abs() < 0.001);
        assert!((y - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_3pt_translation() {
        let t = compute_transform_3pt(
            [(0.0, 0.0), (100.0, 0.0), (0.0, 100.0)],
            [(10.0, 20.0), (110.0, 20.0), (10.0, 120.0)],
        );
        let (x, y) = t.transform_point(0.0, 0.0);
        assert!((x - 10.0).abs() < 0.001);
        assert!((y - 20.0).abs() < 0.001);
    }

    #[test]
    fn test_3pt_with_scale() {
        // Actual positions are 2x the expected
        let t = compute_transform_3pt(
            [(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)],
            [(0.0, 0.0), (20.0, 0.0), (0.0, 20.0)],
        );
        let (x, y) = t.transform_point(5.0, 5.0);
        assert!((x - 10.0).abs() < 0.001);
        assert!((y - 10.0).abs() < 0.001);
    }
}
