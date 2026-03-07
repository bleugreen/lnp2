use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use ndarray::{Array4, ArrayView2, Axis};
use opencv::core::{Mat, Size};
use opencv::imgproc;
use tracing::debug;

use super::context::VisionContext;
use super::error::VisionError;

/// Bounding box from YOLO postprocessing.
#[derive(Debug, Clone)]
pub struct BoundingBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub confidence: f64,
    pub class_id: usize,
    pub angle: Option<f64>,
}

/// Loads and caches ONNX models. Thread-safe.
pub struct ModelManager {
    sessions: RwLock<HashMap<String, Arc<ort::Session>>>,
}

impl ModelManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Load a model from disk. Caches by path.
    pub fn load(&self, path: &str) -> Result<Arc<ort::Session>, VisionError> {
        // Check cache
        {
            let cache = self.sessions.read().map_err(|e| VisionError::Other(e.to_string()))?;
            if let Some(session) = cache.get(path) {
                return Ok(session.clone());
            }
        }

        // Load and cache
        let session = ort::Session::builder()?
            .with_optimization_level(ort::GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?
            .commit_from_file(path)?;

        let session = Arc::new(session);
        {
            let mut cache = self.sessions.write().map_err(|e| VisionError::Other(e.to_string()))?;
            cache.insert(path.to_string(), session.clone());
        }
        Ok(session)
    }
}

/// Preprocess a Mat for YOLOv8 inference.
/// - Letterbox resize to 640x640
/// - BGR → RGB
/// - Normalize to 0.0–1.0
/// - Reshape to [1, 3, 640, 640]
pub fn yolo_preprocess(image: &Mat) -> Result<(Array4<f32>, f64, f64, f64), VisionError> {
    let orig_h = image.rows() as f64;
    let orig_w = image.cols() as f64;
    let target = 640.0;

    // Compute letterbox scale
    let scale = (target / orig_w).min(target / orig_h);
    let new_w = (orig_w * scale).round() as i32;
    let new_h = (orig_h * scale).round() as i32;

    // Resize
    let mut resized = Mat::default();
    imgproc::resize(image, &mut resized, Size::new(new_w, new_h), 0.0, 0.0, imgproc::INTER_LINEAR)?;

    // Create padded 640x640 image (fill with 114 gray)
    let mut padded = Mat::new_rows_cols_with_default(640, 640, opencv::core::CV_8UC3, opencv::core::Scalar::all(114.0))?;

    let pad_x = ((target as i32 - new_w) / 2) as f64;
    let pad_y = ((target as i32 - new_h) / 2) as f64;

    // Copy resized into padded
    let roi = opencv::core::Rect::new(pad_x as i32, pad_y as i32, new_w, new_h);
    let mut roi_mat = Mat::roi(&padded, roi)?;
    resized.copy_to(&mut roi_mat)?;

    // BGR → RGB
    let mut rgb = Mat::default();
    imgproc::cvt_color(&padded, &mut rgb, imgproc::COLOR_BGR2RGB, 0)?;

    // Mat to Array4<f32> [1, 3, 640, 640] normalized
    let mut array = Array4::<f32>::zeros((1, 3, 640, 640));
    let data = rgb.data_bytes()?;
    for y in 0..640 {
        for x in 0..640 {
            let idx = (y * 640 + x) * 3;
            if idx + 2 < data.len() {
                array[[0, 0, y, x]] = data[idx] as f32 / 255.0;     // R
                array[[0, 1, y, x]] = data[idx + 1] as f32 / 255.0; // G
                array[[0, 2, y, x]] = data[idx + 2] as f32 / 255.0; // B
            }
        }
    }

    Ok((array, scale, pad_x, pad_y))
}

/// Postprocess YOLOv8 output tensor.
/// Standard YOLOv8 output shape: [1, (4 + num_classes), 8400]
/// - Apply confidence threshold
/// - NMS
/// - Scale boxes back to original image coordinates
pub fn yolo_postprocess(
    output: &ArrayView2<f32>,
    original_size: (u32, u32),
    scale: f64,
    pad_x: f64,
    pad_y: f64,
    conf_threshold: f64,
    nms_threshold: f64,
) -> Vec<BoundingBox> {
    let num_predictions = output.shape()[1]; // 8400
    let num_attrs = output.shape()[0];       // 4 + num_classes

    if num_attrs < 5 {
        return Vec::new();
    }

    let mut candidates: Vec<BoundingBox> = Vec::new();

    for i in 0..num_predictions {
        // Find best class
        let mut best_class = 0;
        let mut best_conf: f32 = 0.0;
        for c in 4..num_attrs {
            let conf = output[[c, i]];
            if conf > best_conf {
                best_conf = conf;
                best_class = c - 4;
            }
        }

        if (best_conf as f64) < conf_threshold {
            continue;
        }

        // Extract box (cx, cy, w, h in letterbox coordinates)
        let cx = output[[0, i]] as f64;
        let cy = output[[1, i]] as f64;
        let w = output[[2, i]] as f64;
        let h = output[[3, i]] as f64;

        // Convert from letterbox to original image coordinates
        let orig_cx = (cx - pad_x) / scale;
        let orig_cy = (cy - pad_y) / scale;
        let orig_w = w / scale;
        let orig_h = h / scale;

        candidates.push(BoundingBox {
            x: orig_cx,
            y: orig_cy,
            width: orig_w,
            height: orig_h,
            confidence: best_conf as f64,
            class_id: best_class,
            angle: None,
        });
    }

    // Sort by confidence descending
    candidates.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));

    // NMS
    nms(&mut candidates, nms_threshold);

    candidates
}

fn nms(boxes: &mut Vec<BoundingBox>, threshold: f64) {
    let mut keep = vec![true; boxes.len()];

    for i in 0..boxes.len() {
        if !keep[i] {
            continue;
        }
        for j in (i + 1)..boxes.len() {
            if !keep[j] {
                continue;
            }
            if iou(&boxes[i], &boxes[j]) > threshold {
                keep[j] = false;
            }
        }
    }

    let mut idx = 0;
    boxes.retain(|_| {
        let k = keep[idx];
        idx += 1;
        k
    });
}

fn iou(a: &BoundingBox, b: &BoundingBox) -> f64 {
    let a_x1 = a.x - a.width / 2.0;
    let a_y1 = a.y - a.height / 2.0;
    let a_x2 = a.x + a.width / 2.0;
    let a_y2 = a.y + a.height / 2.0;

    let b_x1 = b.x - b.width / 2.0;
    let b_y1 = b.y - b.height / 2.0;
    let b_x2 = b.x + b.width / 2.0;
    let b_y2 = b.y + b.height / 2.0;

    let inter_x1 = a_x1.max(b_x1);
    let inter_y1 = a_y1.max(b_y1);
    let inter_x2 = a_x2.min(b_x2);
    let inter_y2 = a_y2.min(b_y2);

    let inter_area = (inter_x2 - inter_x1).max(0.0) * (inter_y2 - inter_y1).max(0.0);
    let a_area = a.width * a.height;
    let b_area = b.width * b.height;

    inter_area / (a_area + b_area - inter_area + 1e-10)
}

/// Run ONNX model inference on a frame.
pub fn detect_ml(
    image: &Mat,
    session: &ort::Session,
    conf_threshold: f64,
    ctx: &mut VisionContext,
) -> Result<Vec<BoundingBox>, VisionError> {
    let original_size = (image.cols() as u32, image.rows() as u32);
    let (input, scale, pad_x, pad_y) = yolo_preprocess(image)?;

    ctx.log(format!(
        "ML: preprocessed {}x{} → 640x640 (scale={:.3}, pad=({:.0},{:.0}))",
        original_size.0, original_size.1, scale, pad_x, pad_y
    ));

    let outputs = session.run(ort::inputs!["images" => input.view()]?)?;

    // YOLOv8 output: [1, num_attrs, 8400]
    let output_tensor = outputs[0].try_extract_tensor::<f32>()?;
    let output_view = output_tensor.view();

    // Remove batch dimension: [num_attrs, 8400]
    let squeezed = output_view.index_axis(Axis(0), 0);

    let boxes = yolo_postprocess(
        &squeezed,
        original_size,
        scale,
        pad_x,
        pad_y,
        conf_threshold,
        0.45,
    );

    ctx.log(format!("ML: {} detections above {}", boxes.len(), conf_threshold));
    debug!("ML inference: {} detections", boxes.len());

    Ok(boxes)
}
