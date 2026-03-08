use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
pub enum Event {
    // Machine events
    Position {
        x: f64,
        y: f64,
        z: f64,
        a: f64,
        b: f64,
    },
    MotionComplete,
    VacuumState {
        nozzle: String,
        on: bool,
    },
    Connected,

    // Job lifecycle events
    JobStarted {
        job_name: String,
        total_placements: usize,
    },
    JobPaused {
        reason: crate::job::types::PauseReason,
    },
    JobResumed,
    JobComplete {
        stats: crate::job::types::JobStats,
    },
    JobError {
        message: String,
    },

    // Fiducial events
    FiducialComplete {
        board_idx: usize,
    },

    // Tip change events
    TipChange {
        nozzle: String,
        tip: String,
    },

    // Pick events
    Picking {
        nozzle: String,
        part_id: String,
        feeder_id: String,
    },
    PickComplete {
        nozzle: String,
        success: bool,
    },

    // Alignment events
    Aligning {
        nozzle: String,
    },
    AlignComplete {
        nozzle: String,
        offset_x: f64,
        offset_y: f64,
        rotation: f64,
    },

    // Placement events
    Placing {
        nozzle: String,
        reference: String,
        board_idx: usize,
    },
    PlacementComplete {
        reference: String,
        board_idx: usize,
        success: bool,
    },
    PlacementProgress {
        completed: usize,
        total: usize,
        elapsed_secs: f64,
    },
}

pub struct EventBus {
    tx: broadcast::Sender<Event>,
    /// When true, position polling should pause to avoid serial contention.
    pub busy: AtomicBool,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self {
            tx,
            busy: AtomicBool::new(false),
        }
    }

    pub fn publish(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}
