// Frame format: [Preamble] [Frame Type] [Sequence] [Length] [Data] [CRC8]

use super::crc::{bits_to_bytes, bytes_to_bits, calculate_crc8, verify_crc8};
use tracing::debug;

pub type CRCType = u8;
pub type SeqType = u8;
pub type LenType = u16;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FrameType {
    Data = 0x01,
    Ack = 0x02,
    // Reserved for future use
}

impl FrameType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(FrameType::Data),
            0x02 => Some(FrameType::Ack),
            _ => None,
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

/// PHY Frame structure
#[derive(Debug, Clone)]
pub struct Frame {
    pub frame_type: FrameType,
    pub sequence: u8,  // Sequence number for ordering and ACK
    pub data: Vec<u8>, // Payload data
}

impl Frame {
    pub fn new(frame_type: FrameType, sequence: u8, data: Vec<u8>) -> Self {
        Self {
            frame_type,
            sequence,
            data,
        }
    }

    pub fn new_data(sequence: u8, data: Vec<u8>) -> Self {
        Self::new(FrameType::Data, sequence, data)
    }

    pub fn new_ack(sequence: u8) -> Self {
        Self::new(FrameType::Ack, sequence, Vec::new())
    }

    /// Serialize frame to bytes (without preamble)
    /// Format: [Len:2] [CRC:1] [Type:1] [Seq:1] [Data:N]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        // Data length (2 bytes, big-endian)
        let len = self.data.len() as LenType;
        bytes.push((len >> 8) as u8);
        bytes.push((len & 0xFF) as u8);

        // CRC8
        let crc = calculate_crc8(&self.data);
        bytes.push(crc);

        // Frame type (1 byte)
        bytes.push(self.frame_type.to_u8());

        // Sequence number (1 byte)
        bytes.push(self.sequence);

        // Data
        bytes.extend_from_slice(&self.data);

        bytes
    }

    /// Serialize frame to bits (without preamble)
    pub fn to_bits(&self) -> Vec<u8> {
        bytes_to_bits(&self.to_bytes())
    }


    fn bits_to_byte(bits: &[u8]) -> u8 {
        let mut byte = 0u8;
        for (i, &bit) in bits.iter().enumerate().take(8) {
            if bit != 0 {
                byte |= 1 << (7 - i);
            }
        }
        byte
    }

    pub fn parse_header(bits: &[u8]) -> Option<(LenType, CRCType, FrameType, SeqType)> {
        let bytes = bits_to_bytes(bits);
        Self::parse_header_bytes(&bytes)
    }

    fn parse_header_bytes(bytes: &[u8]) -> Option<(LenType, CRCType, FrameType, SeqType)> {
        if bytes.len() < 5 {
            debug!("PHY Header too short: {} bytes", bytes.len());
            return None;
        }

        // Parse length
        let len: LenType = ((bytes[0] as u16) << 8) | (bytes[1] as u16);

        // Parse CRC
        let crc: CRCType = bytes[2];

        // Parse frame type
        let frame_type: FrameType = FrameType::from_u8(bytes[3])?;

        // Parse sequence
        let sequence: SeqType = bytes[4];

        Some((len, crc, frame_type, sequence))
    }

    /// Deserialize frame from bytes (without preamble)
    /// Returns None if CRC check fails or format is invalid
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        // if bytes.len() < 5 { // TODO: remove?
        //     // Minimum: type(1) + seq(1) + len(2) + crc(1)
        //     debug!("Frame too short: {} bytes", bytes.len());
        //     return None;
        // }

        let (len, crc, frame_type, sequence) = Self::parse_header_bytes(&bytes[..5])?;

        // Extract CRC and data
        let data_bytes = &bytes[5..5 + len as usize];
        // let crc = bytes[bytes.len() - 1];

        // Verify CRC
        if !verify_crc8(data_bytes, crc) {
            debug!("CRC check failed");
            return None;
        }

        // Parse frame type
        // let frame_type = FrameType::from_u8(bytes[0])?;

        // Parse sequence
        // let sequence = bytes[1];

        // Parse length
        // let len = ((bytes[2] as u16) << 8) | (bytes[3] as u16);

        // Check if we have enough data
        if bytes.len() < 4 + len as usize + 1 { // TODO: change me!
            debug!("Frame data incomplete");
            return None;
        }

        // Extract data
        // let data = bytes[4..4 + len as usize].to_vec();
        let data = data_bytes.to_vec();

        Some(Frame {
            frame_type,
            sequence,
            data,
        })
    }

    /// Deserialize frame from bits (without preamble)
    pub fn from_bits(bits: &[u8]) -> Option<Self> {
        let bytes = bits_to_bytes(bits);
        Self::from_bytes(&bytes)
    }

    /// Get the total size in bytes (including CRC)
    pub fn size_bytes(&self) -> usize {
        4 + self.data.len() + 1 // type + seq + len(2) + data + crc
    }

    /// Get the total size in bits
    pub fn size_bits(&self) -> usize {
        self.size_bytes() * 8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_frame_serialization() {
        let data = vec![0x01, 0x02, 0x03, 0x04];
        let frame = Frame::new_data(42, data.clone());

        let bytes = frame.to_bytes();
        let recovered = Frame::from_bytes(&bytes).unwrap();

        assert_eq!(recovered.frame_type, FrameType::Data);
        assert_eq!(recovered.sequence, 42);
        assert_eq!(recovered.data, data);
    }

    #[test]
    fn test_ack_frame_serialization() {
        let frame = Frame::new_ack(99);

        let bytes = frame.to_bytes();
        let recovered = Frame::from_bytes(&bytes).unwrap();

        assert_eq!(recovered.frame_type, FrameType::Ack);
        assert_eq!(recovered.sequence, 99);
        assert_eq!(recovered.data.len(), 0);
    }

    #[test]
    fn test_crc_verification() {
        let frame = Frame::new_data(1, vec![0xAA, 0xBB, 0xCC]);
        let mut bytes = frame.to_bytes();

        // Corrupt data
        bytes[4] ^= 0xFF;

        // Should fail CRC check
        assert!(Frame::from_bytes(&bytes).is_none());
    }

    #[test]
    fn test_bits_serialization() {
        let frame = Frame::new_data(5, vec![0x12, 0x34]);
        let bits = frame.to_bits();
        let recovered = Frame::from_bits(&bits).unwrap();

        assert_eq!(recovered.sequence, 5);
        assert_eq!(recovered.data, vec![0x12, 0x34]);
    }
}
