// Manchester encoding: 0 -> [1, -1], 1 -> [-1, 1]
pub struct ManchesterEncoder {
    samples_per_level: usize, // Samples per level (not per bit)
}

impl ManchesterEncoder {
    pub fn new(samples_per_level: usize) -> Self {
        Self { samples_per_level }
    }

    /// slice of bits -> Manchester-encoded audio samples
    pub fn encode(&self, bits: &[u8]) -> Vec<f32> {
        let mut samples =
            Vec::with_capacity(bits.len() * 2 * self.samples_per_level);

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

    pub fn samples_for_bits(&self, num_bits: usize) -> usize {
        num_bits * 2 * self.samples_per_level
    }
}

pub struct ManchesterDecoder {
    samples_per_level: usize,
    sample_buffer: Vec<f32>,
    bit_clock: usize,
}

impl ManchesterDecoder {
    pub fn new(samples_per_level: usize) -> Self {
        Self {
            samples_per_level,
            sample_buffer: Vec::new(),
            bit_clock: 0,
        }
    }

    /// decoded to bits
    pub fn decode(&mut self, samples: &[f32]) -> Vec<u8> {
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

    /// reset decoder state
    pub fn reset(&mut self) {
        self.sample_buffer.clear();
        self.bit_clock = 0;
    }
}

/// Generate preamble for synchronization
/// Using alternating pattern for easy detection: 10101010... (0xAA pattern)
pub fn generate_preamble(
    samples_per_level: usize,
    pattern_bytes: usize,
) -> Vec<f32> {
    let encoder = ManchesterEncoder::new(samples_per_level);

    // Use alternating pattern: 0xAA = 10101010
    let mut bits = Vec::new();
    for _ in 0..pattern_bytes {
        bits.extend_from_slice(&[
            1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1,
        ]);
    }

    encoder.encode(&bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manchester_encoding_decoding() {
        let encoder = ManchesterEncoder::new(2);
        let mut decoder = ManchesterDecoder::new(2);

        let bits = vec![0, 1, 0, 1, 1, 0, 1, 0];
        let samples = encoder.encode(&bits);
        let decoded = decoder.decode(&samples);

        assert_eq!(bits, decoded);
    }

    #[test]
    fn test_preamble_generation() {
        let preamble = generate_preamble(2, 2);
        // 2 bytes * 8 bits * 2 levels * 2 samples_per_level = 64 samples
        assert_eq!(preamble.len(), 64);
    }
}
