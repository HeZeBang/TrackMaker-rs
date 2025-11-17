// Frame format: [Preamble] [Frame Type] [Sequence] [Length] [Data] [CRC8]

use crate::utils::consts::PHY_HEADER_BYTES;

use super::crc::{bits_to_bytes, bytes_to_bits, calculate_crc8, verify_crc8};
use tracing::debug;

pub type CRCType = u8;
pub type SeqType = u8;
pub type LenType = usize;

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
    pub src: u8,       // Source address
    pub dst: u8,       // Destination address
    pub data: Vec<u8>, // Payload data
}

impl Frame {
    pub fn new(frame_type: FrameType, sequence: u8, src: u8, dst: u8, data: Vec<u8>) -> Self {
        Self {
            frame_type,
            sequence,
            src,
            dst,
            data,
        }
    }

    pub fn new_data(sequence: u8, src: u8, dst: u8, data: Vec<u8>) -> Self {
        Self::new(FrameType::Data, sequence, src, dst, data)
    }

    pub fn new_ack(sequence: u8, from: u8, to: u8) -> Self {
        Self::new(FrameType::Ack, sequence, from, to, Vec::new())
    }

    /// Serialize frame to bytes (without preamble)
    /// Format: [Len:2] [CRC:1] [Type:1] [Seq:1] [Src:1] [Dst:1] [Data:N]
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

        // Source address (1 byte)
        bytes.push(self.src);

        // Destination address (1 byte)
        bytes.push(self.dst);

        // Data
        bytes.extend_from_slice(&self.data);

        bytes
    }

    /// Serialize frame to bits (without preamble)
    pub fn to_bits(&self) -> Vec<u8> {
        bytes_to_bits(&self.to_bytes())
    }

    pub fn parse_header(bits: &[u8]) -> Option<(LenType, CRCType, FrameType, SeqType, u8, u8)> {
        let bytes = bits_to_bytes(bits);
        Self::parse_header_bytes(&bytes)
    }

    fn parse_header_bytes(bytes: &[u8]) -> Option<(LenType, CRCType, FrameType, SeqType, u8, u8)> {
        if bytes.len() < PHY_HEADER_BYTES {
            debug!("PHY Header too short: {} bytes", bytes.len());
            return None;
        }

        // Parse length
        let len: LenType = ((bytes[0] as usize) << 8) | (bytes[1] as usize);

        // Parse CRC
        let crc: CRCType = bytes[2];

        // Parse frame type
        let frame_type: FrameType = FrameType::from_u8(bytes[3])?;

        // Parse sequence
        let sequence: SeqType = bytes[4];

        // Parse source address
        let src: u8 = bytes[5];

        // Parse destination address
        let dst: u8 = bytes[6];

        Some((len, crc, frame_type, sequence, src, dst))
    }

    /// Deserialize frame from bytes (without preamble)
    /// Returns None if CRC check fails or format is invalid
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let (len, crc, frame_type, sequence, src, dst) =
            Self::parse_header_bytes(&bytes[..PHY_HEADER_BYTES])?;

        // Extract CRC and data
        let data_bytes = &bytes[PHY_HEADER_BYTES..PHY_HEADER_BYTES + len as usize];

        // Verify CRC
        if !verify_crc8(data_bytes, crc) {
            debug!("CRC check failed");
            return None;
        }

        // Check if we have enough data
        if bytes.len() < PHY_HEADER_BYTES + len as usize {
            debug!("Frame data incomplete");
            return None;
        }

        // Extract data
        let data = data_bytes.to_vec();

        Some(Frame {
            frame_type,
            sequence,
            src,
            dst,
            data,
        })
    }

    /// Deserialize frame from bits (without preamble)
    pub fn from_bits(bits: &[u8]) -> Option<Self> {
        let bytes = bits_to_bytes(bits);
        Self::from_bytes(&bytes)
    }
}