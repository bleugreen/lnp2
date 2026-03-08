pub mod parser;

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio_serial::SerialPortBuilderExt;
use tracing::{debug, error, info};

use crate::config::SerialConfig;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub a: f64,
    pub b: f64,
}

impl Default for Position {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, z: 0.0, a: 0.0, b: 0.0 }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GCodeError {
    #[error("Serial error: {0}")]
    Serial(#[from] tokio_serial::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Command error: {0}")]
    Command(String),
    #[error("Timeout waiting for response")]
    Timeout,
    #[error("Parse error: {0}")]
    Parse(String),
}

pub struct GCodeDriver {
    reader: Mutex<BufReader<tokio::io::ReadHalf<tokio_serial::SerialStream>>>,
    writer: Mutex<tokio::io::WriteHalf<tokio_serial::SerialStream>>,
    position: RwLock<Position>,
    default_timeout: Duration,
    motion_timeout: Duration,
    home_timeout: Duration,
}

impl GCodeDriver {
    /// Auto-detect a serial port by scanning /dev/ttyACM* and /dev/ttyUSB*.
    fn detect_port() -> Result<String, GCodeError> {
        let prefixes = ["/dev/ttyACM", "/dev/ttyUSB"];
        for prefix in &prefixes {
            let dir = std::path::Path::new("/dev");
            if let Ok(entries) = std::fs::read_dir(dir) {
                let mut matches: Vec<_> = entries
                    .flatten()
                    .filter(|e| e.path().to_string_lossy().starts_with(prefix))
                    .collect();
                matches.sort_by_key(|e| e.path());
                if let Some(entry) = matches.first() {
                    let path = entry.path().to_string_lossy().to_string();
                    info!("Auto-detected serial port: {}", path);
                    return Ok(path);
                }
            }
        }
        Err(GCodeError::Command("No serial port found (tried /dev/ttyACM*, /dev/ttyUSB*)".into()))
    }

    pub async fn connect(config: &SerialConfig, init_commands: &[String]) -> Result<Arc<Self>, GCodeError> {
        let port_path = if config.port == "auto" {
            Self::detect_port()?
        } else {
            config.port.clone()
        };
        info!("Opening serial port: {}", port_path);

        let port = tokio_serial::new(&port_path, config.baud)
            .flow_control(tokio_serial::FlowControl::Hardware)
            .data_bits(tokio_serial::DataBits::Eight)
            .parity(tokio_serial::Parity::None)
            .stop_bits(tokio_serial::StopBits::One)
            .open_native_async()?;

        let (read_half, write_half) = tokio::io::split(port);

        let driver = Arc::new(Self {
            reader: Mutex::new(BufReader::new(read_half)),
            writer: Mutex::new(write_half),
            position: RwLock::new(Position::default()),
            default_timeout: Duration::from_millis(config.timeout_ms),
            motion_timeout: Duration::from_millis(config.motion_timeout_ms),
            home_timeout: Duration::from_millis(config.home_timeout_ms),
        });

        // Wait briefly for Marlin to be ready, then drain any startup messages
        tokio::time::sleep(Duration::from_millis(500)).await;
        driver.drain().await;

        // Send init commands
        for cmd in init_commands {
            driver.send(cmd).await?;
        }

        Ok(driver)
    }

    /// Drain any pending data from the serial port (non-blocking).
    async fn drain(&self) {
        let mut reader = self.reader.lock().await;
        let mut line = String::new();
        loop {
            match tokio::time::timeout(Duration::from_millis(100), reader.read_line(&mut line)).await {
                Ok(Ok(0)) | Err(_) => break,
                Ok(Ok(_)) => {
                    debug!("drain: {}", line.trim());
                    line.clear();
                }
                Ok(Err(_)) => break,
            }
        }
    }

    /// Send a GCode command and wait for `ok` response.
    pub async fn send(&self, cmd: &str) -> Result<String, GCodeError> {
        self.send_timeout(cmd, self.default_timeout).await
    }

    /// Send a GCode command with a custom timeout.
    pub async fn send_timeout(&self, cmd: &str, timeout: Duration) -> Result<String, GCodeError> {
        debug!(">> {}", cmd);

        // Write command
        {
            let mut writer = self.writer.lock().await;
            writer.write_all(cmd.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }

        // Read until ok or error
        let mut response = String::new();
        let mut reader = self.reader.lock().await;
        let mut line = String::new();

        let result = tokio::time::timeout(timeout, async {
            loop {
                line.clear();
                let n = reader.read_line(&mut line).await?;
                if n == 0 {
                    return Err(GCodeError::Io(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "Serial port closed",
                    )));
                }
                let trimmed = line.trim();
                debug!("<< {}", trimmed);

                if parser::is_error(trimmed) {
                    return Err(GCodeError::Command(format!(
                        "Command '{}' failed: {}",
                        cmd, trimmed
                    )));
                }

                response.push_str(trimmed);
                response.push('\n');

                if parser::is_ok(trimmed) {
                    return Ok(());
                }
            }
        })
        .await;

        match result {
            Ok(Ok(())) => Ok(response),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                error!("Timeout waiting for response to: {}", cmd);
                Err(GCodeError::Timeout)
            }
        }
    }

    /// Send M114 and return parsed position.
    pub async fn position(&self) -> Result<Position, GCodeError> {
        let response = self.send("M114").await?;
        let pos = parser::parse_position(&response)
            .ok_or_else(|| GCodeError::Parse(format!("Failed to parse position from: {}", response)))?;
        *self.position.write().await = pos.clone();
        Ok(pos)
    }

    /// Send M400 — wait for all moves to complete.
    pub async fn wait(&self) -> Result<(), GCodeError> {
        self.send_timeout("M400", self.motion_timeout).await?;
        Ok(())
    }

    /// Home all axes.
    pub async fn home(&self) -> Result<(), GCodeError> {
        self.send_timeout("G28", self.home_timeout).await?;
        // Update position cache after homing
        self.position().await?;
        Ok(())
    }

    /// Get cached position without querying.
    pub async fn cached_position(&self) -> Position {
        self.position.read().await.clone()
    }

    /// Motion timeout accessor.
    pub fn motion_timeout(&self) -> Duration {
        self.motion_timeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_default() {
        let pos = Position::default();
        assert!((pos.x - 0.0).abs() < 0.001);
        assert!((pos.z - 0.0).abs() < 0.001);
    }
}
