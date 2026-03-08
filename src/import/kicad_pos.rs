use crate::config::boards::{BoardSide, FiducialConfig, PlacementConfig};

use super::{ImportError, ImportResult};

/// Parse KiCad ASCII .pos format.
///
/// Format:
/// ```text
/// ### Footprint Position - ...
/// ## Unit = mm, Angle = deg.
/// ## Side: top
/// # Ref     Val       Package                    PosX       PosY       Rot    Side
/// C1        1uF       C_0603_1608Metric          47.9000    45.6000    0.0    top
/// ```
pub fn parse_ascii(content: &str) -> Result<ImportResult, ImportError> {
    let mut placements = Vec::new();
    let mut fiducials = Vec::new();
    let mut warnings = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Split on whitespace
        let fields: Vec<&str> = trimmed.split_whitespace().collect();
        if fields.len() < 7 {
            warnings.push(format!("Line {}: expected 7+ fields, got {}", line_num + 1, fields.len()));
            continue;
        }

        let reference = fields[0].to_string();
        let _value = fields[1];
        let package = fields[2];

        let x: f64 = fields[3].parse().map_err(|_| ImportError::Parse {
            line: line_num + 1,
            message: format!("invalid X coordinate: {}", fields[3]),
        })?;
        let y: f64 = fields[4].parse().map_err(|_| ImportError::Parse {
            line: line_num + 1,
            message: format!("invalid Y coordinate: {}", fields[4]),
        })?;
        let rotation: f64 = fields[5].parse().map_err(|_| ImportError::Parse {
            line: line_num + 1,
            message: format!("invalid rotation: {}", fields[5]),
        })?;
        let side = parse_side(fields[6]);

        // Normalize rotation to 0..360
        let rotation = normalize_rotation(rotation);

        // Detect fiducials by package name
        if is_fiducial(package, &reference) {
            fiducials.push(FiducialConfig {
                reference,
                x,
                y,
                diameter_mm: 1.0,
            });
        } else {
            // Use package as part_id placeholder — user maps these to parts.toml entries
            placements.push(PlacementConfig {
                reference,
                part_id: package.to_string(),
                x,
                y,
                rotation,
                side,
                enabled: true,
            });
        }
    }

    if placements.is_empty() && fiducials.is_empty() {
        return Err(ImportError::NoData);
    }

    Ok(ImportResult {
        placements,
        fiducials,
        warnings,
    })
}

/// Parse KiCad CSV .pos format.
///
/// Format:
/// ```text
/// Ref,Val,Package,PosX,PosY,Rot,Side
/// C1,1uF,C_0603_1608Metric,47.9000,45.6000,0.0,top
/// ```
pub fn parse_csv(content: &str) -> Result<ImportResult, ImportError> {
    let mut placements = Vec::new();
    let mut fiducials = Vec::new();
    let mut warnings = Vec::new();

    let mut lines = content.lines().enumerate();

    // Find header row to determine column mapping
    let mut col_ref = 0;
    let mut _col_val = 1;
    let mut col_package = 2;
    let mut col_x = 3;
    let mut col_y = 4;
    let mut col_rot = 5;
    let mut col_side = 6;
    let mut header_found = false;

    for (line_num, line) in &mut lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Parse header
        let headers: Vec<&str> = split_csv_line(trimmed);
        for (i, h) in headers.iter().enumerate() {
            let h_lower = h.trim_matches('"').to_lowercase();
            match h_lower.as_str() {
                "ref" => col_ref = i,
                "val" | "value" => _col_val = i,
                "package" | "footprint" => col_package = i,
                "posx" | "pos x" | "x" => col_x = i,
                "posy" | "pos y" | "y" => col_y = i,
                "rot" | "rotation" => col_rot = i,
                "side" => col_side = i,
                _ => {}
            }
        }
        header_found = true;
        break;
    }

    if !header_found {
        return Err(ImportError::NoData);
    }

    // Parse data rows
    for (line_num, line) in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = split_csv_line(trimmed);
        let max_col = [col_ref, col_package, col_x, col_y, col_rot, col_side]
            .into_iter()
            .max()
            .unwrap();

        if fields.len() <= max_col {
            warnings.push(format!(
                "Line {}: expected {} fields, got {}",
                line_num + 1,
                max_col + 1,
                fields.len()
            ));
            continue;
        }

        let reference = unquote(fields[col_ref]);
        let package = unquote(fields[col_package]);

        let x: f64 = unquote(fields[col_x]).parse().map_err(|_| ImportError::Parse {
            line: line_num + 1,
            message: format!("invalid X coordinate: {}", fields[col_x]),
        })?;
        let y: f64 = unquote(fields[col_y]).parse().map_err(|_| ImportError::Parse {
            line: line_num + 1,
            message: format!("invalid Y coordinate: {}", fields[col_y]),
        })?;
        let rotation: f64 = unquote(fields[col_rot]).parse().map_err(|_| ImportError::Parse {
            line: line_num + 1,
            message: format!("invalid rotation: {}", fields[col_rot]),
        })?;
        let side = parse_side(unquote(fields[col_side]));

        let rotation = normalize_rotation(rotation);

        if is_fiducial(&package, &reference) {
            fiducials.push(FiducialConfig {
                reference: reference.to_string(),
                x,
                y,
                diameter_mm: 1.0,
            });
        } else {
            placements.push(PlacementConfig {
                reference: reference.to_string(),
                part_id: package.to_string(),
                x,
                y,
                rotation,
                side,
                enabled: true,
            });
        }
    }

    if placements.is_empty() && fiducials.is_empty() {
        return Err(ImportError::NoData);
    }

    Ok(ImportResult {
        placements,
        fiducials,
        warnings,
    })
}

fn parse_side(s: &str) -> BoardSide {
    match s.to_lowercase().as_str() {
        "bottom" | "bot" | "b" => BoardSide::Bottom,
        _ => BoardSide::Top,
    }
}

fn normalize_rotation(deg: f64) -> f64 {
    let mut r = deg % 360.0;
    if r < 0.0 {
        r += 360.0;
    }
    r
}

/// Check if a placement is a fiducial by package name or reference designator.
fn is_fiducial(package: &str, reference: &str) -> bool {
    let pkg_lower = package.to_lowercase();
    let ref_lower = reference.to_lowercase();

    pkg_lower.contains("fiducial")
        || pkg_lower.contains("fid_")
        || pkg_lower == "fid"
        || ref_lower.starts_with("fid")
}

/// Split a CSV line, handling quoted fields.
fn split_csv_line(line: &str) -> Vec<&str> {
    // Simple split — doesn't handle commas inside quotes, but KiCad doesn't produce those
    line.split(',').collect()
}

/// Remove surrounding quotes from a field.
fn unquote(s: &str) -> &str {
    s.trim().trim_matches('"')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ascii_basic() {
        let content = r#"### Footprint Position - Component Side - created on 2024-01-01
## Unit = mm, Angle = deg.
## Side : top
# Ref     Val         Package                PosX       PosY       Rot    Side
C1        1uF         C_0603_1608Metric      47.9000    45.6000    0.0    top
R1        1K          R_0805_2012Metric      50.1000    30.2000    90.0   top
U1        STM32       QFP48                  60.0000    40.0000    270.0  top
"#;
        let result = parse_ascii(content).unwrap();
        assert_eq!(result.placements.len(), 3);
        assert_eq!(result.fiducials.len(), 0);
        assert_eq!(result.placements[0].reference, "C1");
        assert!((result.placements[0].x - 47.9).abs() < 0.001);
        assert!((result.placements[1].rotation - 90.0).abs() < 0.001);
        assert!((result.placements[2].rotation - 270.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_ascii_with_fiducials() {
        let content = r#"### Footprint Position
## Unit = mm, Angle = deg.
# Ref     Val         Package                PosX       PosY       Rot    Side
FID1      FID         Fiducial_1mm           5.0000     5.0000     0.0    top
C1        1uF         C_0603_1608Metric      47.9000    45.6000    0.0    top
FID2      FID         Fiducial_1mm           95.0000    55.0000    0.0    top
"#;
        let result = parse_ascii(content).unwrap();
        assert_eq!(result.placements.len(), 1);
        assert_eq!(result.fiducials.len(), 2);
        assert_eq!(result.fiducials[0].reference, "FID1");
        assert!((result.fiducials[0].x - 5.0).abs() < 0.001);
        assert_eq!(result.fiducials[1].reference, "FID2");
    }

    #[test]
    fn test_parse_ascii_negative_rotation() {
        let content = r#"# Ref Val Package PosX PosY Rot Side
R1 1K R_0805 50.0 30.0 -90.0 top
"#;
        let result = parse_ascii(content).unwrap();
        assert!((result.placements[0].rotation - 270.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_csv_basic() {
        let content = r#"Ref,Val,Package,PosX,PosY,Rot,Side
C1,1uF,C_0603_1608Metric,47.9000,45.6000,0.0,top
R1,1K,R_0805_2012Metric,50.1000,30.2000,90.0,top
"#;
        let result = parse_csv(content).unwrap();
        assert_eq!(result.placements.len(), 2);
        assert_eq!(result.placements[0].reference, "C1");
        assert!((result.placements[0].x - 47.9).abs() < 0.001);
        assert!((result.placements[1].rotation - 90.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_csv_quoted() {
        let content = r#""Ref","Val","Package","PosX","PosY","Rot","Side"
"C1","1uF","C_0603_1608Metric","47.9000","45.6000","0.0","top"
"#;
        let result = parse_csv(content).unwrap();
        assert_eq!(result.placements.len(), 1);
        assert_eq!(result.placements[0].reference, "C1");
    }

    #[test]
    fn test_parse_csv_with_fiducials() {
        let content = r#"Ref,Val,Package,PosX,PosY,Rot,Side
FID1,FID,Fiducial_1mm,5.0,5.0,0.0,top
C1,1uF,C_0603,47.9,45.6,0.0,top
FID2,FID,Fiducial_1mm,95.0,55.0,0.0,top
"#;
        let result = parse_csv(content).unwrap();
        assert_eq!(result.placements.len(), 1);
        assert_eq!(result.fiducials.len(), 2);
    }

    #[test]
    fn test_parse_csv_bottom_side() {
        let content = r#"Ref,Val,Package,PosX,PosY,Rot,Side
C1,1uF,C_0603,47.9,45.6,0.0,bottom
"#;
        let result = parse_csv(content).unwrap();
        assert_eq!(result.placements[0].side, BoardSide::Bottom);
    }

    #[test]
    fn test_is_fiducial() {
        assert!(is_fiducial("Fiducial_1mm", "FID1"));
        assert!(is_fiducial("SomePackage", "FID2"));
        assert!(is_fiducial("fid_1mm_Cu", "J1"));
        assert!(!is_fiducial("C_0603", "C1"));
        assert!(!is_fiducial("R_0805", "R1"));
    }

    #[test]
    fn test_normalize_rotation() {
        assert!((normalize_rotation(0.0) - 0.0).abs() < 0.001);
        assert!((normalize_rotation(90.0) - 90.0).abs() < 0.001);
        assert!((normalize_rotation(-90.0) - 270.0).abs() < 0.001);
        assert!((normalize_rotation(360.0) - 0.0).abs() < 0.001);
        assert!((normalize_rotation(450.0) - 90.0).abs() < 0.001);
    }

    #[test]
    fn test_empty_content() {
        let result = parse_ascii("");
        assert!(result.is_err());
    }
}
