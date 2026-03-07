use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::feeders::FeederLocation;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VacuumThresholds {
    pub part_on_low: f64,
    pub part_on_high: f64,
    pub part_off_low: f64,
    pub part_off_high: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChangerConfig {
    pub first: FeederLocation,
    pub second: FeederLocation,
    pub third: FeederLocation,
    pub last: FeederLocation,
    #[serde(default = "default_speed_fraction")]
    pub speed_1_to_2: f64,
    #[serde(default = "default_speed_fraction")]
    pub speed_2_to_3: f64,
    #[serde(default = "default_speed_fraction")]
    pub speed_3_to_4: f64,
    pub post_step_1: Option<String>,
    pub post_step_2: Option<String>,
    pub post_step_3: Option<String>,
}

fn default_speed_fraction() -> f64 {
    1.0
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NozzleTipConfig {
    pub name: String,
    #[serde(default = "default_dwell")]
    pub pick_dwell_ms: u32,
    #[serde(default = "default_dwell")]
    pub place_dwell_ms: u32,
    #[serde(default)]
    pub min_part_diameter: f64,
    #[serde(default = "default_max_part")]
    pub max_part_diameter: f64,
    #[serde(default = "default_max_part")]
    pub max_part_height: f64,
    pub vacuum: Option<VacuumThresholds>,
    pub changer: Option<ChangerConfig>,
}

fn default_dwell() -> u32 {
    200
}

fn default_max_part() -> f64 {
    10.0
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NozzleTipsFile {
    #[serde(default)]
    pub tips: HashMap<String, NozzleTipConfig>,
}
