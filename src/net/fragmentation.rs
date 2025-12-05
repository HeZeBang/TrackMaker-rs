use std::collections::HashMap;
use tracing::{debug, info};

/// Structure to hold fragmentation information
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FragmentationInfo {
    /// Identification number for this datagram
    pub identification: u16,
    /// Whether this is the last fragment
    pub more_fragments: bool,
    /// Fragment offset in 8-byte units
    pub fragment_offset: u16,
}

impl FragmentationInfo {
    /// Create a new fragmentation info
    pub fn new(
        identification: u16,
        more_fragments: bool,
        fragment_offset: u16,
    ) -> Self {
        Self {
            identification,
            more_fragments,
            fragment_offset,
        }
    }

    /// Encode to flags_fragment_offset field format (16 bits)
    /// Bits 0-2: Flags (Reserved, Don't Fragment, More Fragments)
    /// Bits 3-15: Fragment Offset (13 bits)
    pub fn to_u16(&self) -> u16 {
        let mut value: u16 = 0;

        // Set More Fragments flag (bit 13)
        if self.more_fragments {
            value |= 0x2000;
        }

        // Fragment offset is in the lower 13 bits
        value |= self.fragment_offset & 0x1FFF;

        value
    }

    /// Decode from flags_fragment_offset field
    pub fn from_u16(value: u16) -> Self {
        let more_fragments = (value & 0x2000) != 0;
        let fragment_offset = value & 0x1FFF;

        Self {
            identification: 0, // Must be set separately
            more_fragments,
            fragment_offset,
        }
    }
}

/// Fragmenter for splitting large IP packets into smaller fragments
pub struct IpFragmenter {
    mtu: usize,
    next_identification: u16,
}

impl IpFragmenter {
    /// Create a new fragmenter with the given MTU
    pub fn new(mtu: usize) -> Self {
        Self {
            mtu,
            next_identification: 0,
        }
    }

    /// Get the next identification number
    pub fn next_identification(&mut self) -> u16 {
        let id = self.next_identification;
        self.next_identification = self.next_identification.wrapping_add(1);
        id
    }

    /// Fragment an IP packet into smaller chunks
    ///
    /// # Arguments
    /// * `packet` - Complete IP packet (including header)
    /// * `mtu` - Maximum Transmission Unit
    ///
    /// # Returns
    /// Vector of fragments, each <= MTU in size
    pub fn fragment_packet(
        &mut self,
        packet: &[u8],
    ) -> Result<Vec<Vec<u8>>, String> {
        if packet.len() <= self.mtu {
            // No fragmentation needed
            debug!("Packet size {} <= MTU {}, no fragmentation", packet.len(), self.mtu);
            return Ok(vec![packet.to_vec()]);
        }

        debug!("Fragmenting packet of size {} (MTU={})", packet.len(), self.mtu);

        // Parse IP header (first 20 bytes minimum)
        if packet.len() < 20 {
            return Err("Invalid IP packet: too small for header".to_string());
        }

        let ip_header = &packet[..20];
        let data = &packet[20..];

        // Get version and IHL
        let version_ihl = ip_header[0];
        let ihl = (version_ihl & 0x0F) as usize * 4;

        if ihl < 20 || ihl > packet.len() {
            return Err("Invalid IP header length".to_string());
        }

        let header_with_options = &packet[..ihl];

        // Calculate maximum data per fragment (must be multiple of 8 bytes)
        let max_data_per_fragment = ((self.mtu - ihl) / 8) * 8;

        if max_data_per_fragment == 0 {
            return Err(
                "MTU too small for fragmentation (need at least 8 bytes of data)"
                    .to_string(),
            );
        }

        let identification = self.next_identification();
        let mut fragments = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            let chunk_size = std::cmp::min(max_data_per_fragment, data.len() - offset);
            let chunk = &data[offset..offset + chunk_size];

            // More fragments flag is set if there's more data after this fragment
            let more_fragments = offset + chunk_size < data.len();
            let fragment_offset = (offset / 8) as u16;

            // Build fragment header
            let mut fragment = Vec::new();
            fragment.extend_from_slice(&ip_header[..20]);

            // Update flags_fragment_offset field
            let mut flags_offset_bytes = [0u8; 2];
            let flags_offset_value = FragmentationInfo::new(
                identification,
                more_fragments,
                fragment_offset,
            )
            .to_u16();
            flags_offset_bytes[0] = (flags_offset_value >> 8) as u8;
            flags_offset_bytes[1] = flags_offset_value as u8;
            fragment[6..8].copy_from_slice(&flags_offset_bytes);

            // Update total length
            let fragment_total_length = ihl + chunk_size;
            fragment[2..4].copy_from_slice(&(fragment_total_length as u16).to_be_bytes());

            // Update identification
            fragment[4..6].copy_from_slice(&identification.to_be_bytes());

            // Zero the checksum field for recalculation
            fragment[10..12].copy_from_slice(&[0u8; 2]);

            // Recalculate checksum (simplified: just copy header)
            // In a real implementation, we'd compute the checksum properly
            // For now, we copy the original checksum (sender should recalculate)
            fragment[10..12].copy_from_slice(&ip_header[10..12]);

            // Add header options if present
            if ihl > 20 {
                fragment.extend_from_slice(&header_with_options[20..ihl]);
            }

            // Add data chunk
            fragment.extend_from_slice(chunk);

            fragments.push(fragment);
            offset += chunk_size;
        }

        debug!(
            "Fragmented packet into {} fragments (identification: {})",
            fragments.len(),
            identification
        );

        Ok(fragments)
    }
}

/// Reassembler for combining IP fragments back into a complete packet
pub struct IpReassembler {
    /// Map from (identification, source_ip) to fragments sorted by offset
    /// Each entry: (offset_in_8byte_units, payload_data)
    fragments: HashMap<(u16, [u8; 4]), Vec<(u16, Vec<u8>)>>,
    /// Map to track if we've seen the last fragment
    last_fragment_seen: HashMap<(u16, [u8; 4]), bool>,
    /// Store header for first fragment to use in reassembled packet
    headers: HashMap<(u16, [u8; 4]), Vec<u8>>,
}

impl IpReassembler {
    /// Create a new reassembler
    pub fn new() -> Self {
        Self {
            fragments: HashMap::new(),
            last_fragment_seen: HashMap::new(),
            headers: HashMap::new(),
        }
    }

    /// Process a received fragment
    ///
    /// # Arguments
    /// * `packet` - IP packet fragment
    ///
    /// # Returns
    /// If a complete packet is reassembled, returns `Some(packet)`, otherwise `None`
    pub fn process_fragment(
        &mut self,
        packet: &[u8],
    ) -> Result<Option<Vec<u8>>, String> {
        if packet.len() < 20 {
            return Err("Invalid IP packet fragment: too small for header".to_string());
        }

        // Parse IP header
        let version_ihl = packet[0];
        let ihl = (version_ihl & 0x0F) as usize * 4;

        if ihl < 20 || ihl > packet.len() {
            return Err("Invalid IP header length in fragment".to_string());
        }

        // Extract fragmentation info
        let flags_offset = u16::from_be_bytes([packet[6], packet[7]]);
        let frag_info = FragmentationInfo::from_u16(flags_offset);

        let identification = u16::from_be_bytes([packet[4], packet[5]]);
        let source_ip = [packet[12], packet[13], packet[14], packet[15]];

        let key = (identification, source_ip);

        // Check if this is a non-fragmented packet
        if !frag_info.more_fragments && frag_info.fragment_offset == 0 {
            // Single fragment packet (not fragmented)
            return Ok(Some(packet.to_vec()));
        }

        // Store header from first fragment (or any fragment)
        if !self.headers.contains_key(&key) {
            self.headers.insert(key, packet[..ihl].to_vec());
        }

        // Initialize fragment storage if needed
        if !self.fragments.contains_key(&key) {
            self.fragments.insert(key, Vec::new());
        }

        // Store fragment with its offset
        let payload = packet[ihl..].to_vec();
        let fragment_list = self.fragments.get_mut(&key).unwrap();
        fragment_list.push((frag_info.fragment_offset, payload.clone()));
        
        debug!(
            "Fragment received: id={}, offset={}, more={}, payload_len={}, total_fragments={}",
            identification,
            frag_info.fragment_offset,
            frag_info.more_fragments,
            payload.len(),
            fragment_list.len()
        );

        // Mark if this is the last fragment
        if !frag_info.more_fragments {
            self.last_fragment_seen.insert(key, true);
            debug!("Last fragment marked for id={}", identification);
        }

        // Check if we can reassemble
        if self.last_fragment_seen.get(&key).copied().unwrap_or(false) {
            let fragment_list = &self.fragments[&key];
            
            // Sort fragments by offset
            let mut sorted_fragments = fragment_list.clone();
            sorted_fragments.sort_by_key(|(offset, _)| *offset);

            debug!(
                "Checking reassembly: id={}, fragments={:?}",
                identification,
                sorted_fragments.iter().map(|(o, _)| o).collect::<Vec<_>>()
            );

            // Check if we have all fragments (no gaps in offsets)
            let mut expected_offset = 0u16;
            let mut complete = true;
            
            for (offset, payload) in &sorted_fragments {
                if *offset != expected_offset {
                    complete = false;
                    debug!(
                        "Gap detected in fragments: expected offset {}, got offset {}",
                        expected_offset, offset
                    );
                    break;
                }
                // Next expected offset is current offset + number of 8-byte units in this payload
                let payload_units = ((payload.len() + 7) / 8) as u16;
                expected_offset = offset + payload_units;
                
                debug!(
                    "Fragment ok: offset={}, payload_len={}, units={}, next_expected={}",
                    offset, payload.len(), payload_units, expected_offset
                );
            }

            if complete {
                // All fragments received and in order
                let mut reassembled = Vec::new();

                // Add header
                if let Some(header) = self.headers.get(&key) {
                    reassembled.extend_from_slice(header);
                }

                // Add all payloads in order
                for (_, payload) in &sorted_fragments {
                    reassembled.extend_from_slice(payload);
                }

                // Update IP header total_length
                let total_length = reassembled.len() as u16;
                if reassembled.len() >= 4 {
                    reassembled[2..4].copy_from_slice(&total_length.to_be_bytes());
                }

                // Clear fragmentation flags in reassembled packet
                if reassembled.len() >= 8 {
                    reassembled[6..8].copy_from_slice(&[0u8; 2]);
                }

                // Clean up stored fragments
                self.fragments.remove(&key);
                self.last_fragment_seen.remove(&key);
                self.headers.remove(&key);

                debug!(
                    "Reassembled {} fragments (identification: {}, total_size: {})",
                    sorted_fragments.len(), identification, reassembled.len()
                );

                return Ok(Some(reassembled));
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fragmentation_info_encode_decode() {
        let info = FragmentationInfo::new(12345, true, 100);
        let encoded = info.to_u16();

        // Check that more_fragments flag is set
        assert_eq!(encoded & 0x2000, 0x2000);

        // Check that fragment offset is preserved
        assert_eq!(encoded & 0x1FFF, 100);

        let decoded = FragmentationInfo::from_u16(encoded);
        assert_eq!(decoded.more_fragments, true);
        assert_eq!(decoded.fragment_offset, 100);
    }

    #[test]
    fn test_no_fragmentation_needed() {
        let mut fragmenter = IpFragmenter::new(500);

        // Create a small packet (less than MTU)
        let packet = vec![0u8; 100];
        let fragments = fragmenter.fragment_packet(&packet).unwrap();

        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0], packet);
    }

    #[test]
    fn test_fragmentation_basic() {
        let mut fragmenter = IpFragmenter::new(100);

        // Create a simple IP header + data
        let mut packet = vec![0x45u8; 20]; // IP version 4, IHL 5 (20 bytes header)
        packet.extend(vec![0x00u8; 300]); // 300 bytes of data

        let fragments = fragmenter.fragment_packet(&packet).unwrap();

        // Should be fragmented into multiple fragments
        assert!(fragments.len() > 1);

        // Each fragment should be <= MTU
        for frag in &fragments {
            assert!(frag.len() <= 100);
        }
    }

    #[test]
    fn test_reassembly() {
        let mut reassembler = IpReassembler::new();

        // Create two fragments
        // Fragment 1: offset 0, 40 bytes of data
        let mut frag1 = vec![
            0x45, 0x00, 0x00, 0x48, // IP header start (version, IHL, ToS, length = 20 + 40 = 72)
            0x12, 0x34, 0x20, 0x00, // Identification=0x1234, flags/offset (more_fragments=1, offset=0)
            0x40, 0x00, 0x00, 0x00, // TTL, protocol, checksum
            0xc0, 0xa8, 0x01, 0x01, // Source IP 192.168.1.1
            0xc0, 0xa8, 0x01, 0x02, // Dest IP 192.168.1.2
        ];
        frag1.extend(vec![0xAAu8; 40]); // Data: 40 bytes

        // Fragment 2: offset 1 (in 8-byte chunks, so at byte 8), remaining 10 bytes of data
        let mut frag2 = vec![
            0x45, 0x00, 0x00, 0x1E, // IP header start (length = 20 + 10 = 48)
            0x12, 0x34, 0x00, 0x01, // Identification=0x1234, flags/offset (more_fragments=0, offset=1)
            0x40, 0x00, 0x00, 0x00, // TTL, protocol, checksum
            0xc0, 0xa8, 0x01, 0x01, // Source IP
            0xc0, 0xa8, 0x01, 0x02, // Dest IP
        ];
        frag2.extend(vec![0xBBu8; 10]); // Data: 10 bytes

        // Process first fragment (no complete packet yet)
        let result1 = reassembler.process_fragment(&frag1).unwrap();
        assert_eq!(result1, None);

        // Process second fragment (should complete)
        let result2 = reassembler.process_fragment(&frag2).unwrap();
        assert!(result2.is_some());

        let reassembled = result2.unwrap();
        // Should contain all data: 40 + 10 = 50 bytes of data + 20 bytes header = 70 bytes
        assert_eq!(reassembled.len(), 70);
        assert_eq!(&reassembled[20..60], vec![0xAAu8; 40].as_slice());
        assert_eq!(&reassembled[60..70], vec![0xBBu8; 10].as_slice());
    }
}
