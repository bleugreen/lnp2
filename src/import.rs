use std::collections::HashMap;
use std::path::Path;

use quick_xml::de::from_str;
use serde::Deserialize;
use tracing::info;

use crate::config::feeders::{
    FeederConfig, FeederLocation, FeedersFile, PhotonFeederConfig,
};
use crate::config::nozzle_tips::{ChangerConfig, NozzleTipConfig, NozzleTipsFile};
use crate::config::parts::{PackageConfig, PackagesFile, PadConfig, PartConfig, PartsFile};

// ── OpenPnP XML types ──────────────────────────────────────────────

// machine.xml nozzle tips
#[derive(Debug, Deserialize)]
#[serde(rename = "nozzle-tip")]
struct XmlNozzleTip {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@pick-dwell-milliseconds", default)]
    pick_dwell_ms: Option<u32>,
    #[serde(rename = "@place-dwell-milliseconds", default)]
    place_dwell_ms: Option<u32>,
    #[serde(rename = "changer-start-location")]
    changer_start: Option<XmlLocation>,
    #[serde(rename = "changer-start-to-mid-speed", default)]
    speed_start_to_mid: Option<f64>,
    #[serde(rename = "changer-mid-location")]
    changer_mid: Option<XmlLocation>,
    #[serde(rename = "changer-mid-to-mid-2-speed", default)]
    speed_mid_to_mid2: Option<f64>,
    #[serde(rename = "changer-mid-location-2")]
    changer_mid2: Option<XmlLocation>,
    #[serde(rename = "changer-mid-2-to-end-speed", default)]
    speed_mid2_to_end: Option<f64>,
    #[serde(rename = "changer-end-location")]
    changer_end: Option<XmlLocation>,
}

#[derive(Debug, Deserialize)]
struct XmlLocation {
    #[serde(rename = "@x", default)]
    x: f64,
    #[serde(rename = "@y", default)]
    y: f64,
    #[serde(rename = "@z", default)]
    z: f64,
    #[serde(rename = "@rotation", default)]
    rotation: f64,
}

impl XmlLocation {
    fn to_feeder_location(&self) -> FeederLocation {
        FeederLocation {
            x: self.x,
            y: self.y,
            z: self.z,
            rotation: self.rotation,
        }
    }

    fn is_zero(&self) -> bool {
        self.x == 0.0 && self.y == 0.0 && self.z == 0.0
    }
}

// machine.xml feeder slots
#[derive(Debug, Deserialize)]
struct XmlSlot {
    #[serde(rename = "@address")]
    address: u8,
    location: Option<XmlLocation>,
}

// parts.xml
#[derive(Debug, Deserialize)]
struct XmlPart {
    #[serde(rename = "@id")]
    id: String,
    #[serde(rename = "@package-id", default)]
    package_id: Option<String>,
    #[serde(rename = "@height", default)]
    height: f64,
    #[serde(rename = "@speed", default = "default_speed")]
    speed: f64,
    #[serde(rename = "@pick-retry-count", default)]
    pick_retry_count: u32,
}

fn default_speed() -> f64 {
    1.0
}

#[derive(Debug, Deserialize)]
#[serde(rename = "openpnp-parts")]
struct XmlParts {
    #[serde(rename = "part", default)]
    parts: Vec<XmlPart>,
}

// packages.xml
#[derive(Debug, Deserialize)]
struct XmlPackage {
    #[serde(rename = "@id")]
    id: String,
    footprint: Option<XmlFootprint>,
    #[serde(rename = "compatible-nozzle-tip-ids")]
    compatible_tips: Option<XmlCompatibleTips>,
}

#[derive(Debug, Deserialize)]
struct XmlFootprint {
    #[serde(rename = "@body-width", default)]
    body_width: f64,
    #[serde(rename = "@body-height", default)]
    body_height: f64,
    #[serde(rename = "pad", default)]
    pads: Vec<XmlPad>,
}

#[derive(Debug, Deserialize)]
struct XmlPad {
    #[serde(rename = "@name", default)]
    name: String,
    #[serde(rename = "@x", default)]
    x: f64,
    #[serde(rename = "@y", default)]
    y: f64,
    #[serde(rename = "@width", default)]
    width: f64,
    #[serde(rename = "@height", default)]
    height: f64,
    #[serde(rename = "@rotation", default)]
    rotation: f64,
    #[serde(rename = "@roundness", default)]
    roundness: f64,
}

#[derive(Debug, Deserialize)]
struct XmlCompatibleTips {
    #[serde(rename = "string", default)]
    ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename = "openpnp-packages")]
struct XmlPackages {
    #[serde(rename = "package", default)]
    packages: Vec<XmlPackage>,
}

// ── Import logic ───────────────────────────────────────────────────

pub fn import_openpnp(
    openpnp_dir: &Path,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(output_dir)?;

    // Parse nozzle tips + feeder slots from machine.xml
    let machine_path = openpnp_dir.join("machine.xml");
    let machine_xml = std::fs::read_to_string(&machine_path)?;

    let nozzle_tips = extract_nozzle_tips(&machine_xml)?;
    let slots = extract_feeder_slots(&machine_xml)?;
    let feeders = build_feeders_from_slots(&slots);

    // Parse parts.xml
    let parts = if let Ok(xml) = std::fs::read_to_string(openpnp_dir.join("parts.xml")) {
        extract_parts(&xml)?
    } else {
        HashMap::new()
    };

    // Parse packages.xml
    let packages = if let Ok(xml) = std::fs::read_to_string(openpnp_dir.join("packages.xml")) {
        extract_packages(&xml)?
    } else {
        HashMap::new()
    };

    // Write TOML files
    let tips_file = NozzleTipsFile { tips: nozzle_tips.clone() };
    write_toml(output_dir, "nozzle_tips.toml", &tips_file)?;

    let feeders_file = FeedersFile { feeders };
    write_toml(output_dir, "feeders.toml", &feeders_file)?;

    let parts_file = PartsFile { parts: parts.clone() };
    write_toml(output_dir, "parts.toml", &parts_file)?;

    let packages_file = PackagesFile { packages: packages.clone() };
    write_toml(output_dir, "packages.toml", &packages_file)?;

    info!(
        "Imported: {} nozzle tips, {} feeder slots, {} parts, {} packages",
        nozzle_tips.len(),
        feeders_file.feeders.len(),
        parts.len(),
        packages.len()
    );

    Ok(())
}

fn write_toml<T: serde::Serialize>(
    dir: &Path,
    filename: &str,
    data: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = toml::to_string_pretty(data)?;
    let path = dir.join(filename);
    std::fs::write(&path, content)?;
    info!("Wrote {}", path.display());
    Ok(())
}

// ── Extraction helpers ─────────────────────────────────────────────

fn extract_nozzle_tips(xml: &str) -> Result<HashMap<String, NozzleTipConfig>, Box<dyn std::error::Error>> {
    let mut tips = HashMap::new();

    // Extract each <nozzle-tip> block individually since machine.xml is complex
    for chunk in split_xml_elements(xml, "nozzle-tip") {
        let nt: XmlNozzleTip = match from_str(&chunk) {
            Ok(nt) => nt,
            Err(_) => continue,
        };

        let changer = match (&nt.changer_start, &nt.changer_mid, &nt.changer_mid2, &nt.changer_end) {
            (Some(start), Some(mid), Some(mid2), Some(end)) if !start.is_zero() => {
                Some(ChangerConfig {
                    first: start.to_feeder_location(),
                    second: mid.to_feeder_location(),
                    third: mid2.to_feeder_location(),
                    last: end.to_feeder_location(),
                    speed_1_to_2: nt.speed_start_to_mid.unwrap_or(1.0),
                    speed_2_to_3: nt.speed_mid_to_mid2.unwrap_or(1.0),
                    speed_3_to_4: nt.speed_mid2_to_end.unwrap_or(1.0),
                    post_step_1: None,
                    post_step_2: None,
                    post_step_3: None,
                })
            }
            _ => None,
        };

        let config = NozzleTipConfig {
            name: nt.name.clone(),
            pick_dwell_ms: nt.pick_dwell_ms.unwrap_or(200),
            place_dwell_ms: nt.place_dwell_ms.unwrap_or(100),
            min_part_diameter: 0.0,
            max_part_diameter: 10.0,
            max_part_height: 10.0,
            vacuum: None,
            changer,
        };

        tips.insert(nt.name, config);
    }

    Ok(tips)
}

fn extract_feeder_slots(xml: &str) -> Result<Vec<(u8, Option<FeederLocation>)>, Box<dyn std::error::Error>> {
    let mut slots = Vec::new();

    for chunk in split_xml_elements(xml, "slot") {
        let slot: XmlSlot = match from_str(&chunk) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let loc = slot
            .location
            .filter(|l| !l.is_zero())
            .map(|l| l.to_feeder_location());

        slots.push((slot.address, loc));
    }

    Ok(slots)
}

fn build_feeders_from_slots(
    slots: &[(u8, Option<FeederLocation>)],
) -> HashMap<String, FeederConfig> {
    let mut feeders = HashMap::new();

    for (address, location) in slots {
        if let Some(loc) = location {
            let name = format!("slot_{}", address);
            feeders.insert(
                name,
                FeederConfig::Photon(PhotonFeederConfig {
                    enabled: true,
                    part_id: String::new(),
                    hardware_id: String::new(),
                    slot_address: *address,
                    location: FeederLocation { ..loc.clone() },
                    part_pitch: 2.0,
                    retry_count: 3,
                    feed_retry_count: 3,
                    pick_retry_count: 0,
                }),
            );
        }
    }

    feeders
}

fn extract_parts(xml: &str) -> Result<HashMap<String, PartConfig>, Box<dyn std::error::Error>> {
    let parsed: XmlParts = from_str(xml)?;
    let mut parts = HashMap::new();

    for p in parsed.parts {
        parts.insert(
            p.id.clone(),
            PartConfig {
                package_id: p.package_id.unwrap_or_default(),
                height: p.height,
                speed: p.speed,
                pick_retry_count: p.pick_retry_count,
            },
        );
    }

    Ok(parts)
}

fn extract_packages(xml: &str) -> Result<HashMap<String, PackageConfig>, Box<dyn std::error::Error>> {
    let parsed: XmlPackages = from_str(xml)?;
    let mut packages = HashMap::new();

    for pkg in parsed.packages {
        let pads = pkg
            .footprint
            .as_ref()
            .map(|fp| {
                fp.pads
                    .iter()
                    .map(|p| PadConfig {
                        name: p.name.clone(),
                        x: p.x,
                        y: p.y,
                        width: p.width,
                        height: p.height,
                        rotation: p.rotation,
                        roundness: p.roundness,
                    })
                    .collect()
            })
            .unwrap_or_default();

        let compatible_nozzle_tips = pkg
            .compatible_tips
            .map(|ct| ct.ids)
            .unwrap_or_default();

        packages.insert(
            pkg.id,
            PackageConfig {
                body_width: pkg.footprint.as_ref().map(|f| f.body_width).unwrap_or(0.0),
                body_height: pkg.footprint.as_ref().map(|f| f.body_height).unwrap_or(0.0),
                compatible_nozzle_tips,
                pads,
            },
        );
    }

    Ok(packages)
}

/// Extract XML elements by tag name from a larger document.
/// Handles both `<tag ...>...</tag>` and self-closing `<tag .../>`.
fn split_xml_elements(xml: &str, tag: &str) -> Vec<String> {
    let open = format!("<{} ", tag);
    let close = format!("</{}>", tag);
    let mut results = Vec::new();
    let mut search_from = 0;

    while search_from < xml.len() {
        let start = match xml[search_from..].find(&open) {
            Some(pos) => search_from + pos,
            None => break,
        };

        // Find the first '>' or '/>' after the opening tag
        let after_open = &xml[start..];

        // Check if self-closing comes before a regular close
        let self_close = after_open.find("/>");
        let regular_gt = after_open.find('>');

        match (self_close, regular_gt) {
            (Some(sc), Some(gt)) if sc < gt => {
                // Self-closing: <tag attr/>
                let end = start + sc + 2;
                results.push(xml[start..end].to_string());
                search_from = end;
            }
            (Some(sc), Some(gt)) if sc == gt - 1 => {
                // Also self-closing: <tag attr />  (the > is part of />)
                let end = start + sc + 2;
                results.push(xml[start..end].to_string());
                search_from = end;
            }
            _ => {
                // Has children — find matching close tag
                if let Some(close_pos) = xml[start..].find(&close) {
                    let end = start + close_pos + close.len();
                    results.push(xml[start..end].to_string());
                    search_from = end;
                } else {
                    search_from = start + 1;
                }
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_parts_xml() {
        let xml = r#"<openpnp-parts>
   <part id="R0805-1K" height-units="Millimeters" height="1.0" package-id="R0805" speed="1.0" pick-retry-count="0"/>
   <part id="R0402-1K" height-units="Millimeters" height="0.5" package-id="R0402" speed="1.0" pick-retry-count="0"/>
</openpnp-parts>"#;
        let parts = extract_parts(xml).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts["R0805-1K"].package_id, "R0805");
        assert!((parts["R0805-1K"].height - 1.0).abs() < 0.001);
        assert_eq!(parts["R0402-1K"].package_id, "R0402");
    }

    #[test]
    fn test_parse_packages_xml() {
        let xml = r#"<openpnp-packages>
   <package version="1.1" id="R0805" pick-vacuum-level="0.0" place-blow-off-level="0.0">
      <footprint units="Millimeters" body-width="2.0" body-height="1.25">
         <pad name="1" x="-0.825" y="0.0" width="0.35" height="1.25" rotation="0.0" roundness="0.0"/>
         <pad name="2" x="0.825" y="0.0" width="0.35" height="1.25" rotation="0.0" roundness="0.0"/>
      </footprint>
      <compatible-nozzle-tip-ids class="java.util.ArrayList">
         <string>NT1</string>
      </compatible-nozzle-tip-ids>
   </package>
</openpnp-packages>"#;
        let packages = extract_packages(xml).unwrap();
        assert_eq!(packages.len(), 1);
        let r0805 = &packages["R0805"];
        assert!((r0805.body_width - 2.0).abs() < 0.001);
        assert_eq!(r0805.pads.len(), 2);
        assert_eq!(r0805.compatible_nozzle_tips, vec!["NT1"]);
    }

    #[test]
    fn test_extract_nozzle_tip() {
        let xml = r#"<machine>
         <nozzle-tip class="org.openpnp.machine.reference.ReferenceNozzleTip" id="NT1" name="N045" pick-dwell-milliseconds="200" place-dwell-milliseconds="100">
            <changer-start-location units="Millimeters" x="43.614" y="124.185" z="31.0" rotation="0.0"/>
            <changer-start-to-mid-speed>0.05</changer-start-to-mid-speed>
            <changer-mid-location units="Millimeters" x="43.614" y="124.185" z="5.5" rotation="0.0"/>
            <changer-mid-to-mid-2-speed>0.2</changer-mid-to-mid-2-speed>
            <changer-mid-location-2 units="Millimeters" x="43.614" y="124.185" z="8.5" rotation="0.0"/>
            <changer-mid-2-to-end-speed>0.5</changer-mid-2-to-end-speed>
            <changer-end-location units="Millimeters" x="63.814" y="124.185" z="8.5" rotation="0.0"/>
            <touch-location units="Millimeters" x="0.0" y="0.0" z="0.0" rotation="0.0"/>
         </nozzle-tip>
</machine>"#;
        let tips = extract_nozzle_tips(xml).unwrap();
        assert_eq!(tips.len(), 1);
        let tip = &tips["N045"];
        assert_eq!(tip.pick_dwell_ms, 200);
        assert_eq!(tip.place_dwell_ms, 100);
        let changer = tip.changer.as_ref().unwrap();
        assert!((changer.first.z - 31.0).abs() < 0.001);
        assert!((changer.second.z - 5.5).abs() < 0.001);
        assert!((changer.speed_1_to_2 - 0.05).abs() < 0.001);
        assert!((changer.last.x - 63.814).abs() < 0.001);
    }

    #[test]
    fn test_extract_slots() {
        let xml = r#"<machine>
               <slot address="1">
                     <location units="Millimeters" x="28.673" y="45.468" z="4.0" rotation="0.0"/>
               </slot>
               <slot address="3"/>
               <slot address="6">
                     <location units="Millimeters" x="106.439" y="49.801" z="4.5" rotation="200.0"/>
               </slot>
</machine>"#;
        let slots = extract_feeder_slots(xml).unwrap();
        // Should have slot 1 and 6 with locations, slot 3 without
        let with_loc: Vec<_> = slots.iter().filter(|(_, l)| l.is_some()).collect();
        assert_eq!(with_loc.len(), 2);

        let feeders = build_feeders_from_slots(&slots);
        assert_eq!(feeders.len(), 2);
        assert!(feeders.contains_key("slot_1"));
        assert!(feeders.contains_key("slot_6"));
    }
}
