use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tracing::debug;

use crate::gcode::{parser, GCodeDriver};

use super::commands::{self, PhotonError};
use super::packet::Packet;

#[derive(Debug, thiserror::Error)]
pub enum BusError {
    #[error("GCode error: {0}")]
    GCode(#[from] crate::gcode::GCodeError),
    #[error("No RS-485 reply in response")]
    NoReply,
    #[error("Invalid packet in reply")]
    InvalidPacket,
    #[error("Packet ID mismatch (expected {expected}, got {got})")]
    PacketIdMismatch { expected: u8, got: u8 },
    #[error("Feeder error: {0}")]
    FeederError(PhotonError),
    #[error("Invalid response payload")]
    InvalidResponse,
    #[error("Feed timeout after {0}ms")]
    FeedTimeout(u64),
}

pub struct PhotonBus {
    gcode: Arc<GCodeDriver>,
    from_address: u8,
    packet_id: AtomicU8,
}

impl PhotonBus {
    pub fn new(gcode: Arc<GCodeDriver>) -> Self {
        Self {
            gcode,
            from_address: 0x00,
            packet_id: AtomicU8::new(0),
        }
    }

    fn next_packet_id(&self) -> u8 {
        self.packet_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Send a packet via M485 and decode the response.
    pub async fn send(&self, mut packet: Packet) -> Result<Packet, BusError> {
        packet.from_address = self.from_address;
        packet.packet_id = self.next_packet_id();

        let hex = packet.encode();
        debug!("M485 >> {}", hex);

        let cmd = format!("M485 {}", hex);
        let response = self.gcode.send(&cmd).await?;

        let reply_hex = parser::parse_rs485(&response).ok_or(BusError::NoReply)?;
        debug!("M485 << {}", reply_hex);

        let reply = Packet::decode(&reply_hex).ok_or(BusError::InvalidPacket)?;

        if reply.packet_id != packet.packet_id {
            return Err(BusError::PacketIdMismatch {
                expected: packet.packet_id,
                got: reply.packet_id,
            });
        }

        Ok(reply)
    }

    /// Get the hardware UUID of the feeder at the given address.
    pub async fn get_feeder_id(&self, address: u8) -> Result<String, BusError> {
        let reply = self.send(commands::get_feeder_id(address)).await?;
        let resp = commands::parse_feeder_id(&reply).ok_or(BusError::InvalidResponse)?;
        if !resp.error.is_ok() {
            return Err(BusError::FeederError(resp.error));
        }
        resp.uuid.ok_or(BusError::InvalidResponse)
    }

    /// Initialize a feeder with its UUID.
    pub async fn initialize(&self, address: u8, uuid: &str) -> Result<(), BusError> {
        let reply = self.send(commands::initialize_feeder(address, uuid)).await?;
        let resp = commands::parse_feeder_id(&reply).ok_or(BusError::InvalidResponse)?;
        if !resp.error.is_ok() {
            return Err(BusError::FeederError(resp.error));
        }
        Ok(())
    }

    /// Feed forward by the given distance (in 0.1mm increments).
    /// Returns the expected feed time in ms.
    pub async fn feed_forward(
        &self,
        address: u8,
        distance_tenths_mm: u8,
    ) -> Result<u16, BusError> {
        let reply = self
            .send(commands::move_feed_forward(address, distance_tenths_mm))
            .await?;
        let resp = commands::parse_feed_response(&reply).ok_or(BusError::InvalidResponse)?;
        if !resp.error.is_ok() {
            return Err(BusError::FeederError(resp.error));
        }
        Ok(resp.expected_time_ms)
    }

    /// Feed forward and poll until complete.
    pub async fn feed_and_wait(
        &self,
        address: u8,
        distance_tenths_mm: u8,
    ) -> Result<(), BusError> {
        let expected_ms = self.feed_forward(address, distance_tenths_mm).await?;

        // Wait for the expected time, then poll
        if expected_ms > 0 {
            tokio::time::sleep(Duration::from_millis(expected_ms as u64)).await;
        }

        let timeout_ms = 5000u64;
        let poll_interval = Duration::from_millis(50);
        let start = std::time::Instant::now();

        loop {
            let reply = self.send(commands::move_feed_status(address)).await?;
            let resp =
                commands::parse_status_response(&reply).ok_or(BusError::InvalidResponse)?;

            if resp.error.is_ok() {
                return Ok(()); // Feed complete
            }
            if resp.error != PhotonError::FeedingInProgress {
                return Err(BusError::FeederError(resp.error));
            }
            if start.elapsed().as_millis() > timeout_ms as u128 {
                return Err(BusError::FeedTimeout(timeout_ms));
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    /// Feed backward by the given distance (in 0.1mm increments).
    pub async fn feed_backward(
        &self,
        address: u8,
        distance_tenths_mm: u8,
    ) -> Result<u16, BusError> {
        let reply = self
            .send(commands::move_feed_backward(address, distance_tenths_mm))
            .await?;
        let resp = commands::parse_feed_response(&reply).ok_or(BusError::InvalidResponse)?;
        if !resp.error.is_ok() {
            return Err(BusError::FeederError(resp.error));
        }
        Ok(resp.expected_time_ms)
    }

    /// Broadcast: identify a feeder by UUID (makes it flash its LED).
    pub async fn identify(&self, uuid: &str) -> Result<(), BusError> {
        let reply = self.send(commands::identify_feeder(uuid)).await?;
        let resp = commands::parse_status_response(&reply).ok_or(BusError::InvalidResponse)?;
        if !resp.error.is_ok() {
            return Err(BusError::FeederError(resp.error));
        }
        Ok(())
    }

    /// Broadcast: get the slot address for a feeder by UUID.
    pub async fn get_feeder_address(&self, uuid: &str) -> Result<u8, BusError> {
        let reply = self.send(commands::get_feeder_address(uuid)).await?;
        let resp = commands::parse_status_response(&reply).ok_or(BusError::InvalidResponse)?;
        if !resp.error.is_ok() {
            return Err(BusError::FeederError(resp.error));
        }
        Ok(reply.from_address)
    }
}
