use std::time::Duration;

use tracing::{debug, info, warn};

use crate::config::feeders::{FeederConfig, PhotonFeederConfig, TrayFeederConfig};
use crate::config::nozzle_tips::NozzleTipConfig;
use crate::motion::NozzleId;
use crate::state::AppState;

use super::types::nozzle_z;

#[derive(Debug, thiserror::Error)]
pub enum PickError {
    #[error("Motion error: {0}")]
    Motion(#[from] crate::motion::MotionError),
    #[error("GCode error: {0}")]
    GCode(#[from] crate::gcode::GCodeError),
    #[error("Actuator error: {0}")]
    Actuator(#[from] crate::actuators::ActuatorError),
    #[error("Feeder error: {0}")]
    Feeder(String),
    #[error("Vacuum check failed: reading {reading:.1} outside [{low:.1}, {high:.1}]")]
    VacuumCheckFailed {
        reading: f64,
        low: f64,
        high: f64,
    },
}

/// Pick a part from a feeder using the specified nozzle.
///
/// Sequence:
/// 1. Feed part (Photon feeders only)
/// 2. Move to pick location
/// 3. Lower Z to pick height
/// 4. Vacuum on + dwell
/// 5. Retract to safe Z
/// 6. Verify pick via vacuum reading
pub async fn pick_part(
    nozzle: NozzleId,
    feeder: &FeederConfig,
    tip: &NozzleTipConfig,
    state: &AppState,
) -> Result<(), PickError> {
    match feeder {
        FeederConfig::Photon(photon) => pick_from_photon(nozzle, photon, tip, state).await,
        FeederConfig::Tray(tray) => pick_from_tray(nozzle, tray, tip, state).await,
    }
}

async fn pick_from_photon(
    nozzle: NozzleId,
    feeder: &PhotonFeederConfig,
    tip: &NozzleTipConfig,
    state: &AppState,
) -> Result<(), PickError> {
    // Step 1: Feed part
    let pitch_tenths = (feeder.part_pitch * 10.0).round() as u8;

    // Use a PhotonBus for the feed command
    let bus = crate::photon::PhotonBus::new(state.gcode.clone());

    let mut feed_attempts = 0;
    let max_feed_retries = feeder.feed_retry_count;
    loop {
        match bus.feed_and_wait(feeder.slot_address, pitch_tenths).await {
            Ok(()) => break,
            Err(e) => {
                feed_attempts += 1;
                if feed_attempts > max_feed_retries {
                    return Err(PickError::Feeder(format!(
                        "Feed failed after {} attempts: {}",
                        feed_attempts, e
                    )));
                }
                warn!(
                    "Feed attempt {}/{} failed: {}, retrying",
                    feed_attempts, max_feed_retries, e
                );
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }

    // Step 2: Move to pick location (apply nozzle offset)
    let config = state.config.read().await;
    let nozzle_config = config.nozzles.get(nozzle.config_key());
    let (offset_x, offset_y) = nozzle_config
        .map(|n| (n.head_offset.x, n.head_offset.y))
        .unwrap_or((0.0, 0.0));
    let safe_z = config.motion.safe_z;
    drop(config);

    let head_x = feeder.location.x - offset_x;
    let head_y = feeder.location.y - offset_y;

    debug!(
        "Pick {}: moving to ({:.3}, {:.3}), feeder z={:.3}",
        nozzle, head_x, head_y, feeder.location.z
    );

    state.motion.move_safe(head_x, head_y).await?;

    // Step 3: Lower Z to pick height
    let z = nozzle_z(nozzle, feeder.location.z);
    state
        .motion
        .move_to(None, None, Some(z), None, None, None)
        .await?;
    state.gcode.wait().await?;

    // Step 4: Vacuum on + dwell
    state.actuators.vacuum_on(nozzle).await?;
    tokio::time::sleep(Duration::from_millis(tip.pick_dwell_ms as u64)).await;

    // Step 5: Retract to safe Z
    state
        .motion
        .move_to(None, None, Some(safe_z), None, None, None)
        .await?;
    state.gcode.wait().await?;

    // Step 6: Verify pick via vacuum
    verify_vacuum_pick(nozzle, tip, state).await?;

    info!("Pick {} complete from slot {}", nozzle, feeder.slot_address);
    Ok(())
}

async fn pick_from_tray(
    nozzle: NozzleId,
    feeder: &TrayFeederConfig,
    tip: &NozzleTipConfig,
    state: &AppState,
) -> Result<(), PickError> {
    // Tray feeders: pick from the current tray position
    // (tray index tracking would be managed by the job runner)
    let config = state.config.read().await;
    let nozzle_config = config.nozzles.get(nozzle.config_key());
    let (offset_x, offset_y) = nozzle_config
        .map(|n| (n.head_offset.x, n.head_offset.y))
        .unwrap_or((0.0, 0.0));
    let safe_z = config.motion.safe_z;
    drop(config);

    let head_x = feeder.location.x - offset_x;
    let head_y = feeder.location.y - offset_y;

    state.motion.move_safe(head_x, head_y).await?;

    let z = nozzle_z(nozzle, feeder.location.z);
    state
        .motion
        .move_to(None, None, Some(z), None, None, None)
        .await?;
    state.gcode.wait().await?;

    state.actuators.vacuum_on(nozzle).await?;
    tokio::time::sleep(Duration::from_millis(tip.pick_dwell_ms as u64)).await;

    state
        .motion
        .move_to(None, None, Some(safe_z), None, None, None)
        .await?;
    state.gcode.wait().await?;

    verify_vacuum_pick(nozzle, tip, state).await?;

    info!("Pick {} complete from tray", nozzle);
    Ok(())
}

/// Verify that a part was successfully picked by checking the vacuum level.
async fn verify_vacuum_pick(
    nozzle: NozzleId,
    tip: &NozzleTipConfig,
    state: &AppState,
) -> Result<(), PickError> {
    let vacuum = match &tip.vacuum {
        Some(v) => v,
        None => return Ok(()), // No vacuum thresholds configured — skip verification
    };

    let reading = state.actuators.vacuum_read(nozzle).await?;

    if reading < vacuum.part_on_low || reading > vacuum.part_on_high {
        debug!(
            "Vacuum check failed for {}: {:.1} not in [{:.1}, {:.1}]",
            nozzle, reading, vacuum.part_on_low, vacuum.part_on_high
        );
        return Err(PickError::VacuumCheckFailed {
            reading,
            low: vacuum.part_on_low,
            high: vacuum.part_on_high,
        });
    }

    debug!("Vacuum check passed for {}: {:.1}", nozzle, reading);
    Ok(())
}

/// Pick with retry logic. Returns Ok if pick succeeds within retry limit.
pub async fn pick_with_retry(
    nozzle: NozzleId,
    feeder: &FeederConfig,
    tip: &NozzleTipConfig,
    state: &AppState,
    max_retries: u32,
) -> Result<u32, PickError> {
    let mut retries = 0;

    loop {
        match pick_part(nozzle, feeder, tip, state).await {
            Ok(()) => return Ok(retries),
            Err(PickError::VacuumCheckFailed { .. }) if retries < max_retries => {
                retries += 1;
                warn!(
                    "Pick retry {}/{} for {}",
                    retries, max_retries, nozzle
                );
                // Turn vacuum off, blow, wait, then retry
                let _ = state.actuators.vacuum_off(nozzle).await;
                let _ = state.actuators.blow(nozzle, 100).await;
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(e) => return Err(e),
        }
    }
}
