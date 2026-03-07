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
