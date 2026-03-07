pub mod align_part;
pub mod context;
pub mod cv;
pub mod detect_fiducial;
pub mod detect_pocket;
pub mod error;
pub mod ml;
pub mod types;

use std::collections::HashMap;
use std::sync::Arc;

use ort::session::Session;
use tracing::{info, warn};

use crate::config::CameraConfig;
use self::error::VisionError;
use self::ml::ModelManager;

pub use self::align_part::align_part;
pub use self::detect_fiducial::detect_fiducial;
pub use self::detect_pocket::detect_pocket;
pub use self::types::{AlignmentResult, CameraCalibration, Detection, DetectionMethod, PadDetection};

/// Holds loaded ML models and config. Created at startup.
pub struct VisionEngine {
    models: ModelManager,
    camera_sessions: HashMap<String, Arc<Session>>,
}

impl VisionEngine {
    pub fn new(cameras: &HashMap<String, CameraConfig>) -> Result<Self, VisionError> {
        let models = ModelManager::new();
        let mut camera_sessions = HashMap::new();

        for (name, cam) in cameras {
            if let Some(ref vision) = cam.vision {
                if let Some(ref path) = vision.model_path {
                    match models.load(path) {
                        Ok(session) => {
                            info!("[vision] Loaded model for camera '{}': {}", name, path);
                            camera_sessions.insert(name.clone(), session);
                        }
                        Err(e) => {
                            warn!("[vision] Failed to load model for camera '{}': {}", name, e);
                        }
                    }
                }
            }
        }

        Ok(Self {
            models,
            camera_sessions,
        })
    }

    /// Get the ONNX session for a camera, if one was loaded.
    pub fn session_for(&self, camera_name: &str) -> Option<&Session> {
        self.camera_sessions
            .get(camera_name)
            .map(|s: &Arc<Session>| s.as_ref())
    }
}
