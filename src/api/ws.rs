use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use serde::Deserialize;
use tracing::debug;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct CameraStreamQuery {
    pub name: String,
}

pub async fn camera_stream(
    ws: WebSocketUpgrade,
    Query(params): Query<CameraStreamQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_camera_stream(socket, state, params.name))
}

async fn handle_camera_stream(mut socket: WebSocket, state: AppState, name: String) {
    let camera = match &state.camera {
        Some(cam) => cam,
        None => {
            debug!("Camera stream requested but no cameras available");
            return;
        }
    };

    let mut rx = match camera.subscribe(&name) {
        Ok(rx) => rx,
        Err(e) => {
            debug!("Camera subscribe failed: {}", e);
            return;
        }
    };

    while let Ok(frame) = rx.recv().await {
        if socket.send(Message::Binary(frame.into())).await.is_err() {
            break;
        }
    }
}

pub async fn events(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_events(socket, state))
}

async fn handle_events(mut socket: WebSocket, state: AppState) {
    let mut rx = state.event_bus.subscribe();
    // Send connected event
    let json = serde_json::to_string(&crate::events::Event::Connected).unwrap();
    if socket.send(Message::Text(json.into())).await.is_err() {
        return;
    }

    while let Ok(event) = rx.recv().await {
        let json = serde_json::to_string(&event).unwrap();
        if socket.send(Message::Text(json.into())).await.is_err() {
            break;
        }
    }
}
