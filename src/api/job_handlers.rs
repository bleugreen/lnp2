use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::config::boards::BoardConfig;
use crate::config::jobs::{BoardOrigin, JobBoard, JobConfig};
use crate::job::{self, JobControl};
use crate::state::AppState;

use super::handlers::{ErrorResponse, internal_err};

// --- Board Import ---

#[derive(Deserialize)]
pub struct ImportBoardRequest {
    pub file_path: String,
    pub name: String,
}

pub async fn board_import(
    State(state): State<AppState>,
    Json(req): Json<ImportBoardRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let path = std::path::Path::new(&req.file_path);
    let board = crate::import::import_board(path, &req.name).map_err(internal_err)?;

    // Save to config/boards/<name>.toml
    let boards_dir = state.config_dir.join("boards");
    std::fs::create_dir_all(&boards_dir).map_err(internal_err)?;

    let toml = toml::to_string_pretty(&board).map_err(internal_err)?;
    let board_path = boards_dir.join(format!("{}.toml", req.name));
    std::fs::write(&board_path, &toml).map_err(internal_err)?;

    // Update in-memory config
    {
        let mut full_config = state.full_config.write().await;
        full_config.boards.insert(req.name.clone(), board.clone());
    }

    Ok(Json(serde_json::json!({
        "name": board.name,
        "placements": board.placements.len(),
        "fiducials": board.fiducials.len(),
        "path": board_path.display().to_string(),
    })))
}

// --- Board CRUD ---

pub async fn board_list(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let full_config = state.full_config.read().await;
    let boards: Vec<serde_json::Value> = full_config
        .boards
        .values()
        .map(|b| {
            serde_json::json!({
                "name": b.name,
                "placements": b.placements.len(),
                "fiducials": b.fiducials.len(),
                "source_file": b.source_file,
            })
        })
        .collect();
    Json(serde_json::json!({ "boards": boards }))
}

pub async fn board_get(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let full_config = state.full_config.read().await;
    let board = full_config
        .boards
        .get(&name)
        .ok_or_else(|| internal_err(format!("Board '{}' not found", name)))?;
    Ok(Json(serde_json::to_value(board).map_err(internal_err)?))
}

pub async fn board_update(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(board): Json<BoardConfig>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    // Save to disk
    let boards_dir = state.config_dir.join("boards");
    std::fs::create_dir_all(&boards_dir).map_err(internal_err)?;
    let toml = toml::to_string_pretty(&board).map_err(internal_err)?;
    std::fs::write(boards_dir.join(format!("{}.toml", name)), &toml).map_err(internal_err)?;

    // Update in-memory
    let mut full_config = state.full_config.write().await;
    full_config.boards.insert(name, board);

    Ok(Json(serde_json::json!({ "ok": true })))
}

// --- Job CRUD ---

pub async fn job_list(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let full_config = state.full_config.read().await;
    let jobs: Vec<serde_json::Value> = full_config
        .jobs
        .values()
        .map(|j| {
            serde_json::json!({
                "name": j.name,
                "boards": j.boards.len(),
            })
        })
        .collect();
    Json(serde_json::json!({ "jobs": jobs }))
}

pub async fn job_get(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let full_config = state.full_config.read().await;
    let job = full_config
        .jobs
        .get(&name)
        .ok_or_else(|| internal_err(format!("Job '{}' not found", name)))?;
    Ok(Json(serde_json::to_value(job).map_err(internal_err)?))
}

pub async fn job_create(
    State(state): State<AppState>,
    Json(job): Json<JobConfig>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    // Save to disk
    let jobs_dir = state.config_dir.join("jobs");
    std::fs::create_dir_all(&jobs_dir).map_err(internal_err)?;
    let toml = toml::to_string_pretty(&job).map_err(internal_err)?;
    std::fs::write(jobs_dir.join(format!("{}.toml", job.name)), &toml).map_err(internal_err)?;

    let name = job.name.clone();
    // Update in-memory
    let mut full_config = state.full_config.write().await;
    full_config.jobs.insert(name.clone(), job);

    Ok(Json(serde_json::json!({ "ok": true, "name": name })))
}

pub async fn job_update(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(job): Json<JobConfig>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let jobs_dir = state.config_dir.join("jobs");
    std::fs::create_dir_all(&jobs_dir).map_err(internal_err)?;
    let toml = toml::to_string_pretty(&job).map_err(internal_err)?;
    std::fs::write(jobs_dir.join(format!("{}.toml", name)), &toml).map_err(internal_err)?;

    let mut full_config = state.full_config.write().await;
    full_config.jobs.insert(name, job);

    Ok(Json(serde_json::json!({ "ok": true })))
}

// --- Job Control ---

#[derive(Deserialize)]
pub struct StartJobRequest {
    pub job_name: String,
}

pub async fn job_start(
    State(state): State<AppState>,
    Json(req): Json<StartJobRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    // Check no job already running
    {
        let active = state.active_job.read().await;
        if active.is_some() {
            return Err(internal_err("A job is already running"));
        }
    }

    let full_config = state.full_config.read().await;

    // Look up job config
    let job_config = full_config
        .jobs
        .get(&req.job_name)
        .ok_or_else(|| internal_err(format!("Job '{}' not found", req.job_name)))?
        .clone();

    // Resolve board configs
    let mut board_configs = Vec::new();
    for job_board in &job_config.boards {
        let board = full_config
            .boards
            .get(&job_board.board_id)
            .ok_or_else(|| {
                internal_err(format!("Board '{}' not found", job_board.board_id))
            })?
            .clone();
        board_configs.push(board);
    }
    drop(full_config);

    // Start the job
    let handle = job::start_job(job_config, board_configs, state.clone());

    // Store the handle
    {
        let mut active = state.active_job.write().await;
        *active = Some(handle);
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn job_pause(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let active = state.active_job.read().await;
    let handle = active.as_ref().ok_or_else(|| internal_err("No active job"))?;
    handle
        .control_tx
        .send(JobControl::Pause)
        .await
        .map_err(internal_err)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn job_resume(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let active = state.active_job.read().await;
    let handle = active.as_ref().ok_or_else(|| internal_err("No active job"))?;
    handle
        .control_tx
        .send(JobControl::Resume)
        .await
        .map_err(internal_err)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn job_abort(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let active = state.active_job.read().await;
    let handle = active.as_ref().ok_or_else(|| internal_err("No active job"))?;
    handle
        .control_tx
        .send(JobControl::Abort)
        .await
        .map_err(internal_err)?;
    drop(active);

    // Clear the active job
    let mut active = state.active_job.write().await;
    *active = None;

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn job_status(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let active = state.active_job.read().await;
    match active.as_ref() {
        Some(handle) => {
            let job_state = handle.state.read().await;
            let mut stats = job_state.stats.clone();
            stats.update_elapsed();

            Ok(Json(serde_json::json!({
                "active": true,
                "job_name": job_state.config.name,
                "status": job_state.status,
                "current_step": job_state.current_step,
                "stats": stats,
            })))
        }
        None => Ok(Json(serde_json::json!({
            "active": false,
        }))),
    }
}

#[derive(Deserialize)]
pub struct SkipPlacementRequest {
    pub board_idx: usize,
    pub placement_idx: usize,
}

pub async fn job_skip(
    State(state): State<AppState>,
    Json(req): Json<SkipPlacementRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let active = state.active_job.read().await;
    let handle = active.as_ref().ok_or_else(|| internal_err("No active job"))?;
    handle
        .control_tx
        .send(JobControl::SkipPlacement {
            board_idx: req.board_idx,
            placement_idx: req.placement_idx,
        })
        .await
        .map_err(internal_err)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
