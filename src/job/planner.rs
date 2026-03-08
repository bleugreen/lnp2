use std::collections::HashMap;

use tracing::{debug, info};

use crate::config::boards::PlacementConfig;
use crate::config::feeders::FeederConfig;
use crate::config::parts::{PackageConfig, PartConfig};
use crate::config::FullConfig;
use crate::motion::NozzleId;

use super::types::{JobStep, NozzleAssignment, PlacementState, TipChange};

/// A planned job: ordered list of steps to execute.
#[derive(Debug)]
pub struct JobPlan {
    pub steps: Vec<JobStep>,
}

/// Placement with its resolved context for planning.
#[derive(Debug, Clone)]
struct ResolvedPlacement {
    board_idx: usize,
    placement_idx: usize,
    placement: PlacementConfig,
    part: PartConfig,
    package: PackageConfig,
    feeder_id: String,
    feeder: FeederConfig,
    compatible_tips: Vec<String>,
}

/// Plan the execution order for all pending placements.
///
/// Algorithm:
/// 1. Resolve each placement → part → package → compatible tips + feeder
/// 2. Group by compatible nozzle tip
/// 3. Sort groups: most placements first, "inflexible first" tiebreaker
/// 4. Within each tip group, pair placements for N1+N2 (greedy nearest)
/// 5. Insert tip changes when needed
/// 6. Output ordered JobSteps
pub fn plan_job(
    boards: &[(usize, &[PlacementConfig], &[PlacementState])],
    config: &FullConfig,
    current_n1_tip: Option<&str>,
    current_n2_tip: Option<&str>,
) -> JobPlan {
    // Step 1: Resolve all pending placements
    let mut resolved = Vec::new();
    for &(board_idx, placements, states) in boards {
        for (placement_idx, placement) in placements.iter().enumerate() {
            if !placement.enabled {
                continue;
            }
            if !matches!(states[placement_idx], PlacementState::Pending) {
                continue;
            }

            let part = match config.parts.get(&placement.part_id) {
                Some(p) => p.clone(),
                None => {
                    debug!("Skipping {}: part '{}' not found", placement.reference, placement.part_id);
                    continue;
                }
            };

            let package = match config.packages.get(&part.package_id) {
                Some(p) => p.clone(),
                None => {
                    debug!("Skipping {}: package '{}' not found", placement.reference, part.package_id);
                    continue;
                }
            };

            // Find a feeder that has this part
            let feeder_entry = config
                .feeders
                .iter()
                .find(|(_, f)| is_feeder_for_part(f, &placement.part_id));

            let (feeder_id, feeder) = match feeder_entry {
                Some((id, f)) => (id.clone(), f.clone()),
                None => {
                    debug!("Skipping {}: no feeder for part '{}'", placement.reference, placement.part_id);
                    continue;
                }
            };

            let compatible_tips = if package.compatible_nozzle_tips.is_empty() {
                // If no tips specified, assume compatible with all
                config.nozzle_tips.keys().cloned().collect()
            } else {
                package.compatible_nozzle_tips.clone()
            };

            resolved.push(ResolvedPlacement {
                board_idx,
                placement_idx,
                placement: placement.clone(),
                part,
                package,
                feeder_id,
                feeder,
                compatible_tips,
            });
        }
    }

    if resolved.is_empty() {
        return JobPlan { steps: Vec::new() };
    }

    info!("Planning {} placements", resolved.len());

    // Step 2: Group by compatible tip
    let mut tip_groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, r) in resolved.iter().enumerate() {
        for tip in &r.compatible_tips {
            tip_groups.entry(tip.clone()).or_default().push(idx);
        }
    }

    // Step 3: Sort tip groups — most placements first, inflexible tiebreaker
    let mut sorted_tips: Vec<(String, Vec<usize>)> = tip_groups.into_iter().collect();
    sorted_tips.sort_by(|a, b| {
        b.1.len().cmp(&a.1.len()).then_with(|| {
            // Inflexible first: tips whose placements have fewer alternative tips
            let avg_flex_a: f64 = a
                .1
                .iter()
                .map(|&i| resolved[i].compatible_tips.len() as f64)
                .sum::<f64>()
                / a.1.len() as f64;
            let avg_flex_b: f64 = b
                .1
                .iter()
                .map(|&i| resolved[i].compatible_tips.len() as f64)
                .sum::<f64>()
                / b.1.len() as f64;
            avg_flex_a.partial_cmp(&avg_flex_b).unwrap()
        })
    });

    // Step 4: Assign placements to cycles, deduplicating
    let mut steps = Vec::new();
    let mut placed: Vec<bool> = vec![false; resolved.len()];
    let mut active_n1_tip = current_n1_tip.map(|s| s.to_string());
    let mut active_n2_tip = current_n2_tip.map(|s| s.to_string());

    for (tip_id, indices) in &sorted_tips {
        // Collect unplaced indices for this tip
        let pending: Vec<usize> = indices.iter().copied().filter(|&i| !placed[i]).collect();
        if pending.is_empty() {
            continue;
        }

        // Check if we need tip changes
        let need_n1_change = active_n1_tip.as_deref() != Some(tip_id);
        let need_n2_change = active_n2_tip.as_deref() != Some(tip_id);

        if need_n1_change || need_n2_change {
            let mut changes = Vec::new();
            if need_n1_change {
                changes.push(TipChange {
                    nozzle: NozzleId::N1,
                    from_tip: active_n1_tip.clone(),
                    to_tip: tip_id.clone(),
                });
                active_n1_tip = Some(tip_id.clone());
            }
            if need_n2_change {
                changes.push(TipChange {
                    nozzle: NozzleId::N2,
                    from_tip: active_n2_tip.clone(),
                    to_tip: tip_id.clone(),
                });
                active_n2_tip = Some(tip_id.clone());
            }
            steps.push(JobStep::ChangeTips(changes));
        }

        // Pair placements for dual-nozzle cycles (N1 + N2)
        let mut remaining: Vec<usize> = pending;

        while !remaining.is_empty() {
            let mut cycle_assignments = Vec::new();

            // Assign first placement to N1
            let n1_idx = remaining.remove(0);
            placed[n1_idx] = true;
            cycle_assignments.push(make_assignment(
                NozzleId::N1,
                tip_id,
                &resolved[n1_idx],
            ));

            // Find nearest compatible placement for N2
            if !remaining.is_empty() {
                let n1_pos = (
                    resolved[n1_idx].placement.x,
                    resolved[n1_idx].placement.y,
                );
                let nearest = find_nearest(&remaining, &resolved, n1_pos);
                if let Some(pos) = nearest {
                    let n2_idx = remaining.remove(pos);
                    placed[n2_idx] = true;
                    cycle_assignments.push(make_assignment(
                        NozzleId::N2,
                        tip_id,
                        &resolved[n2_idx],
                    ));
                }
            }

            // Generate pick → align → place steps for this cycle
            steps.push(JobStep::PickBatch(cycle_assignments.clone()));
            steps.push(JobStep::AlignBatch(cycle_assignments.clone()));
            steps.push(JobStep::PlaceBatch(cycle_assignments));
        }
    }

    info!("Planned {} steps", steps.len());
    JobPlan { steps }
}

fn is_feeder_for_part(feeder: &FeederConfig, part_id: &str) -> bool {
    match feeder {
        FeederConfig::Photon(p) => p.enabled && p.part_id == part_id,
        FeederConfig::Tray(t) => t.enabled && t.part_id == part_id,
    }
}

fn make_assignment(nozzle: NozzleId, tip_id: &str, r: &ResolvedPlacement) -> NozzleAssignment {
    NozzleAssignment {
        nozzle,
        tip_id: tip_id.to_string(),
        feeder_id: r.feeder_id.clone(),
        part_id: r.placement.part_id.clone(),
        board_idx: r.board_idx,
        placement_idx: r.placement_idx,
        alignment: None,
    }
}

/// Find the index within `remaining` whose placement is nearest to `pos`.
fn find_nearest(
    remaining: &[usize],
    resolved: &[ResolvedPlacement],
    pos: (f64, f64),
) -> Option<usize> {
    remaining
        .iter()
        .enumerate()
        .min_by(|(_, &a), (_, &b)| {
            let da = dist_sq(pos, (resolved[a].placement.x, resolved[a].placement.y));
            let db = dist_sq(pos, (resolved[b].placement.x, resolved[b].placement.y));
            da.partial_cmp(&db).unwrap()
        })
        .map(|(i, _)| i)
}

fn dist_sq(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::boards::BoardSide;
    use crate::config::feeders::{FeederLocation, PhotonFeederConfig};
    use crate::config::nozzle_tips::NozzleTipConfig;

    fn test_config() -> FullConfig {
        let mut config = FullConfig {
            machine: crate::config::MachineConfig {
                serial: crate::config::SerialConfig {
                    port: "auto".into(),
                    baud: 115200,
                    timeout_ms: 5000,
                    motion_timeout_ms: 30000,
                    home_timeout_ms: 60000,
                },
                motion: crate::config::MotionConfig {
                    safe_z: 31.5,
                    default_feedrate: 50000.0,
                    default_acceleration: 500.0,
                },
                axes: HashMap::new(),
                nozzles: HashMap::new(),
                cameras: HashMap::new(),
                leds: crate::config::LedConfig {
                    on: "M150 P255".into(),
                    off: "M150 P0".into(),
                },
                connect: crate::config::ConnectConfig {
                    init_commands: vec![],
                },
            },
            feeders: HashMap::new(),
            parts: HashMap::new(),
            packages: HashMap::new(),
            nozzle_tips: HashMap::new(),
            boards: HashMap::new(),
            jobs: HashMap::new(),
        };

        // Add a nozzle tip
        config.nozzle_tips.insert(
            "N045".into(),
            NozzleTipConfig {
                name: "N045".into(),
                pick_dwell_ms: 200,
                place_dwell_ms: 100,
                min_part_diameter: 0.0,
                max_part_diameter: 5.0,
                max_part_height: 5.0,
                vacuum: None,
                changer: None,
            },
        );

        // Add parts
        config.parts.insert(
            "R0805_1K".into(),
            PartConfig {
                package_id: "R0805".into(),
                height: 0.5,
                speed: 1.0,
                pick_retry_count: 0,
            },
        );
        config.parts.insert(
            "C_0603_100nF".into(),
            PartConfig {
                package_id: "C_0603".into(),
                height: 0.4,
                speed: 1.0,
                pick_retry_count: 0,
            },
        );

        // Add packages
        config.packages.insert(
            "R0805".into(),
            PackageConfig {
                body_width: 2.0,
                body_height: 1.25,
                compatible_nozzle_tips: vec!["N045".into()],
                pads: vec![],
            },
        );
        config.packages.insert(
            "C_0603".into(),
            PackageConfig {
                body_width: 1.6,
                body_height: 0.8,
                compatible_nozzle_tips: vec!["N045".into()],
                pads: vec![],
            },
        );

        // Add feeders
        config.feeders.insert(
            "slot_1".into(),
            FeederConfig::Photon(PhotonFeederConfig {
                enabled: true,
                part_id: "R0805_1K".into(),
                hardware_id: "abc".into(),
                slot_address: 1,
                location: FeederLocation {
                    x: 100.0,
                    y: 50.0,
                    z: 5.0,
                    rotation: 0.0,
                },
                part_pitch: 2.0,
                retry_count: 3,
                feed_retry_count: 3,
                pick_retry_count: 0,
            }),
        );
        config.feeders.insert(
            "slot_2".into(),
            FeederConfig::Photon(PhotonFeederConfig {
                enabled: true,
                part_id: "C_0603_100nF".into(),
                hardware_id: "def".into(),
                slot_address: 2,
                location: FeederLocation {
                    x: 110.0,
                    y: 50.0,
                    z: 5.0,
                    rotation: 0.0,
                },
                part_pitch: 2.0,
                retry_count: 3,
                feed_retry_count: 3,
                pick_retry_count: 0,
            }),
        );

        config
    }

    #[test]
    fn test_plan_basic() {
        let config = test_config();

        let placements = vec![
            PlacementConfig {
                reference: "R1".into(),
                part_id: "R0805_1K".into(),
                x: 50.0,
                y: 30.0,
                rotation: 0.0,
                side: BoardSide::Top,
                enabled: true,
            },
            PlacementConfig {
                reference: "C1".into(),
                part_id: "C_0603_100nF".into(),
                x: 55.0,
                y: 35.0,
                rotation: 90.0,
                side: BoardSide::Top,
                enabled: true,
            },
        ];
        let states = vec![PlacementState::Pending, PlacementState::Pending];

        let boards = vec![(0, placements.as_slice(), states.as_slice())];
        let plan = plan_job(&boards, &config, None, None);

        // Should produce tip change + pick/align/place cycles
        assert!(!plan.steps.is_empty());

        // Count assignments
        let total_assignments: usize = plan
            .steps
            .iter()
            .filter_map(|s| match s {
                JobStep::PlaceBatch(a) => Some(a.len()),
                _ => None,
            })
            .sum();
        assert_eq!(total_assignments, 2);
    }

    #[test]
    fn test_plan_skips_disabled() {
        let config = test_config();

        let placements = vec![
            PlacementConfig {
                reference: "R1".into(),
                part_id: "R0805_1K".into(),
                x: 50.0,
                y: 30.0,
                rotation: 0.0,
                side: BoardSide::Top,
                enabled: false, // disabled
            },
        ];
        let states = vec![PlacementState::Pending];

        let boards = vec![(0, placements.as_slice(), states.as_slice())];
        let plan = plan_job(&boards, &config, None, None);
        assert!(plan.steps.is_empty());
    }

    #[test]
    fn test_plan_skips_already_placed() {
        let config = test_config();

        let placements = vec![
            PlacementConfig {
                reference: "R1".into(),
                part_id: "R0805_1K".into(),
                x: 50.0,
                y: 30.0,
                rotation: 0.0,
                side: BoardSide::Top,
                enabled: true,
            },
        ];
        let states = vec![PlacementState::Placed];

        let boards = vec![(0, placements.as_slice(), states.as_slice())];
        let plan = plan_job(&boards, &config, None, None);
        assert!(plan.steps.is_empty());
    }

    #[test]
    fn test_plan_dual_nozzle_pairing() {
        let config = test_config();

        // 4 placements with same tip — should produce 2 cycles with 2 nozzles each
        let placements = vec![
            PlacementConfig {
                reference: "R1".into(),
                part_id: "R0805_1K".into(),
                x: 50.0,
                y: 30.0,
                rotation: 0.0,
                side: BoardSide::Top,
                enabled: true,
            },
            PlacementConfig {
                reference: "R2".into(),
                part_id: "R0805_1K".into(),
                x: 52.0,
                y: 30.0,
                rotation: 0.0,
                side: BoardSide::Top,
                enabled: true,
            },
            PlacementConfig {
                reference: "R3".into(),
                part_id: "R0805_1K".into(),
                x: 54.0,
                y: 30.0,
                rotation: 0.0,
                side: BoardSide::Top,
                enabled: true,
            },
            PlacementConfig {
                reference: "R4".into(),
                part_id: "R0805_1K".into(),
                x: 56.0,
                y: 30.0,
                rotation: 0.0,
                side: BoardSide::Top,
                enabled: true,
            },
        ];
        let states = vec![
            PlacementState::Pending,
            PlacementState::Pending,
            PlacementState::Pending,
            PlacementState::Pending,
        ];

        let boards = vec![(0, placements.as_slice(), states.as_slice())];
        let plan = plan_job(&boards, &config, None, None);

        // Should have 2 place batches, each with 2 assignments
        let place_batches: Vec<_> = plan
            .steps
            .iter()
            .filter_map(|s| match s {
                JobStep::PlaceBatch(a) => Some(a),
                _ => None,
            })
            .collect();
        assert_eq!(place_batches.len(), 2);
        assert_eq!(place_batches[0].len(), 2);
        assert_eq!(place_batches[1].len(), 2);
    }
}
