use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
pub enum Event {
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
