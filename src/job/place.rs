use std::time::Duration;

use tracing::{debug, info, warn};

use crate::config::nozzle_tips::NozzleTipConfig;
use crate::config::parts::PartConfig;
use crate::config::PlacementConfig;
use crate::motion::NozzleId;
use crate::state::AppState;

use super::types::{nozzle_z, AlignmentOffset, BoardState};

#[derive(Debug, thiserror::Error)]
pub enum PlaceError {
    #[error("Motion error: {0}")]
    Motion(#[from] crate::motion::MotionError),
    #[error("GCode error: {0}")]
    GCode(#[from] crate::gcode::GCodeError),
    #[error("Actuator error: {0}")]
    Actuator(#[from] crate::actuators::ActuatorError),
    #[error("Part still on nozzle after place (vacuum reading {reading:.1} > {threshold:.1})")]
    PartStillOnNozzle { reading: f64, threshold: f64 },
    #[error("No board transform available")]
    NoTransform,
}

/// Place a part at its board location using the specified nozzle.
///
/// Sequence:
/// 1. Compute final placement position (board transform + alignment correction)
/// 2. Apply nozzle offset and rotation
/// 3. Move to placement XY
/// 4. Lower Z to place height
/// 5. Release part (vacuum off + blow)
/// 6. Retract to safe Z
/// 7. Verify place via vacuum decay
pub async fn place_part(
    nozzle: NozzleId,
    placement: &PlacementConfig,
    board: &BoardState,
    alignment: &AlignmentOffset,
    part: &PartConfig,
    tip: &NozzleTipConfig,
    state: &AppState,
) -> Result<(), PlaceError> {
    let transform = board.transform.as_ref().ok_or(PlaceError::NoTransform)?;

    // Step 1: Compute final position
    let (board_x, board_y) = transform.transform_point(placement.x, placement.y);
    let board_rot = transform.transform_rotation(placement.rotation);

    // Apply alignment correction
    let final_x = board_x + alignment.dx;
    let final_y = board_y + alignment.dy;
    let final_rot = board_rot + alignment.drot;

    // Step 2: Apply nozzle offset
    let config = state.config.read().await;
    let nozzle_config = config.nozzles.get(nozzle.config_key());
    let (offset_x, offset_y) = nozzle_config
        .map(|n| (n.head_offset.x, n.head_offset.y))
        .unwrap_or((0.0, 0.0));
    let safe_z = config.motion.safe_z;
    let default_feedrate = config.motion.default_feedrate;
    drop(config);

    let head_x = final_x - offset_x;
    let head_y = final_y - offset_y;

    debug!(
        "Place {}: {} at ({:.3}, {:.3}) rot={:.1}°, part height={:.2}",
        nozzle, placement.reference, final_x, final_y, final_rot, part.height
    );

    // Set nozzle rotation
    let rotation_axis = match nozzle {
        NozzleId::N1 => (Some(final_rot), None),
        NozzleId::N2 => (None, Some(final_rot)),
    };

    // Step 3: Move to placement XY
    state.motion.move_safe(head_x, head_y).await?;
    state
        .motion
        .move_to(None, None, None, rotation_axis.0, rotation_axis.1, None)
        .await?;
    state.gcode.wait().await?;

    // Step 4: Lower Z to place height
    let place_z = part.height;
    let z = nozzle_z(nozzle, place_z);
    let place_feedrate = default_feedrate * part.speed;
    state
        .motion
        .move_to(None, None, Some(z), None, None, Some(place_feedrate))
        .await?;
    state.gcode.wait().await?;

    // Step 5: Release part
    state.actuators.vacuum_off(nozzle).await?;
    state.actuators.blow(nozzle, 100).await?;
    tokio::time::sleep(Duration::from_millis(tip.place_dwell_ms as u64)).await;

    // Step 6: Retract to safe Z
    state
        .motion
        .move_to(None, None, Some(safe_z), None, None, None)
        .await?;
    state.gcode.wait().await?;

    // Step 7: Verify place via vacuum decay
    verify_vacuum_place(nozzle, tip, state).await?;

    info!(
        "Place {} complete: {} at ({:.3}, {:.3})",
        nozzle, placement.reference, final_x, final_y
    );
    Ok(())
}

/// Verify that the part was released by checking vacuum has decayed.
async fn verify_vacuum_place(
    nozzle: NozzleId,
    tip: &NozzleTipConfig,
    state: &AppState,
) -> Result<(), PlaceError> {
    let vacuum = match &tip.vacuum {
        Some(v) => v,
        None => return Ok(()),
    };

    let reading = state.actuators.vacuum_read(nozzle).await?;

    if reading > vacuum.part_off_high {
        warn!(
            "Part may still be on nozzle {}: vacuum={:.1} > threshold={:.1}",
            nozzle, reading, vacuum.part_off_high
        );
        return Err(PlaceError::PartStillOnNozzle {
            reading,
            threshold: vacuum.part_off_high,
        });
    }

    debug!("Place verify passed for {}: vacuum={:.1}", nozzle, reading);
    Ok(())
}
