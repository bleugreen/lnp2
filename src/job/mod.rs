pub mod align;
pub mod board;
pub mod pick;
pub mod place;
pub mod planner;
pub mod types;

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{mpsc, RwLock};
use tracing::{error, info, warn};

use crate::changer::NozzleTipChanger;
use crate::config::boards::BoardConfig;
use crate::config::jobs::JobConfig;
use crate::config::FullConfig;
use crate::events::Event;
use crate::state::AppState;

use self::types::*;

/// Commands sent to a running job.
#[derive(Debug)]
pub enum JobControl {
    Pause,
    Resume,
    Abort,
    SkipPlacement {
        board_idx: usize,
        placement_idx: usize,
    },
}

/// Result of a completed (or failed) job.
#[derive(Debug)]
pub struct JobResult {
    pub stats: JobStats,
    pub status: JobStatus,
}

/// Handle for controlling a running job.
#[derive(Clone)]
pub struct JobHandle {
    pub control_tx: mpsc::Sender<JobControl>,
    pub state: Arc<RwLock<JobState>>,
}

/// Start a job as a background tokio task.
///
/// Returns a JobHandle for monitoring and controlling the job.
pub fn start_job(
    job_config: JobConfig,
    board_configs: Vec<BoardConfig>,
    app_state: AppState,
) -> JobHandle {
    let total_placements: usize = board_configs
        .iter()
        .map(|b| b.placements.iter().filter(|p| p.enabled).count())
        .sum();

    let boards: Vec<BoardState> = board_configs
        .iter()
        .enumerate()
        .map(|(idx, bc)| BoardState {
            board_idx: idx,
            transform: None,
            placements: bc
                .placements
                .iter()
                .map(|p| {
                    if p.enabled {
                        PlacementState::Pending
                    } else {
                        PlacementState::Skipped
                    }
                })
                .collect(),
        })
        .collect();

    let job_state = Arc::new(RwLock::new(JobState {
        status: JobStatus::Running,
        config: job_config.clone(),
        boards,
        current_step: None,
        stats: JobStats::new(total_placements),
    }));

    let (control_tx, control_rx) = mpsc::channel(16);

    let handle = JobHandle {
        control_tx: control_tx.clone(),
        state: job_state.clone(),
    };

    // Spawn the runner task
    let state_clone = app_state.clone();
    tokio::spawn(async move {
        let mut runner = JobRunner {
            app_state: state_clone,
            job_state,
            job_config,
            board_configs,
            control_rx,
            n1_tip: None,
            n2_tip: None,
        };
        runner.run().await;
    });

    handle
}

struct JobRunner {
    app_state: AppState,
    job_state: Arc<RwLock<JobState>>,
    job_config: JobConfig,
    board_configs: Vec<BoardConfig>,
    control_rx: mpsc::Receiver<JobControl>,
    n1_tip: Option<String>,
    n2_tip: Option<String>,
}

impl JobRunner {
    async fn run(&mut self) {
        {
            let mut state = self.job_state.write().await;
            state.stats.started_at = Some(Instant::now());
        }

        let total = {
            let state = self.job_state.read().await;
            state.stats.total_placements
        };

        self.emit(Event::JobStarted {
            job_name: self.job_config.name.clone(),
            total_placements: total,
        });

        info!("Job '{}' started with {} placements", self.job_config.name, total);

        // Phase 1: Fiducial checks for each board
        for (board_idx, job_board) in self.job_config.boards.iter().enumerate() {
            if !job_board.enabled {
                continue;
            }
            if let Err(e) = self.check_control().await {
                self.finish(JobStatus::Error {
                    message: e.to_string(),
                })
                .await;
                return;
            }

            let board_config = &self.board_configs[board_idx];
            if board_config.fiducials.len() >= 2 {
                self.update_step(&format!("Locating fiducials for board {}", board_idx))
                    .await;

                match board::locate_board(
                    &board_config.fiducials,
                    &job_board.origin,
                    &self.app_state,
                )
                .await
                {
                    Ok(transform) => {
                        let mut state = self.job_state.write().await;
                        state.boards[board_idx].transform = Some(transform);
                        self.emit(Event::FiducialComplete { board_idx });
                    }
                    Err(e) => {
                        error!("Fiducial check failed for board {}: {}", board_idx, e);
                        self.finish(JobStatus::Error {
                            message: format!("Fiducial check failed: {}", e),
                        })
                        .await;
                        return;
                    }
                }
            } else {
                // No fiducials — use origin-only transform
                let transform = AffineTransform::from_translation_rotation(
                    job_board.origin.x,
                    job_board.origin.y,
                    job_board.origin.rotation,
                );
                let mut state = self.job_state.write().await;
                state.boards[board_idx].transform = Some(transform);
            }
        }

        // Phase 2: Plan and execute placements
        let plan = {
            let state = self.job_state.read().await;
            let full_config = self.app_state.full_config.read().await;

            let board_data: Vec<_> = state
                .boards
                .iter()
                .enumerate()
                .map(|(i, bs)| {
                    (
                        i,
                        self.board_configs[i].placements.as_slice(),
                        bs.placements.as_slice(),
                    )
                })
                .collect();

            planner::plan_job(
                &board_data,
                &full_config,
                self.n1_tip.as_deref(),
                self.n2_tip.as_deref(),
            )
        };

        for step in plan.steps {
            if let Err(e) = self.check_control().await {
                self.finish(JobStatus::Error {
                    message: e.to_string(),
                })
                .await;
                return;
            }

            match step {
                JobStep::ChangeTips(changes) => {
                    if let Err(e) = self.execute_tip_changes(&changes).await {
                        error!("Tip change failed: {}", e);
                        self.finish(JobStatus::Error {
                            message: format!("Tip change failed: {}", e),
                        })
                        .await;
                        return;
                    }
                }
                JobStep::PickBatch(assignments) => {
                    for assign in &assignments {
                        if let Err(e) = self.execute_pick(assign).await {
                            error!("Pick failed for {}: {}", assign.part_id, e);
                            self.pause(PauseReason::PickFailed {
                                placement: assign.part_id.clone(),
                                attempts: 0,
                            })
                            .await;
                            // Wait for resume/abort
                            if let Err(e) = self.wait_for_resume().await {
                                self.finish(JobStatus::Error {
                                    message: e.to_string(),
                                })
                                .await;
                                return;
                            }
                        }
                    }
                }
                JobStep::AlignBatch(mut assignments) => {
                    for assign in &mut assignments {
                        if let Err(e) = self.execute_align(assign).await {
                            warn!("Alignment failed for {}: {}", assign.part_id, e);
                            // Continue without alignment — use zero offset
                            assign.alignment = Some(AlignmentOffset {
                                dx: 0.0,
                                dy: 0.0,
                                drot: 0.0,
                            });
                        }
                    }
                }
                JobStep::PlaceBatch(assignments) => {
                    for assign in &assignments {
                        if let Err(e) = self.execute_place(assign).await {
                            error!("Place failed for board[{}] placement[{}]: {}",
                                assign.board_idx, assign.placement_idx, e);
                            let mut state = self.job_state.write().await;
                            state.boards[assign.board_idx].placements[assign.placement_idx] =
                                PlacementState::Failed {
                                    reason: e.to_string(),
                                };
                            state.stats.failed += 1;
                        } else {
                            let mut state = self.job_state.write().await;
                            state.boards[assign.board_idx].placements[assign.placement_idx] =
                                PlacementState::Placed;
                            state.stats.completed += 1;
                            state.stats.update_elapsed();

                            self.emit(Event::PlacementProgress {
                                completed: state.stats.completed,
                                total: state.stats.total_placements,
                                elapsed_secs: state.stats.elapsed_secs,
                            });
                        }
                    }
                }
                JobStep::FiducialCheck { .. } => {
                    // Handled in phase 1
                }
            }
        }

        self.finish(JobStatus::Complete).await;
    }

    async fn execute_tip_changes(
        &mut self,
        changes: &[TipChange],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let changer = NozzleTipChanger::new(
            self.app_state.gcode.clone(),
            self.app_state.motion.clone(),
            self.app_state.full_config.clone(),
        );

        for change in changes {
            self.update_step(&format!(
                "Changing {} tip to {}",
                change.nozzle, change.to_tip
            ))
            .await;

            self.emit(Event::TipChange {
                nozzle: change.nozzle.to_string(),
                tip: change.to_tip.clone(),
            });

            // Unload current tip if any
            if let Some(ref from) = change.from_tip {
                changer.unload_tip(from).await?;
            }

            // Load new tip
            changer.load_tip(&change.to_tip).await?;

            match change.nozzle {
                crate::motion::NozzleId::N1 => self.n1_tip = Some(change.to_tip.clone()),
                crate::motion::NozzleId::N2 => self.n2_tip = Some(change.to_tip.clone()),
            }
        }

        Ok(())
    }

    async fn execute_pick(
        &self,
        assign: &NozzleAssignment,
    ) -> Result<(), pick::PickError> {
        let full_config = self.app_state.full_config.read().await;
        let feeder = full_config
            .feeders
            .get(&assign.feeder_id)
            .ok_or_else(|| pick::PickError::Feeder(format!("Feeder '{}' not found", assign.feeder_id)))?
            .clone();
        let tip = full_config
            .nozzle_tips
            .get(&assign.tip_id)
            .cloned()
            .unwrap_or_else(|| crate::config::NozzleTipConfig {
                name: assign.tip_id.clone(),
                pick_dwell_ms: 200,
                place_dwell_ms: 100,
                min_part_diameter: 0.0,
                max_part_diameter: 10.0,
                max_part_height: 10.0,
                vacuum: None,
                changer: None,
            });
        drop(full_config);

        self.update_step(&format!("Picking {} with {}", assign.part_id, assign.nozzle))
            .await;

        self.emit(Event::Picking {
            nozzle: assign.nozzle.to_string(),
            part_id: assign.part_id.clone(),
            feeder_id: assign.feeder_id.clone(),
        });

        let max_retries = match &feeder {
            crate::config::FeederConfig::Photon(p) => p.pick_retry_count,
            crate::config::FeederConfig::Tray(t) => t.pick_retry_count,
        };

        let retries =
            pick::pick_with_retry(assign.nozzle, &feeder, &tip, &self.app_state, max_retries)
                .await?;

        if retries > 0 {
            let mut state = self.job_state.write().await;
            state.stats.pick_retries += retries as usize;
        }

        self.emit(Event::PickComplete {
            nozzle: assign.nozzle.to_string(),
            success: true,
        });

        Ok(())
    }

    async fn execute_align(
        &self,
        assign: &mut NozzleAssignment,
    ) -> Result<(), align::AlignError> {
        let full_config = self.app_state.full_config.read().await;
        let part = full_config.parts.get(&assign.part_id);
        let package = part
            .and_then(|p| full_config.packages.get(&p.package_id))
            .cloned();
        drop(full_config);

        let package = match package {
            Some(p) => p,
            None => {
                // No package info — skip alignment
                assign.alignment = Some(AlignmentOffset {
                    dx: 0.0,
                    dy: 0.0,
                    drot: 0.0,
                });
                return Ok(());
            }
        };

        self.update_step(&format!("Aligning {} with {}", assign.part_id, assign.nozzle))
            .await;

        self.emit(Event::Aligning {
            nozzle: assign.nozzle.to_string(),
        });

        // Get the placement rotation for pre-rotation
        let placement_rot = {
            let state = self.job_state.read().await;
            self.board_configs[assign.board_idx].placements[assign.placement_idx].rotation
        };

        let offset =
            align::align_part(assign.nozzle, &package, placement_rot, &self.app_state).await?;

        self.emit(Event::AlignComplete {
            nozzle: assign.nozzle.to_string(),
            offset_x: offset.dx,
            offset_y: offset.dy,
            rotation: offset.drot,
        });

        assign.alignment = Some(offset);
        Ok(())
    }

    async fn execute_place(
        &self,
        assign: &NozzleAssignment,
    ) -> Result<(), place::PlaceError> {
        let state = self.job_state.read().await;
        let board_state = &state.boards[assign.board_idx];
        let placement = &self.board_configs[assign.board_idx].placements[assign.placement_idx];

        let full_config = self.app_state.full_config.read().await;
        let part = full_config
            .parts
            .get(&assign.part_id)
            .cloned()
            .unwrap_or_else(|| crate::config::PartConfig {
                package_id: String::new(),
                height: 0.0,
                speed: 1.0,
                pick_retry_count: 0,
            });
        let tip = full_config
            .nozzle_tips
            .get(&assign.tip_id)
            .cloned()
            .unwrap_or_else(|| crate::config::NozzleTipConfig {
                name: assign.tip_id.clone(),
                pick_dwell_ms: 200,
                place_dwell_ms: 100,
                min_part_diameter: 0.0,
                max_part_diameter: 10.0,
                max_part_height: 10.0,
                vacuum: None,
                changer: None,
            });
        drop(full_config);

        let alignment = assign.alignment.as_ref().cloned().unwrap_or(AlignmentOffset {
            dx: 0.0,
            dy: 0.0,
            drot: 0.0,
        });

        self.update_step(&format!(
            "Placing {} at board[{}]",
            placement.reference, assign.board_idx
        ))
        .await;

        self.emit(Event::Placing {
            nozzle: assign.nozzle.to_string(),
            reference: placement.reference.clone(),
            board_idx: assign.board_idx,
        });

        place::place_part(
            assign.nozzle,
            placement,
            board_state,
            &alignment,
            &part,
            &tip,
            &self.app_state,
        )
        .await?;

        self.emit(Event::PlacementComplete {
            reference: placement.reference.clone(),
            board_idx: assign.board_idx,
            success: true,
        });

        Ok(())
    }

    /// Check for control messages (pause/abort).
    async fn check_control(&mut self) -> Result<(), String> {
        while let Ok(ctrl) = self.control_rx.try_recv() {
            match ctrl {
                JobControl::Abort => return Err("Job aborted".to_string()),
                JobControl::Pause => {
                    self.pause(PauseReason::UserRequested).await;
                    self.wait_for_resume().await?;
                }
                JobControl::SkipPlacement {
                    board_idx,
                    placement_idx,
                } => {
                    let mut state = self.job_state.write().await;
                    if board_idx < state.boards.len()
                        && placement_idx < state.boards[board_idx].placements.len()
                    {
                        state.boards[board_idx].placements[placement_idx] =
                            PlacementState::Skipped;
                        state.stats.skipped += 1;
                    }
                }
                JobControl::Resume => {} // no-op if not paused
            }
        }
        Ok(())
    }

    async fn pause(&self, reason: PauseReason) {
        let mut state = self.job_state.write().await;
        state.status = JobStatus::Paused {
            reason: reason.clone(),
        };
        self.emit(Event::JobPaused { reason });
    }

    async fn wait_for_resume(&mut self) -> Result<(), String> {
        loop {
            match self.control_rx.recv().await {
                Some(JobControl::Resume) => {
                    let mut state = self.job_state.write().await;
                    state.status = JobStatus::Running;
                    self.emit(Event::JobResumed);
                    return Ok(());
                }
                Some(JobControl::Abort) => return Err("Job aborted".to_string()),
                Some(JobControl::SkipPlacement {
                    board_idx,
                    placement_idx,
                }) => {
                    let mut state = self.job_state.write().await;
                    if board_idx < state.boards.len()
                        && placement_idx < state.boards[board_idx].placements.len()
                    {
                        state.boards[board_idx].placements[placement_idx] =
                            PlacementState::Skipped;
                        state.stats.skipped += 1;
                    }
                }
                Some(JobControl::Pause) => {} // already paused
                None => return Err("Job control channel closed".to_string()),
            }
        }
    }

    async fn finish(&self, status: JobStatus) {
        let mut state = self.job_state.write().await;
        state.stats.update_elapsed();
        state.status = status.clone();
        state.current_step = None;

        match &status {
            JobStatus::Complete => {
                info!(
                    "Job '{}' complete: {}/{} placed, {} failed, {} skipped in {:.1}s",
                    state.config.name,
                    state.stats.completed,
                    state.stats.total_placements,
                    state.stats.failed,
                    state.stats.skipped,
                    state.stats.elapsed_secs,
                );
                self.emit(Event::JobComplete {
                    stats: state.stats.clone(),
                });
            }
            JobStatus::Error { message } => {
                error!("Job '{}' failed: {}", state.config.name, message);
                self.emit(Event::JobError {
                    message: message.clone(),
                });
            }
            _ => {}
        }
    }

    async fn update_step(&self, description: &str) {
        let mut state = self.job_state.write().await;
        state.current_step = Some(description.to_string());
    }

    fn emit(&self, event: Event) {
        self.app_state.event_bus.publish(event);
    }
}
