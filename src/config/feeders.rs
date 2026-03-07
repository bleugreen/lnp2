use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeederLocation {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub rotation: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayOrigin {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeederConfig {
    Photon(PhotonFeederConfig),
    Tray(TrayFeederConfig),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PhotonFeederConfig {
    pub enabled: bool,
    pub part_id: String,
    pub hardware_id: String,
    pub slot_address: u8,
    pub location: FeederLocation,
    #[serde(default = "default_part_pitch")]
    pub part_pitch: f64,
    #[serde(default = "default_retry_count")]
    pub retry_count: u32,
    #[serde(default = "default_retry_count")]
    pub feed_retry_count: u32,
    #[serde(default)]
    pub pick_retry_count: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrayFeederConfig {
    pub enabled: bool,
    pub part_id: String,
    pub location: FeederLocation,
    pub tray_count_x: u32,
    pub tray_count_y: u32,
    pub offset_x: f64,
    pub offset_y: f64,
    #[serde(default = "default_tray_origin")]
    pub first_row_first_col: TrayOrigin,
    #[serde(default = "default_retry_count")]
    pub retry_count: u32,
    #[serde(default = "default_retry_count")]
    pub feed_retry_count: u32,
    #[serde(default)]
    pub pick_retry_count: u32,
}

fn default_part_pitch() -> f64 {
    2.0
}

fn default_retry_count() -> u32 {
    3
}

fn default_tray_origin() -> TrayOrigin {
    TrayOrigin::TopLeft
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeedersFile {
    #[serde(default)]
    pub feeders: HashMap<String, FeederConfig>,
}
