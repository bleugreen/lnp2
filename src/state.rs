use std::sync::Arc;

use tokio::sync::RwLock;

use crate::actuators::ActuatorController;
use crate::config::MachineConfig;
use crate::gcode::GCodeDriver;
use crate::motion::MotionController;

#[derive(Clone)]
pub struct AppState {
    pub gcode: Arc<GCodeDriver>,
    pub motion: Arc<MotionController>,
    pub actuators: Arc<ActuatorController>,
    pub config: Arc<RwLock<MachineConfig>>,
}

impl AppState {
    pub fn new(
        gcode: Arc<GCodeDriver>,
        config: Arc<RwLock<MachineConfig>>,
    ) -> Self {
        let motion = Arc::new(MotionController::new(gcode.clone(), config.clone()));
        let actuators = Arc::new(ActuatorController::new(gcode.clone(), config.clone()));

        Self {
            gcode,
            motion,
            actuators,
            config,
        }
    }
}
