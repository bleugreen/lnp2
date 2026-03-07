mod actuators;
mod api;
mod config;
mod gcode;
mod import;
mod motion;
mod photon;
mod state;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tokio::net::TcpListener;
use tracing::info;

use crate::gcode::GCodeDriver;
use crate::state::AppState;

#[derive(Parser)]
#[command(name = "rsrvpnp", about = "Pick and place machine controller")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the HTTP server (default)
    Serve {
        /// Config directory path
        #[arg(long, default_value = "config")]
        config: PathBuf,
    },
    /// Import OpenPnP configuration to TOML files
    Import {
        /// OpenPnP config directory (e.g. ~/.openpnp2)
        #[arg(long)]
        from: PathBuf,
        /// Output directory for TOML files
        #[arg(long, default_value = "config")]
        to: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Import { from, to }) => {
            info!("Importing from {} to {}", from.display(), to.display());
            import::import_openpnp(&from, &to)?;
            info!("Import complete");
            Ok(())
        }
        cmd => {
            let config_dir = match cmd {
                Some(Commands::Serve { config }) => config,
                _ => PathBuf::from("config"),
            };

            info!("Loading config from {}", config_dir.display());
            let full_config = config::load_full_config(&config_dir)?;

            info!(
                "Connecting to {} at {} baud",
                full_config.machine.serial.port, full_config.machine.serial.baud
            );
            let gcode = GCodeDriver::connect(
                &full_config.machine.serial,
                &full_config.machine.connect.init_commands,
            )
            .await?;

            let state = AppState::new(gcode, full_config);

            let app = api::router(state);
            let addr = "0.0.0.0:3000";
            let listener = TcpListener::bind(addr).await?;
            info!("Listening on {}", addr);
            axum::serve(listener, app).await?;

            Ok(())
        }
    }
}
