use super::packet::Packet;

const BROADCAST: u8 = 0xFF;

/// PhotonFeeder error codes returned in response payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhotonError {
    None,
    WrongFeederUuid,
    CouldNotReach,
    UninitializedFeeder,
    FeedingInProgress,
    Unknown(u8),
}

impl PhotonError {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0x00 => Self::None,
            0x01 => Self::WrongFeederUuid,
            0x02 => Self::CouldNotReach,
            0x03 => Self::UninitializedFeeder,
            0x04 => Self::FeedingInProgress,
            other => Self::Unknown(other),
        }
    }

    pub fn is_ok(self) -> bool {
        matches!(self, Self::None)
    }
}

impl std::fmt::Display for PhotonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::WrongFeederUuid => write!(f, "Wrong feeder UUID"),
            Self::CouldNotReach => write!(f, "Could not reach feeder"),
            Self::UninitializedFeeder => write!(f, "Uninitialized feeder"),
            Self::FeedingInProgress => write!(f, "Feeding in progress"),
            Self::Unknown(code) => write!(f, "Unknown error (0x{:02X})", code),
        }
    }
}

// --- Command IDs ---

pub const GET_FEEDER_ID: u8 = 0x01;
pub const INITIALIZE_FEEDER: u8 = 0x02;
pub const GET_VERSION: u8 = 0x03;
pub const MOVE_FEED_FORWARD: u8 = 0x04;
pub const MOVE_FEED_BACKWARD: u8 = 0x05;
pub const MOVE_FEED_STATUS: u8 = 0x06;
pub const GET_FEEDER_ADDRESS: u8 = 0xC0;
pub const IDENTIFY_FEEDER: u8 = 0xC1;

// --- Command builders ---

/// Build a GetFeederId packet. Response: error(1) + uuid(12).
pub fn get_feeder_id(address: u8) -> Packet {
    Packet::new(address, GET_FEEDER_ID)
}

/// Build an InitializeFeeder packet. Payload: uuid(12). Response: error(1) + uuid(12).
pub fn initialize_feeder(address: u8, uuid: &str) -> Packet {
    let mut pkt = Packet::new(address, INITIALIZE_FEEDER);
    pkt.push_uuid(uuid);
    pkt
}

/// Build a GetVersion packet. Response: error(1).
pub fn get_version(address: u8) -> Packet {
    Packet::new(address, GET_VERSION)
}

/// Build a MoveFeedForward packet. Distance in 0.1mm increments.
/// Response: error(1) + expectedTime(2).
pub fn move_feed_forward(address: u8, distance_tenths_mm: u8) -> Packet {
    let mut pkt = Packet::new(address, MOVE_FEED_FORWARD);
    pkt.push_byte(distance_tenths_mm);
    pkt
}

/// Build a MoveFeedBackward packet. Distance in 0.1mm increments.
/// Response: error(1) + expectedTime(2).
pub fn move_feed_backward(address: u8, distance_tenths_mm: u8) -> Packet {
    let mut pkt = Packet::new(address, MOVE_FEED_BACKWARD);
    pkt.push_byte(distance_tenths_mm);
    pkt
}

/// Build a MoveFeedStatus packet. Response: error(1).
pub fn move_feed_status(address: u8) -> Packet {
    Packet::new(address, MOVE_FEED_STATUS)
}

/// Build a GetFeederAddress broadcast packet. Payload: uuid(12). Response: error(1).
pub fn get_feeder_address(uuid: &str) -> Packet {
    let mut pkt = Packet::new(BROADCAST, GET_FEEDER_ADDRESS);
    pkt.push_uuid(uuid);
    pkt
}

/// Build an IdentifyFeeder broadcast packet. Payload: uuid(12). Response: error(1).
pub fn identify_feeder(uuid: &str) -> Packet {
    let mut pkt = Packet::new(BROADCAST, IDENTIFY_FEEDER);
    pkt.push_uuid(uuid);
    pkt
}

// --- Response parsers ---

pub struct FeederIdResponse {
    pub error: PhotonError,
    pub uuid: Option<String>,
}

pub fn parse_feeder_id(packet: &Packet) -> Option<FeederIdResponse> {
    // Payload: error(1) + uuid(12) = 13 bytes
    if packet.payload.len() != 13 {
        return None;
    }
    Some(FeederIdResponse {
        error: PhotonError::from_byte(packet.payload[0]),
        uuid: packet.uuid(1),
    })
}

pub struct FeedResponse {
    pub error: PhotonError,
    pub expected_time_ms: u16,
}

pub fn parse_feed_response(packet: &Packet) -> Option<FeedResponse> {
    if packet.payload.is_empty() {
        return None;
    }
    let error = PhotonError::from_byte(packet.payload[0]);
    let expected_time_ms = if error.is_ok() {
        packet.uint16(1).unwrap_or(0)
    } else {
        0
    };
    Some(FeedResponse {
        error,
        expected_time_ms,
    })
}

pub struct StatusResponse {
    pub error: PhotonError,
}

pub fn parse_status_response(packet: &Packet) -> Option<StatusResponse> {
    if packet.payload.len() != 1 {
        return None;
    }
    Some(StatusResponse {
        error: PhotonError::from_byte(packet.payload[0]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_feeder_id_packet() {
        let pkt = get_feeder_id(0x22);
        assert_eq!(pkt.to_address, 0x22);
        assert_eq!(pkt.payload, vec![GET_FEEDER_ID]);
    }

    #[test]
    fn test_initialize_feeder_packet() {
        let pkt = initialize_feeder(0x22, "0007800B4248571720343331");
        assert_eq!(pkt.to_address, 0x22);
        assert_eq!(pkt.payload.len(), 13); // cmd(1) + uuid(12)
        assert_eq!(pkt.payload[0], INITIALIZE_FEEDER);
    }

    #[test]
    fn test_move_feed_forward_packet() {
        let pkt = move_feed_forward(0x01, 20); // 2.0mm
        assert_eq!(pkt.payload, vec![MOVE_FEED_FORWARD, 20]);
    }

    #[test]
    fn test_get_feeder_address_broadcast() {
        let pkt = get_feeder_address("0007800B4248571720343331");
        assert_eq!(pkt.to_address, 0xFF);
        assert_eq!(pkt.payload[0], GET_FEEDER_ADDRESS);
        assert_eq!(pkt.payload.len(), 13); // cmd(1) + uuid(12)
    }

    #[test]
    fn test_parse_feeder_id_response() {
        let mut payload = vec![0x00]; // error = None
        payload.extend_from_slice(&[
            0x00, 0x07, 0x80, 0x0B, 0x42, 0x48, 0x57, 0x17, 0x20, 0x34, 0x33, 0x31,
        ]);
        let packet = Packet {
            to_address: 0,
            from_address: 0x22,
            packet_id: 0,
            payload,
        };
        let resp = parse_feeder_id(&packet).unwrap();
        assert!(resp.error.is_ok());
        assert_eq!(resp.uuid.unwrap(), "0007800B4248571720343331");
    }

    #[test]
    fn test_parse_feed_response() {
        let packet = Packet {
            to_address: 0,
            from_address: 0x01,
            packet_id: 0,
            payload: vec![0x00, 0x01, 0xF4], // error=None, time=500ms
        };
        let resp = parse_feed_response(&packet).unwrap();
        assert!(resp.error.is_ok());
        assert_eq!(resp.expected_time_ms, 500);
    }

    #[test]
    fn test_parse_status_feeding_in_progress() {
        let packet = Packet {
            to_address: 0,
            from_address: 0x01,
            packet_id: 0,
            payload: vec![0x04], // FeedingInProgress
        };
        let resp = parse_status_response(&packet).unwrap();
        assert_eq!(resp.error, PhotonError::FeedingInProgress);
    }

    #[test]
    fn test_parse_status_done() {
        let packet = Packet {
            to_address: 0,
            from_address: 0x01,
            packet_id: 0,
            payload: vec![0x00], // None = feed complete
        };
        let resp = parse_status_response(&packet).unwrap();
        assert!(resp.error.is_ok());
    }
}
