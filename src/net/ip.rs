use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read, Write};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ipv4Header {
    pub version_ihl: u8,
    // Type of service
    pub tos: u8,
    pub total_length: u16,
    pub identification: u16,
    pub flags_fragment_offset: u16,
    pub ttl: u8,
    pub protocol: u8,
    // This is a header checksum, not a data checksum
    pub checksum: u16,
    pub source_ip: [u8; 4],
    pub dest_ip: [u8; 4],
}

impl Ipv4Header {
    pub fn new(
        total_length: u16,
        identification: u16,
        ttl: u8,
        protocol: u8,
        source_ip: [u8; 4],
        dest_ip: [u8; 4],
    ) -> Self {
        let mut header = Self {
            version_ihl: 0x45, // Version 4 and Header Length 5 (20 bytes)
            tos: 0,            // Not used
            total_length,
            identification,
            flags_fragment_offset: 0, // No fragmentation for now
            ttl,
            protocol,
            checksum: 0,
            source_ip,
            dest_ip,
        };
        header.checksum = header.calculate_checksum();
        header
    }

    pub fn from_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        let mut rdr = Cursor::new(bytes);
        let version_ihl = rdr.read_u8()?;
        let tos = rdr.read_u8()?;
        let total_length = rdr.read_u16::<BigEndian>()?;
        let identification = rdr.read_u16::<BigEndian>()?;
        let flags_fragment_offset = rdr.read_u16::<BigEndian>()?;
        let ttl = rdr.read_u8()?;
        let protocol = rdr.read_u8()?;
        let checksum = rdr.read_u16::<BigEndian>()?;
        let mut source_ip = [0u8; 4];
        rdr.read_exact(&mut source_ip)?;
        let mut dest_ip = [0u8; 4];
        rdr.read_exact(&mut dest_ip)?;

        Ok(Self {
            version_ihl,
            tos,
            total_length,
            identification,
            flags_fragment_offset,
            ttl,
            protocol,
            checksum,
            source_ip,
            dest_ip,
        })
    }

    pub fn to_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut wtr = Vec::new();
        wtr.write_u8(self.version_ihl)?;
        wtr.write_u8(self.tos)?;
        wtr.write_u16::<BigEndian>(self.total_length)?;
        wtr.write_u16::<BigEndian>(self.identification)?;
        wtr.write_u16::<BigEndian>(self.flags_fragment_offset)?;
        wtr.write_u8(self.ttl)?;
        wtr.write_u8(self.protocol)?;
        wtr.write_u16::<BigEndian>(self.checksum)?;
        wtr.write_all(&self.source_ip)?;
        wtr.write_all(&self.dest_ip)?;
        Ok(wtr)
    }

    pub fn calculate_checksum(&self) -> u16 {
        let mut temp_header = self.clone();
        temp_header.checksum = 0;
        // We can safely unwrap here because to_bytes only fails on IO errors with the writer,
        // and Vec<u8> writer doesn't fail on small writes.
        let bytes = temp_header
            .to_bytes()
            .unwrap();

        let mut sum = 0u32;
        for i in (0..bytes.len()).step_by(2) {
            let word = u16::from_be_bytes([bytes[i], bytes[i + 1]]);
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
    fn test_ipv4_header_serialization() {
        let header = Ipv4Header::new(
            20,
            12345,
            64,
            17, // UDP
            [192, 168, 1, 1],
            [192, 168, 1, 2],
        );

        let bytes = header.to_bytes().unwrap();
        assert_eq!(bytes.len(), 20);

        let deserialized = Ipv4Header::from_bytes(&bytes).unwrap();
        assert_eq!(header, deserialized);
        assert_eq!(header.checksum, deserialized.checksum);
    }
}
