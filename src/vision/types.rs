use serde::{Deserialize, Serialize};

use crate::config::CameraConfig;

/// Returned by all detection functions.
#[derive(Debug, Clone, Serialize)]
pub struct Detection {
    /// Offset from camera center in mm (for machine correction).
    pub offset_x_mm: f64,
    pub offset_y_mm: f64,
    pub rotation_deg: f64,
    pub confidence: f64,
    pub method: DetectionMethod,
    /// Detected region in pixel coordinates (for UI overlay drawing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region_px: Option<RegionPx>,
}

/// Pixel-space bounding region for drawing overlays.
#[derive(Debug, Clone, Serialize)]
pub struct RegionPx {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub rotation_deg: f64,
}

#[derive(Debug, Clone, Serialize)]
pub enum DetectionMethod {
    AdaptiveContour,
    MinAreaRect,
    CircularSymmetry,
    TemplateMatch,
    PadAlignment { pad_count: usize, agreement: f64 },
    OnnxModel { model_name: String },
}

/// Extended result for part alignment (bottom vision).
#[derive(Debug, Clone, Serialize)]
pub struct AlignmentResult {
    pub detection: Detection,
    pub pad_detections: Vec<PadDetection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PadDetection {
    pub pad_name: String,
    pub detected_x_mm: f64,
    pub detected_y_mm: f64,
    pub expected_x_mm: f64,
    pub expected_y_mm: f64,
    pub error_mm: f64,
}

/// Camera calibration extracted from CameraConfig.
#[derive(Debug, Clone)]
pub struct CameraCalibration {
    pub upp_x: f64,
    pub upp_y: f64,
    pub width: u32,
    pub height: u32,
}

impl From<&CameraConfig> for CameraCalibration {
    fn from(c: &CameraConfig) -> Self {
        Self {
            upp_x: c.upp_x,
            upp_y: c.upp_y,
            width: c.width,
            height: c.height,
        }
    }
}

/// Per-camera vision parameters.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VisionConfig {
    #[serde(default = "default_block_size")]
    pub adaptive_block_size: i32,
    #[serde(default = "default_c")]
    pub adaptive_c: f64,
    #[serde(default = "default_blur_ksize")]
    pub blur_ksize: i32,
    #[serde(default)]
    pub mask_diameter_px: Option<i32>,
    #[serde(default)]
    pub hsv_filter: Option<HsvRange>,
    #[serde(default = "default_contour_area_min")]
    pub contour_area_min_mm2: f64,
    #[serde(default = "default_contour_area_max")]
    pub contour_area_max_mm2: f64,

    #[serde(default)]
    pub model_path: Option<String>,
    #[serde(default = "default_confidence")]
    pub confidence_threshold: f64,
    #[serde(default = "default_true")]
    pub prefer_ml: bool,

    #[serde(default)]
    pub save_diagnostics: bool,
    #[serde(default)]
    pub diagnostics_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HsvRange {
    pub h_min: f64,
    pub h_max: f64,
    pub s_min: f64,
    pub s_max: f64,
    pub v_min: f64,
    pub v_max: f64,
}

impl Default for VisionConfig {
    fn default() -> Self {
        Self {
            adaptive_block_size: 11,
            adaptive_c: 2.0,
            blur_ksize: 9,
            mask_diameter_px: None,
            hsv_filter: None,
            contour_area_min_mm2: 0.01,
            contour_area_max_mm2: 900_000.0,
            model_path: None,
            confidence_threshold: 0.5,
            prefer_ml: true,
            save_diagnostics: false,
            diagnostics_dir: None,
        }
    }
}

fn default_block_size() -> i32 { 11 }
fn default_c() -> f64 { 2.0 }
fn default_blur_ksize() -> i32 { 9 }
fn default_contour_area_min() -> f64 { 0.01 }
fn default_contour_area_max() -> f64 { 900_000.0 }
fn default_confidence() -> f64 { 0.5 }
fn default_true() -> bool { true }
