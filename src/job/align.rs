use std::time::Duration;

use tracing::{debug, info};

use crate::config::PackageConfig;
use crate::motion::NozzleId;
use crate::state::AppState;
use crate::vision::CameraCalibration;

use super::types::{nozzle_z, AlignmentOffset};

#[derive(Debug, thiserror::Error)]
pub enum AlignError {
    #[error("Motion error: {0}")]
    Motion(#[from] crate::motion::MotionError),
    #[error("GCode error: {0}")]
    GCode(#[from] crate::gcode::GCodeError),
    #[error("Actuator error: {0}")]
    Actuator(#[from] crate::actuators::ActuatorError),
    #[error("Vision error: {0}")]
    Vision(#[from] crate::vision::VisionError),
    #[error("No bottom camera configured")]
    NoBottomCamera,
    #[error("No camera available")]
    NoCamera,
}

/// Align a part on the nozzle using bottom camera vision.
///
/// Sequence:
/// 1. Move nozzle over bottom camera
/// 2. Lower nozzle to camera focus Z
/// 3. Turn on LED illumination
/// 4. Capture + run alignment
/// 5. Turn off LED
/// 6. Retract to safe Z
/// 7. Return offset corrections
pub async fn align_part(
    nozzle: NozzleId,
    package: &PackageConfig,
    pre_rotation: f64,
    state: &AppState,
) -> Result<AlignmentOffset, AlignError> {
    let camera_mgr = state.camera.as_ref().ok_or(AlignError::NoCamera)?;

    let config = state.config.read().await;
    let cam_config = config
        .cameras
        .get("bottom")
        .ok_or(AlignError::NoBottomCamera)?;
    let cam_location = cam_config
        .location
        .as_ref()
        .ok_or(AlignError::NoBottomCamera)?;

    let cal = CameraCalibration::from(cam_config);
    let vision_config = cam_config.vision.clone().unwrap_or_default();
    let cam_x = cam_location.x;
    let cam_y = cam_location.y;
    let cam_z = cam_location.z;
    let safe_z = config.motion.safe_z;

    // Get nozzle offset
    let nozzle_config = config.nozzles.get(nozzle.config_key());
    let (offset_x, offset_y) = nozzle_config
        .map(|n| (n.head_offset.x, n.head_offset.y))
        .unwrap_or((0.0, 0.0));
    drop(config);

    // Step 1: Move nozzle over bottom camera
    let head_x = cam_x - offset_x;
    let head_y = cam_y - offset_y;
    state.motion.move_safe(head_x, head_y).await?;

    // Pre-rotate nozzle to approximate placement angle
    let rotation_axis = match nozzle {
        NozzleId::N1 => (Some(pre_rotation), None),
        NozzleId::N2 => (None, Some(pre_rotation)),
    };
    state
        .motion
        .move_to(None, None, None, rotation_axis.0, rotation_axis.1, None)
        .await?;
    state.gcode.wait().await?;

    // Step 2: Lower nozzle to camera focus Z
    let z = nozzle_z(nozzle, cam_z);
    state
        .motion
        .move_to(None, None, Some(z), None, None, None)
        .await?;
    state.gcode.wait().await?;

    // Step 3: LED on
    state.actuators.led_on(255, 255, 255, 255).await?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Step 4: Capture + align
    let jpeg = camera_mgr
        .capture("bottom")
        .await
        .map_err(|e| AlignError::Vision(crate::vision::VisionError::Other(e.to_string())))?;

    let vision = state.vision.clone();
    let package_clone = package.clone();
    let result = tokio::task::spawn_blocking(move || {
        let session = vision.as_ref().and_then(|v| v.session_for("bottom"));
        crate::vision::align_part(&jpeg, &package_clone, &cal, &vision_config, session)
    })
    .await
    .map_err(|e| AlignError::Vision(crate::vision::VisionError::Other(e.to_string())))?
    .map_err(AlignError::Vision)?;

    // Step 5: LED off
    state.actuators.led_off().await?;

    // Step 6: Retract
    state
        .motion
        .move_to(None, None, Some(safe_z), None, None, None)
        .await?;
    state.gcode.wait().await?;

    let offset = AlignmentOffset {
        dx: result.detection.offset_x_mm,
        dy: result.detection.offset_y_mm,
        drot: result.detection.rotation_deg,
    };

    info!(
        "Alignment {} complete: offset=({:.3}, {:.3})mm, rot={:.2}°, confidence={:.2}",
        nozzle, offset.dx, offset.dy, offset.drot, result.detection.confidence
    );

    Ok(offset)
}
