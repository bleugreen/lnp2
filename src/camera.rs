use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::config::CameraConfig;

#[derive(Debug, thiserror::Error)]
pub enum CameraError {
    #[error("Camera not found: {0}")]
    NotFound(String),
    #[error("Camera error: {0}")]
    Device(String),
    #[error("No frame available")]
    NoFrame,
}

pub struct CameraManager {
    cameras: HashMap<String, CameraHandle>,
}

struct CameraHandle {
    config: CameraConfig,
    latest_frame: Arc<RwLock<Option<Vec<u8>>>>,
    broadcast_tx: broadcast::Sender<Vec<u8>>,
    _task: JoinHandle<()>,
}

impl CameraManager {
    pub fn start(cameras: &HashMap<String, CameraConfig>) -> Self {
        let mut handles = HashMap::new();

        for (name, config) in cameras {
            let (broadcast_tx, _) = broadcast::channel(4); // small buffer, drop old frames
            let latest_frame: Arc<RwLock<Option<Vec<u8>>>> = Arc::new(RwLock::new(None));

            let frame_ref = latest_frame.clone();
            let tx = broadcast_tx.clone();
            let device = config.device.clone();
            let width = config.width;
            let height = config.height;
            let flip_x = config.flip_x;
            let flip_y = config.flip_y;
            let cam_name: String = name.clone();

            let task = tokio::task::spawn_blocking(move || {
                capture_loop(&cam_name, &device, width, height, flip_x, flip_y, frame_ref, tx);
            });

            handles.insert(
                name.clone(),
                CameraHandle {
                    config: config.clone(),
                    latest_frame,
                    broadcast_tx,
                    _task: task,
                },
            );
        }

        Self { cameras: handles }
    }

    /// Get latest captured frame (JPEG bytes).
    pub async fn capture(&self, name: &str) -> Result<Vec<u8>, CameraError> {
        let handle = self.cameras.get(name).ok_or_else(|| CameraError::NotFound(name.to_string()))?;
        let frame = handle.latest_frame.read().await;
        frame.clone().ok_or(CameraError::NoFrame)
    }

    /// Subscribe to live frame stream.
    pub fn subscribe(&self, name: &str) -> Result<broadcast::Receiver<Vec<u8>>, CameraError> {
        let handle = self.cameras.get(name).ok_or_else(|| CameraError::NotFound(name.to_string()))?;
        Ok(handle.broadcast_tx.subscribe())
    }

    /// List available camera names.
    pub fn list(&self) -> Vec<String> {
        self.cameras.keys().cloned().collect()
    }

    /// Get camera configs for the list endpoint.
    pub fn configs(&self) -> HashMap<String, &CameraConfig> {
        self.cameras
            .iter()
            .map(|(name, handle)| (name.clone(), &handle.config))
            .collect()
    }
}

fn capture_loop(
    name: &str,
    device: &str,
    width: u32,
    height: u32,
    flip_x: bool,
    flip_y: bool,
    latest_frame: Arc<RwLock<Option<Vec<u8>>>>,
    broadcast_tx: broadcast::Sender<Vec<u8>>,
) {
    use nokhwa::pixel_format::RgbFormat;
    use nokhwa::utils::{CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution};
    use nokhwa::Camera;

    let index = if let Some(idx) = device.strip_prefix("/dev/video") {
        match idx.parse::<u32>() {
            Ok(n) => CameraIndex::Index(n),
            Err(_) => {
                error!("[{}] Invalid device path: {}", name, device);
                return;
            }
        }
    } else {
        error!("[{}] Unsupported device path: {} (expected /dev/videoN)", name, device);
        return;
    };

    let format = RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(CameraFormat::new(
        Resolution::new(width, height),
        FrameFormat::MJPEG,
        15,
    )));

    let mut camera = match Camera::new(index, format) {
        Ok(c) => c,
        Err(e) => {
            warn!("[{}] Failed to open camera {}: {}", name, device, e);
            return;
        }
    };

    if let Err(e) = camera.open_stream() {
        warn!("[{}] Failed to start camera stream: {}", name, e);
        return;
    }

    // Create CLAHE instance (reused across frames)
    let mut clahe = match imgproc::create_clahe(3.0, Size::new(8, 8)) {
        Ok(c) => c,
        Err(e) => {
            warn!("[{}] Failed to create CLAHE, running without: {}", name, e);
            info!("[{}] Camera started: {} ({}x{}) [no CLAHE]", name, device, width, height);
            // Fall back to simple loop without CLAHE
            loop {
                match camera.frame_raw() {
                    Ok(raw_bytes) => {
                        let jpeg = raw_bytes.to_vec();
                        if let Ok(mut frame) = latest_frame.try_write() {
                            *frame = Some(jpeg.clone());
                        }
                        let _ = broadcast_tx.send(jpeg);
                    }
                    Err(e) => {
                        error!("[{}] Frame capture error: {}", name, e);
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
        }
    };

    info!("[{}] Camera started: {} ({}x{}) [CLAHE enabled]", name, device, width, height);

    loop {
        match camera.frame_raw() {
            Ok(raw_bytes) => {
                let jpeg = match process_frame(&raw_bytes, flip_x, flip_y, &mut clahe) {
                    Ok(processed) => processed,
                    Err(_) => raw_bytes.to_vec(), // fallback: use raw frame
                };

                if let Ok(mut frame) = latest_frame.try_write() {
                    *frame = Some(jpeg.clone());
                }
                let _ = broadcast_tx.send(jpeg);
            }
            Err(e) => {
                error!("[{}] Frame capture error: {}", name, e);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

/// Decode JPEG, apply CLAHE normalization and optional flip, re-encode to JPEG.
fn process_frame(
    jpeg_bytes: &[u8],
    flip_x: bool,
    flip_y: bool,
    clahe: &mut opencv::core::Ptr<imgproc::CLAHE>,
) -> Result<Vec<u8>, opencv::Error> {
    // Decode JPEG → BGR Mat
    let buf = Vector::from_slice(jpeg_bytes);
    let mut bgr = imgcodecs::imdecode(&buf, imgcodecs::IMREAD_COLOR)?;

    // Flip if needed
    if flip_x && flip_y {
        let mut flipped = Mat::default();
        cv_core::flip(&bgr, &mut flipped, -1)?; // -1 = both axes
        bgr = flipped;
    } else if flip_x {
        let mut flipped = Mat::default();
        cv_core::flip(&bgr, &mut flipped, 1)?; // 1 = horizontal
        bgr = flipped;
    } else if flip_y {
        let mut flipped = Mat::default();
        cv_core::flip(&bgr, &mut flipped, 0)?; // 0 = vertical
        bgr = flipped;
    }

    // Convert BGR → LAB, apply CLAHE to L channel, convert back
    let mut lab = Mat::default();
    imgproc::cvt_color(&bgr, &mut lab, imgproc::COLOR_BGR2Lab, 0)?;

    let mut channels: Vector<Mat> = Vector::new();
    cv_core::split(&lab, &mut channels)?;

    let mut l_eq = Mat::default();
    clahe.apply(&channels.get(0)?, &mut l_eq)?;
    channels.set(0, l_eq)?;

    let mut lab_eq = Mat::default();
    cv_core::merge(&channels, &mut lab_eq)?;

    let mut result = Mat::default();
    imgproc::cvt_color(&lab_eq, &mut result, imgproc::COLOR_Lab2BGR, 0)?;

    // Encode back to JPEG
    let mut out_buf = Vector::new();
    let params = Vector::from_slice(&[imgcodecs::IMWRITE_JPEG_QUALITY, 90]);
    imgcodecs::imencode(".jpg", &result, &mut out_buf, &params)?;

    Ok(out_buf.to_vec())
}
