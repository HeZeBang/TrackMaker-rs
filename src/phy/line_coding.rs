use std::fmt;

use tracing::{debug, warn};

/// Trait for line coding
pub trait LineCode: Send {
    fn encode(&self, bits: &[u8]) -> Vec<f32>;

    fn decode(&self, samples: &[f32]) -> Vec<u8>;

    fn samples_for_bits(&self, num_bits: usize) -> usize;

    fn generate_preamble(&self, pattern_bytes: usize) -> Vec<f32> {
        let mut bits = Vec::with_capacity(pattern_bytes * 8);
        for _ in 0..pattern_bytes {
            // 0xAA pattern: 10101010, 4B5B uses 0x33 pattern: 00110011
            bits.extend_from_slice(&[0,0,1,1,0,0,1,1]);
        }
        self.encode(&bits)
    }

    fn reset(&mut self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineCodingKind {
    Manchester,
    FourBFiveB,
}

impl LineCodingKind {
    pub fn name(self) -> &'static str {
        match self {
            LineCodingKind::Manchester => "Manchester",
            LineCodingKind::FourBFiveB => "4B5B",
        }
    }

    pub fn create(self, samples_per_level: usize) -> Box<dyn LineCode> {
        match self {
            LineCodingKind::Manchester => Box::new(ManchesterCodec::new(samples_per_level)),
            LineCodingKind::FourBFiveB => {
                Box::new(FourBFiveBCodec::new(samples_per_level))
            }
        }
    }
}

// Add Display implementation
impl fmt::Display for LineCodingKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

// ============================================================================
// Manchester
// ============================================================================
/// Manchester: 0 -> [1, -1]，1 -> [-1, 1]
pub struct ManchesterCodec {
    samples_per_level: usize,
}

impl ManchesterCodec {
    pub fn new(samples_per_level: usize) -> Self {
        Self { samples_per_level }
    }
}

impl LineCode for ManchesterCodec {
    fn encode(&self, bits: &[u8]) -> Vec<f32> {
        let mut samples =
            Vec::with_capacity(bits.len() * self.samples_per_level * 2);

        for &bit in bits {
            if bit == 0 {
                // 0 -> high then low
                samples.extend(vec![1.0; self.samples_per_level]);
                samples.extend(vec![-1.0; self.samples_per_level]);
            } else {
                // 1 -> low then high
                samples.extend(vec![-1.0; self.samples_per_level]);
                samples.extend(vec![1.0; self.samples_per_level]);
            }
        }

        samples
    }

    fn decode(&self, samples: &[f32]) -> Vec<u8> {
        let samples_per_bit = self.samples_per_level * 2;
        // TODO: handle leftover samples from previous calls, for now we assume samples are aligned
        let num_complete_bits = samples.len() / samples_per_bit;
        let mut bits = Vec::with_capacity(num_complete_bits);

        for i in 0..num_complete_bits {
            let start = i * samples_per_bit;
            let mid = start + self.samples_per_level;
            let end = start + samples_per_bit;

            // Calculate average of first half and second half
            let first_half: f32 = samples[start..mid]
                .iter()
                .sum::<f32>()
                / self.samples_per_level as f32;
            let second_half: f32 = samples[mid..end]
                .iter()
                .sum::<f32>()
                / self.samples_per_level as f32;

            // if first_half > second_half, 0, else 1
            if first_half > second_half {
                bits.push(0);
            } else {
                bits.push(1);
            }
        }

        bits
    }

    fn samples_for_bits(&self, num_bits: usize) -> usize {
        num_bits * self.samples_per_level * 2
    }

    fn reset(&mut self) {
        // Manchester is stateless
    }
}

// ============================================================================
// 4B5B
// ============================================================================

const FOURB_FIVEB_ENCODE_TABLE: [u8; 16] = [
    0b11110, // 0x0
    0b01001, // 0x1
    0b10100, // 0x2
    0b10101, // 0x3
    0b01010, // 0x4
    0b01011, // 0x5
    0b01110, // 0x6
    0b01111, // 0x7
    0b10010, // 0x8
    0b10011, // 0x9
    0b10110, // 0xA
    0b10111, // 0xB
    0b11010, // 0xC
    0b11011, // 0xD
    0b11100, // 0xE
    0b11101, // 0xF
];

fn decode_4b5b_symbol(symbol: u8) -> Option<u8> {
    match symbol {
        0b11110 => Some(0x0),
        0b01001 => Some(0x1),
        0b10100 => Some(0x2),
        0b10101 => Some(0x3),
        0b01010 => Some(0x4),
        0b01011 => Some(0x5),
        0b01110 => Some(0x6),
        0b01111 => Some(0x7),
        0b10010 => Some(0x8),
        0b10011 => Some(0x9),
        0b10110 => Some(0xA),
        0b10111 => Some(0xB),
        0b11010 => Some(0xC),
        0b11011 => Some(0xD),
        0b11100 => Some(0xE),
        0b11101 => Some(0xF),
        _ => {debug!("Warning: invalid 4B/5B symbol {:05b}", symbol); None }
    }
}

pub struct FourBFiveBCodec {
    samples_per_level: usize,
    // For NRZI encoding
    last_level: f32,
    // For NRZI decoding
    prev_level_avg: f32,
}

impl FourBFiveBCodec {
    pub fn new(samples_per_level: usize) -> Self {
        Self {
            samples_per_level,
            last_level: 1.0, // Start with high level for NRZI
            prev_level_avg: 1.0, // Start with high level for NRZI
        }
    }
}

impl LineCode for FourBFiveBCodec {
    /// Encode bits using 4B/5B and NRZI.
    fn encode(&self, bits: &[u8]) -> Vec<f32> {
        // 1. Group bits into 4-bit nibbles
        let num_nibbles = (bits.len() + 3) / 4;
        let mut encoded_bits = Vec::with_capacity(num_nibbles * 5);

        for i in 0..num_nibbles {
            let start = i * 4;
            let end = (start + 4).min(bits.len());
            let mut nibble = 0u8;
            for j in 0..(end - start) {
                if bits[start + j] != 0 {
                    nibble |= 1 << (3 - j);
                }
            }

            // 2. Encode 4B to 5B
            let symbol = FOURB_FIVEB_ENCODE_TABLE[nibble as usize];

            for j in 0..5 {
                encoded_bits.push((symbol >> (4 - j)) & 1);
            }
        }

        // 3. Apply NRZI encoding
        let mut samples =
            Vec::with_capacity(encoded_bits.len() * self.samples_per_level);
        let mut current_level = self.last_level;

        for bit in encoded_bits {
            if bit == 1 {
                // '1' inverts the level
                current_level = -current_level;
            }
            // '0' keeps the level
            samples.extend(vec![current_level; self.samples_per_level]);
        }

        samples
    }

    /// Decode samples using NRZI and 4B/5B.
    fn decode(&self, samples: &[f32]) -> Vec<u8> {
        if samples.is_empty() {
            return Vec::new();
        }

        // 1. NRZI Decode: Detect level changes
        let num_symbols = samples.len() / self.samples_per_level;
        let mut five_b_bits = Vec::with_capacity(num_symbols);
        let mut last_avg = self.prev_level_avg;

        for i in 0..num_symbols {
            let start = i * self.samples_per_level;
            let end = start + self.samples_per_level;
            let current_avg: f32 = samples[start..end].iter().sum::<f32>()
                / self.samples_per_level as f32;

            // Transition (change of sign) means '1', no transition means '0'
            if last_avg * current_avg < 0.0 {
                five_b_bits.push(1);
            } else {
                five_b_bits.push(0);
            }
            // Avoid last_avg being zero
            if current_avg.abs() > 1e-6 {
                last_avg = current_avg;
            }
        }

        // 2. 5B/4B Decode
        let num_nibbles = five_b_bits.len() / 5;
        let mut decoded_bits = Vec::with_capacity(num_nibbles * 4);

        for i in 0..num_nibbles {
            let start = i * 5;
            let mut symbol = 0u8;
            for j in 0..5 {
                symbol |= five_b_bits[start + j] << (4 - j);
            }

            if let Some(nibble) = decode_4b5b_symbol(symbol) {
                for j in 0..4 {
                    decoded_bits.push((nibble >> (3 - j)) & 1);
                }
            } else {
                // Error handling: if an invalid symbol is found, we might stop or fill with errors.
                // For now, we stop to avoid propagating errors.
                warn!("Decoding stopped due to invalid 4B/5B symbol.");
                break;
            }
        }

        decoded_bits
    }

    fn samples_for_bits(&self, num_bits: usize) -> usize {
        // num_bits -> num_nibbles -> num_5b_symbols -> num_samples
        let num_nibbles = (num_bits + 3) / 4;
        let num_5b_symbols = num_nibbles * 5;
        num_5b_symbols * self.samples_per_level
    }

    fn reset(&mut self) {
        self.last_level = 1.0;
        self.prev_level_avg = 1.0;
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manchester_encoding_decoding() {
        let codec = ManchesterCodec::new(2);
        let bits = vec![0, 1, 0, 1, 1, 0, 1, 0];
        let samples = codec.encode(&bits);
        let decoded = codec.decode(&samples);

        assert_eq!(bits, decoded);
    }

    #[test]
    fn test_manchester_preamble_generation() {
        let codec = ManchesterCodec::new(2);
        let preamble = codec.generate_preamble(2);
        // 2 bytes * 8 bits/byte * 2 levels/bit * 2 samples_per_level = 64 samples
        assert_eq!(preamble.len(), 64);
    }

    #[test]
    fn test_4b5b_encoding_decoding() {
        let mut codec = FourBFiveBCodec::new(4);
        let bits = vec![1, 0, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 1]; // 0xA70F
        let samples = codec.encode(&bits);
        let decoded = codec.decode(&samples);

        assert_eq!(bits, decoded);
    }

    #[test]
    fn test_4b5b_preamble_length() {
        let codec = FourBFiveBCodec::new(4);
        let preamble = codec.generate_preamble(2);
        // 2 bytes * 8 bits/byte = 16 bits
        // 16 bits / 4 bits/nibble = 4 nibbles
        // 4 nibbles * 5 encoded_bits/nibble = 20 encoded bits
        // 20 encoded bits * 4 samples_per_level = 80 samples
        assert_eq!(preamble.len(), 80);
    }
}
