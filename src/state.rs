use std::sync::Arc;

use tokio::sync::RwLock;

use crate::actuators::ActuatorController;
use crate::camera::CameraManager;
use crate::config::{FullConfig, MachineConfig};
use crate::events::EventBus;
use crate::gcode::GCodeDriver;
use crate::motion::MotionController;

#[derive(Clone)]
pub struct AppState {
    pub gcode: Arc<GCodeDriver>,
    pub motion: Arc<MotionController>,
    pub actuators: Arc<ActuatorController>,
    pub config: Arc<RwLock<MachineConfig>>,
    pub full_config: Arc<RwLock<FullConfig>>,
    pub camera: Option<Arc<CameraManager>>,
    pub event_bus: Arc<EventBus>,
}

impl AppState {
    pub fn new(
        gcode: Arc<GCodeDriver>,
        full_config: FullConfig,
        camera: Option<CameraManager>,
        event_bus: EventBus,
    ) -> Self {
        let machine_config = Arc::new(RwLock::new(full_config.machine.clone()));
        let motion = Arc::new(MotionController::new(gcode.clone(), machine_config.clone()));
        let actuators = Arc::new(ActuatorController::new(gcode.clone(), machine_config.clone()));

        Self {
            gcode,
            motion,
            actuators,
            config: machine_config,
            full_config: Arc::new(RwLock::new(full_config)),
            camera: camera.map(Arc::new),
            event_bus: Arc::new(event_bus),
        }
    }
}
