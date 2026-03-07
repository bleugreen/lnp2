use std::sync::atomic::Ordering;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::motion::NozzleId;
use crate::state::AppState;

/// Guard that sets event_bus.busy while serial commands are in flight.
struct BusyGuard<'a>(&'a crate::events::EventBus);

impl<'a> BusyGuard<'a> {
    fn new(bus: &'a crate::events::EventBus) -> Self {
        bus.busy.store(true, Ordering::Relaxed);
        Self(bus)
    }
}

impl<'a> Drop for BusyGuard<'a> {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Relaxed);
    }
}

// --- Response types ---

#[derive(Serialize)]
pub(crate) struct ErrorResponse {
    error: String,
}

fn internal_err(e: impl std::fmt::Display) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: e.to_string(),
        }),
    )
}

// --- GCode ---

#[derive(Deserialize)]
pub struct GCodeRequest {
    pub command: String,
}

pub async fn gcode_send(
    State(state): State<AppState>,
    Json(req): Json<GCodeRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let response = state.gcode.send(&req.command).await.map_err(internal_err)?;
    Ok(Json(serde_json::json!({ "response": response })))
}

#[derive(Deserialize)]
pub struct GCodeBatchRequest {
    pub commands: Vec<String>,
}

pub async fn gcode_batch(
    State(state): State<AppState>,
    Json(req): Json<GCodeBatchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let mut responses = Vec::new();
    for cmd in &req.commands {
        let response = state.gcode.send(cmd).await.map_err(internal_err)?;
        responses.push(response);
    }
    Ok(Json(serde_json::json!({ "responses": responses })))
}

// --- Motion ---

#[derive(Deserialize)]
pub struct MoveRequest {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
    pub a: Option<f64>,
    pub b: Option<f64>,
    pub feedrate: Option<f64>,
}

pub async fn move_to(
    State(state): State<AppState>,
    Json(req): Json<MoveRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let _busy = BusyGuard::new(&state.event_bus);
    state
        .motion
        .move_to(req.x, req.y, req.z, req.a, req.b, req.feedrate)
        .await
        .map_err(internal_err)?;
    state.gcode.wait().await.map_err(internal_err)?;
    // Publish final position after move completes
    let pos = state.gcode.position().await.map_err(internal_err)?;
    state.event_bus.publish(crate::events::Event::Position {
        x: pos.x, y: pos.y, z: pos.z, a: pos.a, b: pos.b,
    });
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct MoveSafeRequest {
    pub x: f64,
    pub y: f64,
}

pub async fn move_safe(
    State(state): State<AppState>,
    Json(req): Json<MoveSafeRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let _busy = BusyGuard::new(&state.event_bus);
    state
        .motion
        .move_safe(req.x, req.y)
        .await
        .map_err(internal_err)?;
    let pos = state.gcode.position().await.map_err(internal_err)?;
    state.event_bus.publish(crate::events::Event::Position {
        x: pos.x, y: pos.y, z: pos.z, a: pos.a, b: pos.b,
    });
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn home(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let _busy = BusyGuard::new(&state.event_bus);
    state.motion.home().await.map_err(internal_err)?;
    let pos = state.gcode.position().await.map_err(internal_err)?;
    state.event_bus.publish(crate::events::Event::Position {
        x: pos.x, y: pos.y, z: pos.z, a: pos.a, b: pos.b,
    });
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn get_position(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let pos = state.motion.get_position().await.map_err(internal_err)?;
    Ok(Json(serde_json::to_value(pos).unwrap()))
}

#[derive(Deserialize)]
pub struct AccelerationRequest {
    pub value: f64,
}

pub async fn set_acceleration(
    State(state): State<AppState>,
    Json(req): Json<AccelerationRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    state
        .motion
        .set_acceleration(req.value)
        .await
        .map_err(internal_err)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// --- Actuators ---

#[derive(Deserialize)]
pub struct VacuumRequest {
    pub nozzle: NozzleId,
    pub action: VacuumAction,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VacuumAction {
    On,
    Off,
}

pub async fn vacuum_control(
    State(state): State<AppState>,
    Json(req): Json<VacuumRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    match req.action {
        VacuumAction::On => state.actuators.vacuum_on(req.nozzle).await.map_err(internal_err)?,
        VacuumAction::Off => state.actuators.vacuum_off(req.nozzle).await.map_err(internal_err)?,
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct VacuumReadQuery {
    pub nozzle: NozzleId,
}

pub async fn vacuum_read(
    State(state): State<AppState>,
    Query(query): Query<VacuumReadQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let value = state
        .actuators
        .vacuum_read(query.nozzle)
        .await
        .map_err(internal_err)?;
    Ok(Json(serde_json::json!({ "nozzle": query.nozzle, "value": value })))
}

#[derive(Deserialize)]
pub struct BlowRequest {
    pub nozzle: NozzleId,
    #[serde(default = "default_blow_duration")]
    pub duration_ms: u32,
}

fn default_blow_duration() -> u32 {
    100
}

pub async fn blow_control(
    State(state): State<AppState>,
    Json(req): Json<BlowRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    state
        .actuators
        .blow(req.nozzle, req.duration_ms)
        .await
        .map_err(internal_err)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct LedRequest {
    pub r: Option<u8>,
    pub g: Option<u8>,
    pub b: Option<u8>,
    pub brightness: Option<u8>,
    #[serde(default)]
    pub off: bool,
}

pub async fn led_control(
    State(state): State<AppState>,
    Json(req): Json<LedRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if req.off {
        state.actuators.led_off().await.map_err(internal_err)?;
    } else {
        state
            .actuators
            .led_on(
                req.r.unwrap_or(255),
                req.g.unwrap_or(255),
                req.b.unwrap_or(255),
                req.brightness.unwrap_or(255),
            )
            .await
            .map_err(internal_err)?;
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

// --- Config ---

pub async fn get_config(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let config = state.config.read().await;
    Json(serde_json::to_value(&*config).unwrap())
}

pub async fn set_config(
    State(state): State<AppState>,
    Json(new_config): Json<crate::config::MachineConfig>,
) -> Json<serde_json::Value> {
    let mut config = state.config.write().await;
    *config = new_config;
    Json(serde_json::json!({ "ok": true }))
}

// --- Camera ---

#[derive(Deserialize)]
pub struct CameraCaptureQuery {
    pub name: String,
}

pub async fn camera_capture(
    State(state): State<AppState>,
    Query(query): Query<CameraCaptureQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let camera = state
        .camera
        .as_ref()
        .ok_or_else(|| internal_err("No cameras configured"))?;

    let jpeg = camera.capture(&query.name).await.map_err(internal_err)?;

    Ok(([(axum::http::header::CONTENT_TYPE, "image/jpeg")], jpeg))
}

pub async fn camera_list(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    match &state.camera {
        Some(camera) => {
            let configs = camera.configs();
            Json(serde_json::json!({
                "cameras": configs.keys().collect::<Vec<_>>(),
                "configs": configs
            }))
        }
        None => Json(serde_json::json!({ "cameras": [], "configs": {} })),
    }
}

// --- Dataset Capture ---

#[derive(Deserialize)]
pub struct CaptureRequest {
    pub camera: String,
    /// Optional label for this capture (e.g. "pocket", "sprocket", "fiducial")
    #[serde(default)]
    pub label: Option<String>,
}

pub async fn dataset_capture(
    State(state): State<AppState>,
    Json(req): Json<CaptureRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let camera = state.camera.as_ref().ok_or_else(|| internal_err("No cameras configured"))?;
    let jpeg = camera.capture(&req.camera).await.map_err(internal_err)?;

    // Get current machine position for metadata
    let pos = state.motion.get_position().await.ok();

    // Build filename: {timestamp}_{camera}_{label}_{x}_{y}.jpg
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let label = req.label.as_deref().unwrap_or("unlabeled");
    let pos_str = match pos {
        Some(ref p) => format!("x{:.1}_y{:.1}", p.x, p.y),
        None => "nopos".to_string(),
    };
    let filename = format!("{}_{}_{}_{}_.jpg", ts, req.camera, label, pos_str);

    // Ensure dataset directory exists
    let dir = std::path::Path::new("dataset/images");
    std::fs::create_dir_all(dir).map_err(internal_err)?;

    let path = dir.join(&filename);
    std::fs::write(&path, &jpeg).map_err(internal_err)?;

    // Count total images
    let count = std::fs::read_dir(dir)
        .map(|d| d.filter(|e| e.is_ok()).count())
        .unwrap_or(0);

    Ok(Json(serde_json::json!({
        "filename": filename,
        "count": count,
        "position": pos,
    })))
}

pub async fn dataset_count(
    State(_state): State<AppState>,
) -> Json<serde_json::Value> {
    let dir = std::path::Path::new("dataset/images");
    let count = std::fs::read_dir(dir)
        .map(|d| d.filter(|e| e.is_ok()).count())
        .unwrap_or(0);
    Json(serde_json::json!({ "count": count }))
}

// --- Vision ---

#[derive(Deserialize)]
pub struct VisionDetectAllRequest {
    pub camera: String,
    #[serde(default = "default_detect_all_conf")]
    pub confidence_threshold: f64,
}

fn default_detect_all_conf() -> f64 { 0.3 }

pub async fn vision_detect_all(
    State(state): State<AppState>,
    Json(req): Json<VisionDetectAllRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let camera = state.camera.as_ref().ok_or_else(|| internal_err("No cameras configured"))?;
    let jpeg = camera.capture(&req.camera).await.map_err(internal_err)?;

    let vision = state.vision.clone();
    let camera_name = req.camera.clone();
    let conf = req.confidence_threshold;

    let config_guard = state.config.read().await;
    let cam_config = config_guard
        .cameras
        .get(&req.camera)
        .ok_or_else(|| internal_err(format!("Camera '{}' not found", req.camera)))?;
    let class_names: Vec<String> = cam_config
        .vision
        .as_ref()
        .and_then(|v| v.class_names.clone())
        .unwrap_or_default();
    drop(config_guard);

    let result = tokio::task::spawn_blocking(move || {
        let session = vision.as_ref().and_then(|v| v.session_for(&camera_name));
        let session = session.ok_or_else(|| crate::vision::VisionError::Other("No model loaded".into()))?;
        let image = crate::vision::cv::decode_frame(&jpeg)?;
        let mut ctx = crate::vision::context::VisionContext::new(&crate::vision::types::VisionConfig::default());
        crate::vision::ml::detect_ml(&image, session, conf, &mut ctx)
    })
    .await
    .map_err(|e| internal_err(e))?
    .map_err(internal_err)?;

    let detections: Vec<serde_json::Value> = result.iter().map(|b| {
        let class_name = class_names.get(b.class_id)
            .cloned()
            .unwrap_or_else(|| format!("class_{}", b.class_id));
        serde_json::json!({
            "class_id": b.class_id,
            "class_name": class_name,
            "confidence": b.confidence,
            "x": b.x,
            "y": b.y,
            "width": b.width,
            "height": b.height,
        })
    }).collect();

    Ok(Json(serde_json::json!({ "detections": detections })))
}

#[derive(Deserialize)]
pub struct VisionDetectPocketRequest {
    pub camera: String,
    pub expected_width_mm: f64,
    pub expected_height_mm: f64,
}

pub async fn vision_detect_pocket(
    State(state): State<AppState>,
    Json(req): Json<VisionDetectPocketRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let camera = state.camera.as_ref().ok_or_else(|| internal_err("No cameras configured"))?;
    let jpeg = camera.capture(&req.camera).await.map_err(internal_err)?;

    let config_guard = state.config.read().await;
    let cam_config = config_guard
        .cameras
        .get(&req.camera)
        .ok_or_else(|| internal_err(format!("Camera '{}' not found", req.camera)))?;

    let cal = crate::vision::CameraCalibration::from(cam_config);
    let vision_config = cam_config
        .vision
        .clone()
        .unwrap_or_default();
    let expected = (req.expected_width_mm, req.expected_height_mm);

    let vision = state.vision.clone();
    let camera_name = req.camera.clone();

    let result = tokio::task::spawn_blocking(move || {
        let session = vision.as_ref().and_then(|v| v.session_for(&camera_name));
        crate::vision::detect_pocket(&jpeg, expected, &cal, &vision_config, session)
    })
    .await
    .map_err(|e| internal_err(e))?
    .map_err(internal_err)?;

    Ok(Json(serde_json::json!({ "detection": result })))
}

#[derive(Deserialize)]
pub struct VisionDetectFiducialRequest {
    pub camera: String,
    #[serde(default = "default_fid_min")]
    pub min_diameter_mm: f64,
    #[serde(default = "default_fid_max")]
    pub max_diameter_mm: f64,
}

fn default_fid_min() -> f64 { 0.5 }
fn default_fid_max() -> f64 { 2.0 }

pub async fn vision_detect_fiducial(
    State(state): State<AppState>,
    Json(req): Json<VisionDetectFiducialRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let camera = state.camera.as_ref().ok_or_else(|| internal_err("No cameras configured"))?;
    let jpeg = camera.capture(&req.camera).await.map_err(internal_err)?;

    let config_guard = state.config.read().await;
    let cam_config = config_guard
        .cameras
        .get(&req.camera)
        .ok_or_else(|| internal_err(format!("Camera '{}' not found", req.camera)))?;

    let cal = crate::vision::CameraCalibration::from(cam_config);
    let min_d = req.min_diameter_mm;
    let max_d = req.max_diameter_mm;

    let result = tokio::task::spawn_blocking(move || {
        crate::vision::detect_fiducial(&jpeg, &cal, min_d, max_d, None)
    })
    .await
    .map_err(|e| internal_err(e))?
    .map_err(internal_err)?;

    Ok(Json(serde_json::json!({ "detection": result })))
}

#[derive(Deserialize)]
pub struct VisionAlignPartRequest {
    pub camera: String,
    pub part_id: String,
}

pub async fn vision_align_part(
    State(state): State<AppState>,
    Json(req): Json<VisionAlignPartRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let camera = state.camera.as_ref().ok_or_else(|| internal_err("No cameras configured"))?;
    let jpeg = camera.capture(&req.camera).await.map_err(internal_err)?;

    // Look up part → package
    let full_config = state.full_config.read().await;
    let part = full_config
        .parts
        .get(&req.part_id)
        .ok_or_else(|| internal_err(format!("Part '{}' not found", req.part_id)))?;
    let package = full_config
        .packages
        .get(&part.package_id)
        .ok_or_else(|| internal_err(format!("Package '{}' not found", part.package_id)))?
        .clone();

    let config_guard = state.config.read().await;
    let cam_config = config_guard
        .cameras
        .get(&req.camera)
        .ok_or_else(|| internal_err(format!("Camera '{}' not found", req.camera)))?;

    let cal = crate::vision::CameraCalibration::from(cam_config);
    let vision_config = cam_config.vision.clone().unwrap_or_default();
    let vision = state.vision.clone();
    let camera_name = req.camera.clone();

    drop(config_guard);
    drop(full_config);

    let result = tokio::task::spawn_blocking(move || {
        let session = vision.as_ref().and_then(|v| v.session_for(&camera_name));
        crate::vision::align_part(&jpeg, &package, &cal, &vision_config, session)
    })
    .await
    .map_err(|e| internal_err(e))?
    .map_err(internal_err)?;

    Ok(Json(serde_json::json!(result)))
}
