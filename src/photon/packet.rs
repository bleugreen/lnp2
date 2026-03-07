/// CRC8-107 checksum used by the PhotonFeeder protocol.
/// Polynomial 0x107, ported from OpenPnP's CRC8_107.java.
pub fn crc8_107(data: &[u8]) -> u8 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc ^= 0x1070 << 3;
            }
            crc <<= 1;
        }
    }
    ((crc >> 8) & 0xFF) as u8
}

/// A PhotonFeeder protocol packet.
///
/// Wire format:
/// ```text
/// Header (5 bytes): [ToAddr, FromAddr, PacketID, PayloadLen, CRC8]
/// Payload:          [CommandID, ...data]
/// ```
#[derive(Debug, Clone)]
pub struct Packet {
    pub to_address: u8,
    pub from_address: u8,
    pub packet_id: u8,
    pub payload: Vec<u8>,
}

impl Packet {
    pub fn new(to_address: u8, command_id: u8) -> Self {
        Self {
            to_address,
            from_address: 0,
            packet_id: 0,
            payload: vec![command_id],
        }
    }

    /// Encode the packet to a hex string for M485 transport.
    pub fn encode(&self) -> String {
        let crc = self.calculate_crc();
        let payload_len = self.payload.len() as u8;

        let mut hex = format!(
            "{:02X}{:02X}{:02X}{:02X}{:02X}",
            self.to_address, self.from_address, self.packet_id, payload_len, crc
        );
        for &b in &self.payload {
            hex.push_str(&format!("{:02X}", b));
        }
        hex
    }

    /// Decode a packet from a hex string. Returns None if invalid.
    pub fn decode(hex: &str) -> Option<Self> {
        if hex == "TIMEOUT" {
            return None;
        }
        if hex.len() % 2 != 0 || hex.len() < 10 {
            return None;
        }

        let bytes = hex_to_bytes(hex)?;
        if bytes.len() < 5 {
            return None;
        }

        let to_address = bytes[0];
        let from_address = bytes[1];
        let packet_id = bytes[2];
        let payload_len = bytes[3] as usize;
        let expected_crc = bytes[4];
        let payload = bytes[5..].to_vec();

        if payload.len() != payload_len {
            return None;
        }

        let packet = Packet {
            to_address,
            from_address,
            packet_id,
            payload,
        };

        if packet.calculate_crc() != expected_crc {
            return None;
        }

        Some(packet)
    }

    fn calculate_crc(&self) -> u8 {
        let payload_len = self.payload.len() as u8;
        let mut data = vec![self.to_address, self.from_address, self.packet_id, payload_len];
        data.extend_from_slice(&self.payload);
        crc8_107(&data)
    }

    /// Extract a 12-byte UUID as hex string from the payload at the given offset.
    pub fn uuid(&self, start: usize) -> Option<String> {
        if start + 12 > self.payload.len() {
            return None;
        }
        let mut s = String::with_capacity(24);
        for &b in &self.payload[start..start + 12] {
            s.push_str(&format!("{:02X}", b));
        }
        Some(s)
    }

    /// Extract a big-endian u16 from the payload at the given offset.
    pub fn uint16(&self, start: usize) -> Option<u16> {
        if start + 2 > self.payload.len() {
            return None;
        }
        Some(((self.payload[start] as u16) << 8) | self.payload[start + 1] as u16)
    }

    /// Push a byte to the payload.
    pub fn push_byte(&mut self, b: u8) {
        self.payload.push(b);
    }

    /// Push a 12-byte UUID (24 hex chars) to the payload.
    pub fn push_uuid(&mut self, uuid: &str) {
        if let Some(bytes) = hex_to_bytes(uuid) {
            self.payload.extend_from_slice(&bytes);
        }
    }
}

fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc8_107_known_values() {
        // From PacketTest.java: packet 2B1347010A03
        // Header: to=0x2B, from=0x13, id=0x47, len=0x01, payload=[0x03]
        // CRC should be 0x0A
        let data = [0x2B, 0x13, 0x47, 0x01, 0x03];
        assert_eq!(crc8_107(&data), 0x0A);
    }

    #[test]
    fn test_encode_packet() {
        let packet = Packet {
            to_address: 0x2B,
            from_address: 0x13,
            packet_id: 0x47,
            payload: vec![0x03],
        };
        assert_eq!(packet.encode(), "2B1347010A03");
    }

    #[test]
    fn test_decode_valid_packet() {
        let packet = Packet::decode("2B1347010A03").unwrap();
        assert_eq!(packet.to_address, 0x2B);
        assert_eq!(packet.from_address, 0x13);
        assert_eq!(packet.packet_id, 0x47);
        assert_eq!(packet.payload, vec![0x03]);
    }

    #[test]
    fn test_decode_empty_string() {
        assert!(Packet::decode("").is_none());
    }

    #[test]
    fn test_decode_odd_length() {
        assert!(Packet::decode("01234").is_none());
    }

    #[test]
    fn test_decode_bad_checksum() {
        // Real checksum is 0xC7, modified to 0xC9
        assert!(Packet::decode("2B000001C903").is_none());
    }

    #[test]
    fn test_decode_wrong_length_short() {
        // Length says 0x00 but has 1 byte payload
        assert!(Packet::decode("2B000000D203").is_none());
    }

    #[test]
    fn test_decode_wrong_length_long() {
        // Length says 0x02 but has 1 byte payload
        assert!(Packet::decode("2B000002F803").is_none());
    }

    #[test]
    fn test_decode_timeout() {
        assert!(Packet::decode("TIMEOUT").is_none());
    }

    #[test]
    fn test_roundtrip() {
        let original = Packet {
            to_address: 0xFF,
            from_address: 0x00,
            packet_id: 0x05,
            payload: vec![0xC0, 0x01, 0x02, 0x03],
        };
        let hex = original.encode();
        let decoded = Packet::decode(&hex).unwrap();
        assert_eq!(decoded.to_address, original.to_address);
        assert_eq!(decoded.from_address, original.from_address);
        assert_eq!(decoded.packet_id, original.packet_id);
        assert_eq!(decoded.payload, original.payload);
    }

    #[test]
    fn test_uuid_extraction() {
        // Payload: [error_byte, 12 uuid bytes]
        let mut payload = vec![0x00]; // error = None
        payload.extend_from_slice(&[
            0x00, 0x07, 0x80, 0x0B, 0x42, 0x48, 0x57, 0x17, 0x20, 0x34, 0x33, 0x31,
        ]);
        let packet = Packet {
            to_address: 0,
            from_address: 0,
            packet_id: 0,
            payload,
        };
        assert_eq!(
            packet.uuid(1).unwrap(),
            "0007800B4248571720343331"
        );
    }

    #[test]
    fn test_uint16() {
        let packet = Packet {
            to_address: 0,
            from_address: 0,
            packet_id: 0,
            payload: vec![0x00, 0x01, 0xF4], // error(0), uint16(500)
        };
        assert_eq!(packet.uint16(1).unwrap(), 500);
    }
}
