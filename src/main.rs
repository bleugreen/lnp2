mod actuators;
mod api;
mod config;
mod gcode;
mod motion;
mod state;

use std::path::PathBuf;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::info;

use crate::gcode::GCodeDriver;
use crate::state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // Load config
    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("config/machine.toml"));

    info!("Loading config from {}", config_path.display());
    let machine_config = config::load_config(&config_path)?;

    // Connect to serial
    info!(
        "Connecting to {} at {} baud",
        machine_config.serial.port, machine_config.serial.baud
    );
    let gcode = GCodeDriver::connect(
        &machine_config.serial,
        &machine_config.connect.init_commands,
    )
    .await?;

    let config = Arc::new(RwLock::new(machine_config));
    let state = AppState::new(gcode, config);

    // Start HTTP server
    let app = api::router(state);
    let addr = "0.0.0.0:3000";
    let listener = TcpListener::bind(addr).await?;
    info!("Listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}
