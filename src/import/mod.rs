pub mod kicad_pos;
pub mod openpnp;

pub use openpnp::import_openpnp;

use std::path::Path;

use crate::config::boards::{BoardConfig, BoardSide, FiducialConfig, PlacementConfig};

/// Result from importing a placement file.
#[derive(Debug)]
pub struct ImportResult {
    pub placements: Vec<PlacementConfig>,
    pub fiducials: Vec<FiducialConfig>,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unknown file format")]
    UnknownFormat,
    #[error("Parse error on line {line}: {message}")]
    Parse { line: usize, message: String },
    #[error("No data rows found")]
    NoData,
}

#[derive(Debug, PartialEq)]
enum Format {
    KiCadAscii,
    KiCadCsv,
    Unknown,
}

/// Detect the format of a placement file from its first lines.
fn detect_format(content: &str) -> Format {
    let lines: Vec<&str> = content.lines().take(10).collect();

    for line in &lines {
        let trimmed = line.trim();
        // KiCad ASCII: starts with "### " or "## " header comments
        if trimmed.starts_with("### Footprint Position") || trimmed.starts_with("## Unit") {
            return Format::KiCadAscii;
        }
        // KiCad CSV: header row with known column names
        if trimmed.starts_with("Ref,") || trimmed.starts_with("\"Ref\",") {
            return Format::KiCadCsv;
        }
    }

    // Fallback: check if first non-comment line is comma-separated with expected columns
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if trimmed.contains(',') && trimmed.split(',').count() >= 6 {
            return Format::KiCadCsv;
        }
    }

    Format::Unknown
}

/// Import a placement file (KiCad .pos format, ASCII or CSV).
/// Returns placements and fiducials as separate lists.
pub fn import_placement_file(path: &Path) -> Result<ImportResult, ImportError> {
    let content = std::fs::read_to_string(path)?;
    match detect_format(&content) {
        Format::KiCadAscii => kicad_pos::parse_ascii(&content),
        Format::KiCadCsv => kicad_pos::parse_csv(&content),
        Format::Unknown => Err(ImportError::UnknownFormat),
    }
}

/// Import a placement file and create a BoardConfig.
pub fn import_board(path: &Path, board_name: &str) -> Result<BoardConfig, ImportError> {
    let result = import_placement_file(path)?;

    Ok(BoardConfig {
        name: board_name.to_string(),
        source_file: Some(path.to_string_lossy().to_string()),
        fiducials: result.fiducials,
        placements: result.placements,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_kicad_ascii() {
        let content = "### Footprint Position - created on 2024-01-01\n## Unit = mm, Angle = deg.\n## Side: top\n# Ref  Val  Package  PosX  PosY  Rot  Side\n";
        assert_eq!(detect_format(content), Format::KiCadAscii);
    }

    #[test]
    fn test_detect_kicad_csv() {
        let content = "Ref,Val,Package,PosX,PosY,Rot,Side\nC1,1uF,C_0603,47.9,45.6,0.0,top\n";
        assert_eq!(detect_format(content), Format::KiCadCsv);
    }

    #[test]
    fn test_detect_unknown() {
        let content = "random garbage\n";
        assert_eq!(detect_format(content), Format::Unknown);
    }
}
