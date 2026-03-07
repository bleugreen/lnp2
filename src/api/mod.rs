pub mod handlers;
pub mod ws;

use axum::routing::{get, post, put};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/api/gcode", post(handlers::gcode_send))
        .route("/api/gcode/batch", post(handlers::gcode_batch))
        .route("/api/move", post(handlers::move_to))
        .route("/api/move/safe", post(handlers::move_safe))
        .route("/api/home", post(handlers::home))
        .route("/api/position", get(handlers::get_position))
        .route("/api/acceleration", post(handlers::set_acceleration))
        .route("/api/vacuum", post(handlers::vacuum_control))
        .route("/api/vacuum/read", get(handlers::vacuum_read))
        .route("/api/blow", post(handlers::blow_control))
        .route("/api/led", post(handlers::led_control))
        .route("/api/config", get(handlers::get_config))
        .route("/api/config", put(handlers::set_config))
        // Camera endpoints
        .route("/api/camera/capture", get(handlers::camera_capture))
        .route("/api/camera/list", get(handlers::camera_list))
        // WebSocket endpoints
        .route("/api/camera/stream", get(ws::camera_stream))
        .route("/api/events", get(ws::events))
        .layer(CorsLayer::permissive())
        .with_state(state);

    // Static file serving for web GUI (fallback to index.html for SPA routing)
    let static_files = ServeDir::new("web/dist")
        .append_index_html_on_directories(true)
        .fallback(ServeFile::new("web/dist/index.html"));

    api.fallback_service(static_files)
}
