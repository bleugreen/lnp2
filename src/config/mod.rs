pub mod feeders;
pub mod nozzle_tips;
pub mod parts;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tracing::{info, warn};

pub use feeders::{FeederConfig, FeedersFile};
pub use nozzle_tips::{NozzleTipConfig, NozzleTipsFile};
pub use parts::{PackageConfig, PackagesFile, PadConfig, PartConfig, PartsFile};

// --- Machine config (existing Phase 1 types) ---

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
    #[serde(default)]
    pub vision: Option<crate::vision::types::VisionConfig>,
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

// --- Full config (all config files combined) ---

#[derive(Debug, Clone)]
pub struct FullConfig {
    pub machine: MachineConfig,
    pub feeders: HashMap<String, FeederConfig>,
    pub parts: HashMap<String, PartConfig>,
    pub packages: HashMap<String, PackageConfig>,
    pub nozzle_tips: HashMap<String, NozzleTipConfig>,
}

pub fn load_config(path: &Path) -> Result<MachineConfig, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let config: MachineConfig = toml::from_str(&content)?;
    Ok(config)
}

/// Load all config files from the config directory.
/// machine.toml is required; feeders/parts/packages/nozzle_tips are optional.
pub fn load_full_config(config_dir: &Path) -> Result<FullConfig, Box<dyn std::error::Error>> {
    let machine = load_config(&config_dir.join("machine.toml"))?;

    let feeders = load_optional::<FeedersFile>(&config_dir.join("feeders.toml"))?
        .map(|f| f.feeders)
        .unwrap_or_default();

    let parts = load_optional::<PartsFile>(&config_dir.join("parts.toml"))?
        .map(|f| f.parts)
        .unwrap_or_default();

    let packages = load_optional::<PackagesFile>(&config_dir.join("packages.toml"))?
        .map(|f| f.packages)
        .unwrap_or_default();

    let nozzle_tips = load_optional::<NozzleTipsFile>(&config_dir.join("nozzle_tips.toml"))?
        .map(|f| f.tips)
        .unwrap_or_default();

    info!(
        "Loaded config: {} feeders, {} parts, {} packages, {} nozzle tips",
        feeders.len(),
        parts.len(),
        packages.len(),
        nozzle_tips.len()
    );

    Ok(FullConfig {
        machine,
        feeders,
        parts,
        packages,
        nozzle_tips,
    })
}

fn load_optional<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<Option<T>, Box<dyn std::error::Error>> {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let parsed: T = toml::from_str(&content)?;
            Ok(Some(parsed))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!("Optional config not found: {}", path.display());
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feeder_config_photon() {
        let toml = r#"
[feeders.photon_1]
type = "photon"
enabled = true
part_id = "C_0402_100nF"
hardware_id = "0007800B4248571720343331"
slot_address = 34
location = { x = 100.0, y = 200.0, z = 5.0, rotation = 0.0 }
part_pitch = 2.0
retry_count = 3
feed_retry_count = 3
pick_retry_count = 0
"#;
        let file: FeedersFile = toml::from_str(toml).unwrap();
        assert!(file.feeders.contains_key("photon_1"));
        match &file.feeders["photon_1"] {
            FeederConfig::Photon(p) => {
                assert_eq!(p.slot_address, 34);
                assert_eq!(p.hardware_id, "0007800B4248571720343331");
                assert!((p.location.x - 100.0).abs() < 0.001);
            }
            _ => panic!("Expected Photon feeder"),
        }
    }

    #[test]
    fn test_feeder_config_tray() {
        let toml = r#"
[feeders.tray_1]
type = "tray"
enabled = true
part_id = "QFP48_MCU"
location = { x = 300.0, y = 150.0, z = 3.0, rotation = 0.0 }
tray_count_x = 4
tray_count_y = 3
offset_x = 10.0
offset_y = 10.0
first_row_first_col = "top_left"
retry_count = 3
feed_retry_count = 3
pick_retry_count = 0
"#;
        let file: FeedersFile = toml::from_str(toml).unwrap();
        match &file.feeders["tray_1"] {
            FeederConfig::Tray(t) => {
                assert_eq!(t.tray_count_x, 4);
                assert_eq!(t.tray_count_y, 3);
            }
            _ => panic!("Expected Tray feeder"),
        }
    }

    #[test]
    fn test_part_config() {
        let toml = r#"
[parts.C_0402_100nF]
package_id = "C_0402_1005Metric"
height = 0.5
speed = 1.0
"#;
        let file: PartsFile = toml::from_str(toml).unwrap();
        let part = &file.parts["C_0402_100nF"];
        assert_eq!(part.package_id, "C_0402_1005Metric");
        assert!((part.height - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_package_config() {
        let toml = r#"
[packages.R0805]
body_width = 2.0
body_height = 1.25
compatible_nozzle_tips = ["N045"]

[[packages.R0805.pads]]
name = "1"
x = -0.825
y = 0.0
width = 0.35
height = 1.25
rotation = 0.0
roundness = 0.0

[[packages.R0805.pads]]
name = "2"
x = 0.825
y = 0.0
width = 0.35
height = 1.25
rotation = 0.0
roundness = 0.0
"#;
        let file: PackagesFile = toml::from_str(toml).unwrap();
        let pkg = &file.packages["R0805"];
        assert_eq!(pkg.pads.len(), 2);
        assert!((pkg.body_width - 2.0).abs() < 0.001);
        assert_eq!(pkg.compatible_nozzle_tips, vec!["N045"]);
    }

    #[test]
    fn test_nozzle_tip_config() {
        let toml = r#"
[tips.N045]
name = "N045"
pick_dwell_ms = 200
place_dwell_ms = 100
min_part_diameter = 0.0
max_part_diameter = 5.0
max_part_height = 5.0

[tips.N045.vacuum]
part_on_low = 100.0
part_on_high = 300.0
part_off_low = 0.0
part_off_high = 50.0

[tips.N045.changer]
first  = { x = 370.452, y = 125.573, z = 31.0, rotation = 0.0 }
second = { x = 370.452, y = 125.573, z = 0.9, rotation = 0.0 }
third  = { x = 370.452, y = 125.573, z = 4.1, rotation = 0.0 }
last   = { x = 355.364, y = 125.573, z = 4.3, rotation = 0.0 }
speed_1_to_2 = 0.05
speed_2_to_3 = 0.20
speed_3_to_4 = 0.50
"#;
        let file: NozzleTipsFile = toml::from_str(toml).unwrap();
        let tip = &file.tips["N045"];
        assert_eq!(tip.name, "N045");
        assert_eq!(tip.pick_dwell_ms, 200);
        let changer = tip.changer.as_ref().unwrap();
        assert!((changer.speed_1_to_2 - 0.05).abs() < 0.001);
        assert!((changer.last.x - 355.364).abs() < 0.001);
        let vacuum = tip.vacuum.as_ref().unwrap();
        assert!((vacuum.part_on_low - 100.0).abs() < 0.001);
    }
}
