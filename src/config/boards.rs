use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BoardConfig {
    pub name: String,
    #[serde(default)]
    pub source_file: Option<String>,
    #[serde(default)]
    pub fiducials: Vec<FiducialConfig>,
    #[serde(default)]
    pub placements: Vec<PlacementConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FiducialConfig {
    pub reference: String,
    pub x: f64,
    pub y: f64,
    #[serde(default = "default_fiducial_diameter")]
    pub diameter_mm: f64,
}

fn default_fiducial_diameter() -> f64 {
    1.0
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlacementConfig {
    pub reference: String,
    pub part_id: String,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub side: BoardSide,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BoardSide {
    Top,
    Bottom,
}

impl Default for BoardSide {
    fn default() -> Self {
        BoardSide::Top
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BoardsFile {
    #[serde(default)]
    pub boards: HashMap<String, BoardConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_board_config_roundtrip() {
        let toml = r#"
name = "test_board"
source_file = "test.pos"

[[fiducials]]
reference = "FID1"
x = 5.0
y = 5.0
diameter_mm = 1.0

[[fiducials]]
reference = "FID2"
x = 95.0
y = 55.0
diameter_mm = 1.0

[[placements]]
reference = "R1"
part_id = "R0805_1K"
x = 47.9
y = 45.6
rotation = 0.0
side = "top"
enabled = true

[[placements]]
reference = "C3"
part_id = "C_0402_100nF"
x = 50.1
y = 30.2
rotation = 90.0
side = "top"
enabled = true
"#;
        let board: BoardConfig = toml::from_str(toml).unwrap();
        assert_eq!(board.name, "test_board");
        assert_eq!(board.fiducials.len(), 2);
        assert_eq!(board.placements.len(), 2);
        assert_eq!(board.placements[0].reference, "R1");
        assert!((board.placements[0].x - 47.9).abs() < 0.001);
    }
}
