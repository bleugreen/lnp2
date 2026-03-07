use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::debug;

use crate::config::MachineConfig;
use crate::gcode::{GCodeDriver, GCodeError, Position};

#[derive(Debug, thiserror::Error)]
pub enum MotionError {
    #[error("GCode error: {0}")]
    GCode(#[from] GCodeError),
    #[error("Soft limit: axis {axis} value {value} outside [{min}, {max}]")]
    SoftLimit {
        axis: String,
        value: f64,
        min: f64,
        max: f64,
    },
    #[error("Unknown axis: {0}")]
    UnknownAxis(String),
}

pub struct MotionController {
    gcode: Arc<GCodeDriver>,
    config: Arc<RwLock<MachineConfig>>,
}

impl MotionController {
    pub fn new(gcode: Arc<GCodeDriver>, config: Arc<RwLock<MachineConfig>>) -> Self {
        Self { gcode, config }
    }

    /// Move to specified coordinates. Only axes with Some values are included.
    pub async fn move_to(
        &self,
        x: Option<f64>,
        y: Option<f64>,
        z: Option<f64>,
        a: Option<f64>,
        b: Option<f64>,
        feedrate: Option<f64>,
    ) -> Result<(), MotionError> {
        let config = self.config.read().await;

        // Validate soft limits
        let checks: &[(&str, Option<f64>)] = &[
            ("x", x), ("y", y), ("z", z), ("a", a), ("b", b),
        ];
        for &(name, value) in checks {
            if let Some(v) = value {
                if let Some(axis) = config.axes.get(name) {
                    if v < axis.min || v > axis.max {
                        return Err(MotionError::SoftLimit {
                            axis: name.to_string(),
                            value: v,
                            min: axis.min,
                            max: axis.max,
                        });
                    }
                }
            }
        }

        // Build G1 command
        let mut cmd = String::from("G1");
        if let Some(v) = x { cmd.push_str(&format!(" X{:.4}", v)); }
        if let Some(v) = y { cmd.push_str(&format!(" Y{:.4}", v)); }
        if let Some(v) = z { cmd.push_str(&format!(" Z{:.4}", v)); }
        if let Some(v) = a { cmd.push_str(&format!(" A{:.4}", v)); }
        if let Some(v) = b { cmd.push_str(&format!(" B{:.4}", v)); }

        let f = feedrate.unwrap_or(config.motion.default_feedrate);
        cmd.push_str(&format!(" F{:.0}", f));
        drop(config);

        debug!("move_to: {}", cmd);
        self.gcode.send(&cmd).await?;
        Ok(())
    }

    /// Safe XY move: retract Z to safe zone first, then move XY.
    pub async fn move_safe(&self, x: f64, y: f64) -> Result<(), MotionError> {
        let config = self.config.read().await;
        let safe_z = config.motion.safe_z;
        let feedrate = config.motion.default_feedrate;
        drop(config);

        // Retract Z to safe position
        self.gcode
            .send(&format!("G1 Z{:.4} F{:.0}", safe_z, feedrate))
            .await?;
        self.gcode.wait().await?;

        // Move XY
        self.move_to(Some(x), Some(y), None, None, None, None).await?;
        self.gcode.wait().await?;
        Ok(())
    }

    /// Safe move with Z target: safe XY move, then lower Z.
    pub async fn move_safe_z(&self, x: f64, y: f64, z: f64) -> Result<(), MotionError> {
        self.move_safe(x, y).await?;
        self.move_to(None, None, Some(z), None, None, None).await?;
        self.gcode.wait().await?;
        Ok(())
    }

    /// Home all axes.
    pub async fn home(&self) -> Result<(), MotionError> {
        let config = self.config.read().await;
        let accel = config.motion.default_acceleration;
        drop(config);

        self.gcode
            .send(&format!("M204 S{:.0}", accel))
            .await?;
        self.gcode.home().await?;
        Ok(())
    }

    /// Get current position from Marlin.
    pub async fn get_position(&self) -> Result<Position, MotionError> {
        Ok(self.gcode.position().await?)
    }

    /// Set acceleration.
    pub async fn set_acceleration(&self, mm_s2: f64) -> Result<(), MotionError> {
        self.gcode
            .send(&format!("M204 S{:.0}", mm_s2))
            .await?;
        Ok(())
    }

    /// Determine which nozzle is active based on Z position.
    /// Z < safe_zone_low → N1 is down (Z1 low)
    /// Z > safe_zone_high → N2 is down (Z2 = 63 - Z1, so Z1 high = N2 down)
    /// In safe zone → neither engaged
    pub async fn active_nozzle(&self) -> Option<NozzleId> {
        let pos = self.gcode.cached_position().await;
        let config = self.config.read().await;
        let z_config = config.axes.get("z")?;
        let low = z_config.safe_zone_low.unwrap_or(26.5);
        let high = z_config.safe_zone_high.unwrap_or(36.5);

        if pos.z < low {
            Some(NozzleId::N1)
        } else if pos.z > high {
            Some(NozzleId::N2)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NozzleId {
    N1,
    N2,
}

impl NozzleId {
    pub fn config_key(&self) -> &str {
        match self {
            NozzleId::N1 => "n1",
            NozzleId::N2 => "n2",
        }
    }
}

impl std::fmt::Display for NozzleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NozzleId::N1 => write!(f, "N1"),
            NozzleId::N2 => write!(f, "N2"),
        }
    }
}
