use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::config::nozzle_tips::ChangerConfig;
use crate::config::{FullConfig, MachineConfig};
use crate::gcode::{GCodeDriver, GCodeError};
use crate::motion::MotionController;

#[derive(Debug, thiserror::Error)]
pub enum ChangerError {
    #[error("GCode error: {0}")]
    GCode(#[from] GCodeError),
    #[error("Motion error: {0}")]
    Motion(#[from] crate::motion::MotionError),
    #[error("Nozzle tip not found: {0}")]
    TipNotFound(String),
    #[error("No changer config for tip: {0}")]
    NoChangerConfig(String),
}

pub struct NozzleTipChanger {
    gcode: Arc<GCodeDriver>,
    motion: Arc<MotionController>,
    full_config: Arc<RwLock<FullConfig>>,
}

impl NozzleTipChanger {
    pub fn new(
        gcode: Arc<GCodeDriver>,
        motion: Arc<MotionController>,
        full_config: Arc<RwLock<FullConfig>>,
    ) -> Self {
        Self {
            gcode,
            motion,
            full_config,
        }
    }

    /// Load a nozzle tip using the 4-waypoint changer sequence:
    /// 1. Move to safe Z
    /// 2. Move XY to first waypoint
    /// 3. Lower Z to first (approach height)
    /// 4. Move to second at speed_1_to_2 (engage)
    /// 5. Move to third at speed_2_to_3 (lift slightly)
    /// 6. Move to last at speed_3_to_4 (pull away)
    /// 7. Retract to safe Z
    pub async fn load_tip(&self, tip_name: &str) -> Result<(), ChangerError> {
        let changer = self.get_changer_config(tip_name).await?;
        let safe_z = self.safe_z().await;
        let max_feedrate = self.max_feedrate().await;

        info!("Loading nozzle tip: {}", tip_name);

        // Retract to safe Z first
        self.move_z(safe_z, max_feedrate).await?;
        self.gcode.wait().await?;

        // Move XY to first waypoint
        self.move_xy(changer.first.x, changer.first.y, max_feedrate)
            .await?;
        self.gcode.wait().await?;

        // Lower to first Z (approach)
        self.move_z(changer.first.z, max_feedrate).await?;
        self.gcode.wait().await?;

        if let Some(cmd) = &changer.post_step_1 {
            if !cmd.is_empty() {
                self.gcode.send(cmd).await?;
            }
        }

        // Move to second (engage — slow)
        let f1 = max_feedrate * changer.speed_1_to_2;
        self.move_xyz(changer.second.x, changer.second.y, changer.second.z, f1)
            .await?;
        self.gcode.wait().await?;

        if let Some(cmd) = &changer.post_step_2 {
            if !cmd.is_empty() {
                self.gcode.send(cmd).await?;
            }
        }

        // Move to third (lift slightly)
        let f2 = max_feedrate * changer.speed_2_to_3;
        self.move_xyz(changer.third.x, changer.third.y, changer.third.z, f2)
            .await?;
        self.gcode.wait().await?;

        if let Some(cmd) = &changer.post_step_3 {
            if !cmd.is_empty() {
                self.gcode.send(cmd).await?;
            }
        }

        // Move to last (pull away)
        let f3 = max_feedrate * changer.speed_3_to_4;
        self.move_xyz(changer.last.x, changer.last.y, changer.last.z, f3)
            .await?;
        self.gcode.wait().await?;

        // Retract to safe Z
        self.move_z(safe_z, max_feedrate).await?;
        self.gcode.wait().await?;

        info!("Loaded nozzle tip: {}", tip_name);
        Ok(())
    }

    /// Unload a nozzle tip — reverse of load sequence:
    /// last → third → second → first → safe Z
    pub async fn unload_tip(&self, tip_name: &str) -> Result<(), ChangerError> {
        let changer = self.get_changer_config(tip_name).await?;
        let safe_z = self.safe_z().await;
        let max_feedrate = self.max_feedrate().await;

        info!("Unloading nozzle tip: {}", tip_name);

        // Retract to safe Z
        self.move_z(safe_z, max_feedrate).await?;
        self.gcode.wait().await?;

        // Move XY to last waypoint (entry for unload)
        self.move_xy(changer.last.x, changer.last.y, max_feedrate)
            .await?;
        self.gcode.wait().await?;

        // Lower to last Z
        self.move_z(changer.last.z, max_feedrate).await?;
        self.gcode.wait().await?;

        // Move to third (approach slot)
        let f3 = max_feedrate * changer.speed_3_to_4;
        self.move_xyz(changer.third.x, changer.third.y, changer.third.z, f3)
            .await?;
        self.gcode.wait().await?;

        if let Some(cmd) = &changer.post_step_3 {
            if !cmd.is_empty() {
                self.gcode.send(cmd).await?;
            }
        }

        // Move to second (engage slot — slow)
        let f2 = max_feedrate * changer.speed_2_to_3;
        self.move_xyz(changer.second.x, changer.second.y, changer.second.z, f2)
            .await?;
        self.gcode.wait().await?;

        if let Some(cmd) = &changer.post_step_2 {
            if !cmd.is_empty() {
                self.gcode.send(cmd).await?;
            }
        }

        // Move to first (release — very slow)
        let f1 = max_feedrate * changer.speed_1_to_2;
        self.move_xyz(changer.first.x, changer.first.y, changer.first.z, f1)
            .await?;
        self.gcode.wait().await?;

        if let Some(cmd) = &changer.post_step_1 {
            if !cmd.is_empty() {
                self.gcode.send(cmd).await?;
            }
        }

        // Retract to safe Z
        self.move_z(safe_z, max_feedrate).await?;
        self.gcode.wait().await?;

        info!("Unloaded nozzle tip: {}", tip_name);
        Ok(())
    }

    async fn get_changer_config(&self, tip_name: &str) -> Result<ChangerConfig, ChangerError> {
        let config = self.full_config.read().await;
        let tip = config
            .nozzle_tips
            .get(tip_name)
            .ok_or_else(|| ChangerError::TipNotFound(tip_name.to_string()))?;
        tip.changer
            .clone()
            .ok_or_else(|| ChangerError::NoChangerConfig(tip_name.to_string()))
    }

    async fn safe_z(&self) -> f64 {
        let config = self.full_config.read().await;
        config.machine.motion.safe_z
    }

    async fn max_feedrate(&self) -> f64 {
        let config = self.full_config.read().await;
        config.machine.motion.default_feedrate
    }

    async fn move_z(&self, z: f64, feedrate: f64) -> Result<(), GCodeError> {
        let cmd = format!("G1 Z{:.4} F{:.0}", z, feedrate);
        debug!("changer: {}", cmd);
        self.gcode.send(&cmd).await?;
        Ok(())
    }

    async fn move_xy(&self, x: f64, y: f64, feedrate: f64) -> Result<(), GCodeError> {
        let cmd = format!("G1 X{:.4} Y{:.4} F{:.0}", x, y, feedrate);
        debug!("changer: {}", cmd);
        self.gcode.send(&cmd).await?;
        Ok(())
    }

    async fn move_xyz(&self, x: f64, y: f64, z: f64, feedrate: f64) -> Result<(), GCodeError> {
        let cmd = format!("G1 X{:.4} Y{:.4} Z{:.4} F{:.0}", x, y, z, feedrate);
        debug!("changer: {}", cmd);
        self.gcode.send(&cmd).await?;
        Ok(())
    }
}
