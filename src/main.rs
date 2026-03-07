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
use crate::photon::PhotonBus;
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
    /// Scan RS-485 bus for feeders
    Scan {
        /// Config directory path
        #[arg(long, default_value = "config")]
        config: PathBuf,
        /// Start address to scan
        #[arg(long, default_value_t = 1)]
        from_addr: u8,
        /// End address to scan (inclusive)
        #[arg(long, default_value_t = 25)]
        to_addr: u8,
    },
    /// Feed a specific feeder slot
    Feed {
        /// Config directory path
        #[arg(long, default_value = "config")]
        config: PathBuf,
        /// Slot address
        #[arg(long)]
        slot: u8,
        /// Distance in mm (default: 2.0)
        #[arg(long, default_value_t = 2.0)]
        distance: f64,
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
        }
        Some(Commands::Scan {
            config,
            from_addr,
            to_addr,
        }) => {
            let full_config = config::load_full_config(&config)?;
            let gcode = GCodeDriver::connect(
                &full_config.machine.serial,
                &full_config.machine.connect.init_commands,
            )
            .await?;
            let bus = PhotonBus::new(gcode);

            println!("Scanning addresses {}..={}", from_addr, to_addr);
            for addr in from_addr..=to_addr {
                match bus.get_feeder_id(addr).await {
                    Ok(uuid) => println!("  slot {:>2}: {} (feeder found)", addr, uuid),
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains("Timeout") || msg.contains("No RS-485") {
                            // No feeder at this address — silent
                        } else {
                            println!("  slot {:>2}: {}", addr, msg);
                        }
                    }
                }
            }
            println!("Scan complete");
        }
        Some(Commands::Feed {
            config,
            slot,
            distance,
        }) => {
            let full_config = config::load_full_config(&config)?;
            let gcode = GCodeDriver::connect(
                &full_config.machine.serial,
                &full_config.machine.connect.init_commands,
            )
            .await?;
            let bus = PhotonBus::new(gcode);

            // First get the feeder UUID and initialize it
            println!("Initializing slot {}...", slot);
            let uuid = bus.get_feeder_id(slot).await?;
            println!("  UUID: {}", uuid);
            bus.initialize(slot, &uuid).await?;
            println!("  Initialized");

            // Convert mm to tenths (0.1mm units)
            let tenths = (distance * 10.0).round() as u8;
            println!("Feeding forward {}mm ({} tenths)...", distance, tenths);

            bus.feed_and_wait(slot, tenths).await?;
            println!("Feed complete");
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
        }
    }

    Ok(())
}
