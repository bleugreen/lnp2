use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MachineConfig {
    pub serial: SerialConfig,
    pub motion: MotionConfig,
    pub axes: HashMap<String, AxisConfig>,
    pub nozzles: HashMap<String, NozzleConfig>,
    #[serde(default)]
    pub cameras: HashMap<String, CameraConfig>,
    pub leds: LedConfig,
    pub connect: ConnectConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SerialConfig {
    pub port: String,
    pub baud: u32,
    pub timeout_ms: u64,
    pub motion_timeout_ms: u64,
    pub home_timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MotionConfig {
    pub safe_z: f64,
    pub default_feedrate: f64,
    pub default_acceleration: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AxisConfig {
    pub letter: String,
    pub min: f64,
    pub max: f64,
    pub home: f64,
    pub feedrate: f64,
    pub acceleration: f64,
    pub safe_zone_low: Option<f64>,
    pub safe_zone_high: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Offset2D {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NozzleConfig {
    pub head_offset: Offset2D,
    pub vacuum_on: Vec<String>,
    pub vacuum_off: Vec<String>,
    pub blow_on: String,
    pub blow_off: String,
    pub sensor_mux: String,
    pub sensor_read: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CameraConfig {
    pub device: String,
    pub width: u32,
    pub height: u32,
    pub upp_x: f64,
    pub upp_y: f64,
    pub default_z: f64,
    #[serde(default)]
    pub flip_x: bool,
    #[serde(default)]
    pub flip_y: bool,
    pub location: Option<Location3D>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Location3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LedConfig {
    pub on: String,
    pub off: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectConfig {
    pub init_commands: Vec<String>,
}

pub fn load_config(path: &Path) -> Result<MachineConfig, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let config: MachineConfig = toml::from_str(&content)?;
    Ok(config)
}
