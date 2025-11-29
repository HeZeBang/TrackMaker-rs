use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read, Write};

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum IcmpType {
    EchoReply = 0,
    EchoRequest = 8,
    Unknown(u8),
}

impl From<u8> for IcmpType {
    fn from(val: u8) -> Self {
        match val {
            0 => IcmpType::EchoReply,
            8 => IcmpType::EchoRequest,
            n => IcmpType::Unknown(n),
        }
    }
}

impl Into<u8> for IcmpType {
    fn into(self) -> u8 {
        match self {
            IcmpType::EchoReply => 0,
            IcmpType::EchoRequest => 8,
            IcmpType::Unknown(n) => n,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IcmpPacket {
    pub icmp_type: IcmpType,
    pub code: u8,
    // Not only header, complete packet
    pub checksum: u16,
    pub identifier: u16,
    pub sequence_number: u16,
    pub payload: Vec<u8>,
}

impl IcmpPacket {
    pub fn new(
        icmp_type: IcmpType,
        code: u8,
        identifier: u16,
        sequence_number: u16,
        payload: Vec<u8>,
    ) -> Self {
        let mut packet = Self {
            icmp_type,
            code,
            checksum: 0,
            identifier,
            sequence_number,
            payload,
        };
        packet.checksum = packet.calculate_checksum();
        packet
    }

    pub fn from_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        let mut rdr = Cursor::new(bytes);
        let icmp_type = IcmpType::from(rdr.read_u8()?);
        let code = rdr.read_u8()?;
        let checksum = rdr.read_u16::<BigEndian>()?;
        let identifier = rdr.read_u16::<BigEndian>()?;
        let sequence_number = rdr.read_u16::<BigEndian>()?;

        let mut payload = Vec::new();
        rdr.read_to_end(&mut payload)?;

        Ok(Self {
            icmp_type,
            code,
            checksum,
            identifier,
            sequence_number,
            payload,
        })
    }

    pub fn to_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut wtr = Vec::new();
        wtr.write_u8(self.icmp_type.into())?;
        wtr.write_u8(self.code)?;
        wtr.write_u16::<BigEndian>(self.checksum)?;
        wtr.write_u16::<BigEndian>(self.identifier)?;
        wtr.write_u16::<BigEndian>(self.sequence_number)?;
        wtr.write_all(&self.payload)?;
        Ok(wtr)
    }

    pub fn calculate_checksum(&self) -> u16 {
        let mut temp_packet = self.clone();
        temp_packet.checksum = 0;
        // We can safely unwrap here because to_bytes only fails on IO errors with the writer,
        // and Vec<u8> writer doesn't fail on small writes.
        let bytes = temp_packet
            .to_bytes()
            .unwrap();

        let mut sum = 0u32;
        for chunk in bytes.chunks(2) {
            let word = if chunk.len() == 2 {
                u16::from_be_bytes([chunk[0], chunk[1]])
            } else {
                // padding 0 for odd
                // BigEndian
                u16::from_be_bytes([chunk[0], 0])
            };
            sum = sum.wrapping_add(word as u32);
        }

        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }

        !(sum as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icmp_packet_serialization() {
        let payload = vec![1, 2, 3, 4];
        let packet =
            IcmpPacket::new(IcmpType::EchoRequest, 0, 123, 456, payload.clone());

        let bytes = packet.to_bytes().unwrap();
        let deserialized = IcmpPacket::from_bytes(&bytes).unwrap();

        assert_eq!(packet.icmp_type, deserialized.icmp_type);
        assert_eq!(packet.code, deserialized.code);
        assert_eq!(packet.identifier, deserialized.identifier);
        assert_eq!(packet.sequence_number, deserialized.sequence_number);
        assert_eq!(packet.payload, deserialized.payload);
        assert_eq!(packet.checksum, deserialized.checksum);
    }
}
