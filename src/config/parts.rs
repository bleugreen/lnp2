use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PartConfig {
    pub package_id: String,
    #[serde(default)]
    pub height: f64,
    #[serde(default = "default_speed")]
    pub speed: f64,
    #[serde(default)]
    pub pick_retry_count: u32,
}

fn default_speed() -> f64 {
    1.0
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PadConfig {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub roundness: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageConfig {
    #[serde(default)]
    pub body_width: f64,
    #[serde(default)]
    pub body_height: f64,
    #[serde(default)]
    pub compatible_nozzle_tips: Vec<String>,
    #[serde(default)]
    pub pads: Vec<PadConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PartsFile {
    #[serde(default)]
    pub parts: HashMap<String, PartConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackagesFile {
    #[serde(default)]
    pub packages: HashMap<String, PackageConfig>,
}
