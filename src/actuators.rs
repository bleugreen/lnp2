use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::debug;

use crate::config::MachineConfig;
use crate::gcode::{parser, GCodeDriver, GCodeError};
use crate::motion::NozzleId;

#[derive(Debug, thiserror::Error)]
pub enum ActuatorError {
    #[error("GCode error: {0}")]
    GCode(#[from] GCodeError),
    #[error("Unknown nozzle: {0}")]
    UnknownNozzle(String),
    #[error("Vacuum read failed: could not parse sensor value")]
    VacuumParseFailed,
}

pub struct ActuatorController {
    gcode: Arc<GCodeDriver>,
    config: Arc<RwLock<MachineConfig>>,
}

impl ActuatorController {
    pub fn new(gcode: Arc<GCodeDriver>, config: Arc<RwLock<MachineConfig>>) -> Self {
        Self { gcode, config }
    }

    /// Turn vacuum on for the specified nozzle.
    pub async fn vacuum_on(&self, nozzle: NozzleId) -> Result<(), ActuatorError> {
        let config = self.config.read().await;
        let nozzle_config = config
            .nozzles
            .get(nozzle.config_key())
            .ok_or_else(|| ActuatorError::UnknownNozzle(nozzle.to_string()))?;

        debug!("vacuum_on: {}", nozzle);
        for cmd in &nozzle_config.vacuum_on {
            self.gcode.send(cmd).await?;
        }
        Ok(())
    }

    /// Turn vacuum off for the specified nozzle.
    pub async fn vacuum_off(&self, nozzle: NozzleId) -> Result<(), ActuatorError> {
        let config = self.config.read().await;
        let nozzle_config = config
            .nozzles
            .get(nozzle.config_key())
            .ok_or_else(|| ActuatorError::UnknownNozzle(nozzle.to_string()))?;

        debug!("vacuum_off: {}", nozzle);
        for cmd in &nozzle_config.vacuum_off {
            self.gcode.send(cmd).await?;
        }
        Ok(())
    }

    /// Pulse the blow-off solenoid for the specified duration.
    pub async fn blow(&self, nozzle: NozzleId, duration_ms: u32) -> Result<(), ActuatorError> {
        let config = self.config.read().await;
        let nozzle_config = config
            .nozzles
            .get(nozzle.config_key())
            .ok_or_else(|| ActuatorError::UnknownNozzle(nozzle.to_string()))?;

        let blow_on = nozzle_config.blow_on.clone();
        let blow_off = nozzle_config.blow_off.clone();
        drop(config);

        debug!("blow: {} for {}ms", nozzle, duration_ms);
        self.gcode.send(&blow_on).await?;
        self.gcode
            .send(&format!("G4 P{}", duration_ms))
            .await?;
        self.gcode.send(&blow_off).await?;
        Ok(())
    }

    /// Read vacuum sensor value for the specified nozzle.
    /// Sends I2C mux select → register select → read sequence.
    pub async fn vacuum_read(&self, nozzle: NozzleId) -> Result<f64, ActuatorError> {
        let config = self.config.read().await;
        let nozzle_config = config
            .nozzles
            .get(nozzle.config_key())
            .ok_or_else(|| ActuatorError::UnknownNozzle(nozzle.to_string()))?;

        let mux_cmd = nozzle_config.sensor_mux.clone();
        let read_cmd = nozzle_config.sensor_read.clone();
        drop(config);

        debug!("vacuum_read: {}", nozzle);
        // Select I2C mux channel
        self.gcode.send(&mux_cmd).await?;
        // Select register (read mode)
        self.gcode.send("M260 A109 B6 S1").await?;
        // Read sensor
        let response = self.gcode.send(&read_cmd).await?;

        parser::parse_vacuum(&response).ok_or(ActuatorError::VacuumParseFailed)
    }

    /// Turn LEDs on with specified color and brightness.
    pub async fn led_on(&self, r: u8, g: u8, b: u8, brightness: u8) -> Result<(), ActuatorError> {
        let cmd = format!("M150 P{} R{} U{} B{}", brightness, r, g, b);
        debug!("led_on: {}", cmd);
        self.gcode.send(&cmd).await?;
        Ok(())
    }

    /// Turn LEDs off.
    pub async fn led_off(&self) -> Result<(), ActuatorError> {
        debug!("led_off");
        self.gcode.send("M150 P0").await?;
        Ok(())
    }
}
