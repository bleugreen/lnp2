use std::path::PathBuf;

use opencv::core::Mat;
use opencv::imgcodecs;
use opencv::prelude::*;
use tracing::warn;

use super::types::VisionConfig;

/// Accumulates intermediate images and metadata during a detection run.
/// Optionally saves to disk for post-mortem debugging.
pub struct VisionContext {
    checkpoints: Vec<(String, Vec<u8>)>,
    log: Vec<String>,
    save_to: Option<PathBuf>,
}

impl VisionContext {
    pub fn new(config: &VisionConfig) -> Self {
        let save_to = if config.save_diagnostics {
            let dir = config
                .diagnostics_dir
                .as_deref()
                .unwrap_or("/tmp/rsrvpnp-vision");
            let timestamp = chrono_timestamp();
            Some(PathBuf::from(dir).join(timestamp))
        } else {
            None
        };

        Self {
            checkpoints: Vec::new(),
            log: Vec::new(),
            save_to,
        }
    }

    /// Encode a Mat as JPEG and store as a checkpoint.
    pub fn checkpoint(&mut self, name: &str, mat: &Mat) {
        if self.save_to.is_none() {
            return;
        }
        let mut buf = opencv::core::Vector::<u8>::new();
        let params = opencv::core::Vector::<i32>::new();
        if imgcodecs::imencode(".jpg", mat, &mut buf, &params).unwrap_or(false) {
            let bytes: Vec<u8> = buf.iter().collect();
            self.checkpoints.push((name.to_string(), bytes));
        }
    }

    pub fn log(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
    }

    /// Consume the context, returning all collected diagnostics.
    pub fn into_diagnostics(mut self) -> Vec<(String, Vec<u8>)> {
        // Disable the Drop-based saving since we're returning the data
        self.save_to = None;
        std::mem::take(&mut self.checkpoints)
    }
}

impl Drop for VisionContext {
    fn drop(&mut self) {
        if let Some(ref dir) = self.save_to {
            if let Err(e) = std::fs::create_dir_all(dir) {
                warn!("Failed to create diagnostics dir: {}", e);
                return;
            }
            for (i, (name, data)) in self.checkpoints.iter().enumerate() {
                let path = dir.join(format!("{:02}_{}.jpg", i, name));
                if let Err(e) = std::fs::write(&path, data) {
                    warn!("Failed to write diagnostic {}: {}", path.display(), e);
                }
            }
            let log_path = dir.join("log.txt");
            let _ = std::fs::write(&log_path, self.log.join("\n"));
        }
    }
}

fn chrono_timestamp() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}
