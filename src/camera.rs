use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
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
    config: Arc<RwLock<CameraConfig>>,
    latest_frame: Arc<RwLock<Option<Bytes>>>,
    broadcast_tx: broadcast::Sender<Bytes>,
    _task: JoinHandle<()>,
}

impl CameraManager {
    pub fn start(cameras: &HashMap<String, CameraConfig>) -> Self {
        let mut handles = HashMap::new();

        for (name, config) in cameras {
            let (broadcast_tx, _) = broadcast::channel(4); // small buffer, drop old frames
            let latest_frame: Arc<RwLock<Option<Bytes>>> = Arc::new(RwLock::new(None));

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
                    config: Arc::new(RwLock::new(config.clone())),
                    latest_frame,
                    broadcast_tx,
                    _task: task,
                },
            );
        }

        Self { cameras: handles }
    }

    /// Get latest captured frame (JPEG bytes).
    pub async fn capture(&self, name: &str) -> Result<Bytes, CameraError> {
        let handle = self.cameras.get(name).ok_or_else(|| CameraError::NotFound(name.to_string()))?;
        let frame = handle.latest_frame.read().await;
        frame.clone().ok_or(CameraError::NoFrame)
    }

    /// Subscribe to live frame stream.
    pub fn subscribe(&self, name: &str) -> Result<broadcast::Receiver<Bytes>, CameraError> {
        let handle = self.cameras.get(name).ok_or_else(|| CameraError::NotFound(name.to_string()))?;
        Ok(handle.broadcast_tx.subscribe())
    }

    /// List available camera names.
    pub fn list(&self) -> Vec<String> {
        self.cameras.keys().cloned().collect()
    }

    /// Get camera configs for the list endpoint.
    pub async fn configs(&self) -> HashMap<String, CameraConfig> {
        let mut result = HashMap::new();
        for (name, handle) in &self.cameras {
            let config = handle.config.read().await;
            result.insert(name.clone(), config.clone());
        }
        result
    }

    /// Update a camera's config at runtime (e.g. calibration changes).
    /// UPP changes take effect immediately for API consumers.
    /// Flip changes are stored but require restart to affect the capture loop.
    pub async fn update_config(&self, name: &str, new_config: CameraConfig) -> Result<(), CameraError> {
        let handle = self.cameras.get(name).ok_or_else(|| CameraError::NotFound(name.to_string()))?;
        let mut config = handle.config.write().await;
        *config = new_config;
        Ok(())
    }
}

fn capture_loop(
    name: &str,
    device: &str,
    width: u32,
    height: u32,
    flip_x: bool,
    flip_y: bool,
    latest_frame: Arc<RwLock<Option<Bytes>>>,
    broadcast_tx: broadcast::Sender<Bytes>,
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

    // Auto-exposure state
    let mut auto_exp = AutoExposure::new(device);
    if auto_exp.available {
        info!("[{}] Camera started: {} ({}x{}) [auto-exposure enabled, initial={}]",
            name, device, width, height, auto_exp.current_exposure);
    } else {
        info!("[{}] Camera started: {} ({}x{}) [auto-exposure unavailable]",
            name, device, width, height);
    }

    let mut frame_count: u32 = 0;

    loop {
        match camera.frame_raw() {
            Ok(raw_bytes) => {
                let jpeg_vec = if flip_x || flip_y {
                    flip_jpeg(&raw_bytes, flip_x, flip_y)
                } else {
                    raw_bytes.to_vec()
                };

                // Adjust exposure every 5 frames (~3/sec at 15fps)
                if auto_exp.available {
                    frame_count += 1;
                    if frame_count % 5 == 0 {
                        auto_exp.adjust(&jpeg_vec, name);
                    }
                }

                // Bytes: Arc-backed, cheap clone for broadcast + latest_frame
                let jpeg = Bytes::from(jpeg_vec);
                let _ = broadcast_tx.send(jpeg.clone());
                if let Ok(mut frame) = latest_frame.try_write() {
                    *frame = Some(jpeg);
                }
            }
            Err(e) => {
                error!("[{}] Frame capture error: {}", name, e);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

/// Automatic exposure control via v4l2.
/// Measures mean brightness of the center region of each frame and nudges
/// exposure_time_absolute up/down to keep it in a target range.
struct AutoExposure {
    device: String,
    available: bool,
    current_exposure: i32,
    min_exposure: i32,
    max_exposure: i32,
}

impl AutoExposure {
    fn new(device: &str) -> Self {
        // Try to read current exposure and set manual mode
        let mut ae = Self {
            device: device.to_string(),
            available: false,
            current_exposure: 120,
            min_exposure: 1,
            max_exposure: 5000,
        };

        // Ensure manual exposure mode (value=1)
        let _ = std::process::Command::new("v4l2-ctl")
            .args(["-d", device, "-c", "auto_exposure=1"])
            .output();

        // Read current exposure
        if let Ok(output) = std::process::Command::new("v4l2-ctl")
            .args(["-d", device, "-C", "exposure_time_absolute"])
            .output()
        {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout);
                if let Some(val) = s.split(':').nth(1) {
                    if let Ok(v) = val.trim().parse::<i32>() {
                        ae.current_exposure = v;
                        ae.available = true;
                    }
                }
            }
        }

        ae
    }

    fn adjust(&mut self, jpeg_bytes: &[u8], cam_name: &str) {
        let mean = match mean_brightness_center(jpeg_bytes) {
            Some(v) => v,
            None => return,
        };

        // Target: mean brightness 90-150 (out of 255) for center region
        const TARGET_LOW: f64 = 90.0;
        const TARGET_HIGH: f64 = 150.0;

        if mean >= TARGET_LOW && mean <= TARGET_HIGH {
            return; // exposure is fine
        }

        // Proportional adjustment: bigger error → bigger step
        // Compute target exposure directly: scale by (target / actual)
        let target_mean = (TARGET_LOW + TARGET_HIGH) / 2.0; // 120
        let ratio = target_mean / mean.max(1.0);
        // Clamp ratio to avoid wild swings from transient frames
        let ratio = ratio.clamp(0.1, 4.0);
        let new_exposure = ((self.current_exposure as f64 * ratio) as i32)
            .clamp(self.min_exposure, self.max_exposure);

        if new_exposure == self.current_exposure {
            return;
        }

        let cmd = format!("exposure_time_absolute={}", new_exposure);
        if let Ok(output) = std::process::Command::new("v4l2-ctl")
            .args(["-d", &self.device, "-c", &cmd])
            .output()
        {
            if output.status.success() {
                info!("[{}] Exposure: {} → {} (mean brightness: {:.0})",
                    cam_name, self.current_exposure, new_exposure, mean);
                self.current_exposure = new_exposure;
            }
        }
    }
}

/// Compute mean brightness of the center 50% of a JPEG image.
/// Uses the `image` crate for decoding — fast enough for periodic checks.
fn mean_brightness_center(jpeg_bytes: &[u8]) -> Option<f64> {
    use image::ImageFormat;

    let img = image::load_from_memory_with_format(jpeg_bytes, ImageFormat::Jpeg).ok()?;
    let gray = img.to_luma8();
    let (w, h) = gray.dimensions();

    // Sample center 50% of the image
    let x0 = w / 4;
    let x1 = 3 * w / 4;
    let y0 = h / 4;
    let y1 = 3 * h / 4;

    let mut sum: u64 = 0;
    let mut count: u64 = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            sum += gray.get_pixel(x, y).0[0] as u64;
            count += 1;
        }
    }

    if count == 0 { return None; }
    Some(sum as f64 / count as f64)
}

/// Lossless JPEG flip via turbojpeg (operates in DCT domain, no pixel decode/re-encode).
/// Falls back to pixel-based flip if turbojpeg transform fails.
fn flip_jpeg(jpeg_bytes: &[u8], flip_x: bool, flip_y: bool) -> Vec<u8> {
    let op = match (flip_x, flip_y) {
        (true, true) => turbojpeg::TransformOp::Rot180,
        (true, false) => turbojpeg::TransformOp::Hflip,
        (false, true) => turbojpeg::TransformOp::Vflip,
        (false, false) => return jpeg_bytes.to_vec(),
    };

    let transform = turbojpeg::Transform::op(op);
    match turbojpeg::transform(&transform, jpeg_bytes) {
        Ok(buf) => buf.to_vec(),
        Err(e) => {
            warn!("turbojpeg transform failed, falling back to pixel flip: {}", e);
            flip_jpeg_fallback(jpeg_bytes, flip_x, flip_y)
        }
    }
}

/// Fallback pixel-based JPEG flip (decode → transform → re-encode).
fn flip_jpeg_fallback(jpeg_bytes: &[u8], flip_x: bool, flip_y: bool) -> Vec<u8> {
    use image::ImageFormat;
    use std::io::Cursor;

    let img = match image::load_from_memory_with_format(jpeg_bytes, ImageFormat::Jpeg) {
        Ok(img) => img,
        Err(_) => return jpeg_bytes.to_vec(),
    };

    let img = if flip_x && flip_y {
        img.rotate180()
    } else if flip_x {
        img.fliph()
    } else {
        img.flipv()
    };

    let mut buf = Cursor::new(Vec::new());
    if img.write_to(&mut buf, ImageFormat::Jpeg).is_ok() {
        buf.into_inner()
    } else {
        jpeg_bytes.to_vec()
    }
}
