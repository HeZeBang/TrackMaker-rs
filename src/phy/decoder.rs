use super::frame::Frame;
use super::line_coding::{LineCode, LineCodingKind};
use crate::utils::consts::MAX_FRAME_DATA_SIZE;
use tracing::{debug, trace, warn};

pub struct PhyDecoder {
    line_code: Box<dyn LineCode>,
    preamble: Vec<f32>,

    // Correlation-based sync
    correlation_threshold: f32,
    preamble_energy: f32,

    // Sample buffer for processing
    sample_buffer: Vec<f32>,
    buffer_offset: usize, // Current processing position in buffer

    max_frame_bytes: usize,

    decoded_frames: Vec<Frame>,
}

enum DecodeFrameStatus {
    Success = 0,
    PreambleNotFound,
    CorrelationTooLow,
    NoEnoughSamples,
    HeaderDecodeFailed,
    InvalidDataLength,
    CRCFailed,
}

impl PhyDecoder {
    pub fn new(
        samples_per_level: usize,
        preamble_bytes: usize,
        line_coding_kind: LineCodingKind,
    ) -> Self {
        let line_code = line_coding_kind.create(samples_per_level);
        let preamble = line_code.generate_preamble(preamble_bytes);

        // for correlation normalization, this is pre-computed
        let preamble_energy: f32 = preamble
            .iter()
            .map(|x| x * x)
            .sum::<f32>()
            .sqrt();

        Self {
            line_code,
            preamble,
            // TODO: adjust threshold
            correlation_threshold: 0.9,
            preamble_energy,
            sample_buffer: Vec::new(),
            buffer_offset: 0,
            max_frame_bytes: MAX_FRAME_DATA_SIZE * 2, // 1x for encoder raw data + header + CRC...
            decoded_frames: Vec::new(),
        }
    }

    // entry point for processing incoming samples
    pub fn process_samples(&mut self, samples: &[f32]) -> Vec<Frame> {
        self.decoded_frames.clear();

        self.sample_buffer
            .extend_from_slice(samples);
        // simple sliding window processing
        while self.sample_buffer.len() > self.buffer_offset {
            // TODO: avoid splitted stream, make this dynamic
            let (status, frame_len) = self.try_decode_frame_at_offset();
            if let DecodeFrameStatus::Success = status {
                // Successfully decoded a frame
                self.buffer_offset += frame_len.unwrap();
                trace!("Advanced offset to {}", self.buffer_offset);
            } else if let DecodeFrameStatus::CorrelationTooLow = status {
                // No frame found
                self.buffer_offset += 1;
            } else {
                break; // Wait for more samples
            }
        }

        // Clean up processed samples (keep some overlap for preamble detection)
        let keep_samples = self.preamble.len() * 2;
        if self.buffer_offset > keep_samples {
            let drain_amount = self.buffer_offset - keep_samples;
            self.sample_buffer
                .drain(..drain_amount);
            self.buffer_offset = keep_samples;
        }

        self.decoded_frames.clone()
    }

    pub fn get_decoded_frames(&self) -> &Vec<Frame> {
        &self.decoded_frames
    }

    pub fn reset(&mut self) {
        self.sample_buffer.clear();
        self.buffer_offset = 0;
        self.line_code.reset();
    }

    /// Try to decode a frame starting at current buffer_offset
    /// Returns Some(frame_length_in_samples) if successful, None otherwise
    fn try_decode_frame_at_offset(
        &mut self,
    ) -> (DecodeFrameStatus, Option<usize>) {
        let remaining = &self.sample_buffer[self.buffer_offset..];

        // premature return if not enough samples for preamble
        if remaining.len() < self.preamble.len() {
            return (DecodeFrameStatus::NoEnoughSamples, None);
        }

        // Check for preamble using normalized cross-correlation
        let window = &remaining[..self.preamble.len()];
        let correlation = self.compute_normalized_correlation(window);
        if correlation < self.correlation_threshold {
            return (DecodeFrameStatus::CorrelationTooLow, None); // No preamble here
        }

        // Preamble detected /////////////////

        debug!(
            "Preamble detected at offset {} (corr={:.3})",
            self.buffer_offset, correlation
        );

        let frame_start = self.buffer_offset + self.preamble.len();

        // no data
        if frame_start >= self.sample_buffer.len() {
            return (DecodeFrameStatus::NoEnoughSamples, None);
        }

        let frame_samples = &self.sample_buffer[frame_start..];

        // Decode header to get frame length
        let header_bits = 32; // type(8) + seq(8) + len(16)
        let header_samples = self.line_code.samples_for_bits(header_bits);

        // premature return if not enough samples for header
        if frame_samples.len() < header_samples {
            return (DecodeFrameStatus::NoEnoughSamples, None);
        }

        let header_decoded = self
            .line_code
            .decode(&frame_samples[..header_samples]);

        if header_decoded.len() < header_bits {
            warn!("Failed to decode header at offset {}", self.buffer_offset);
            return (DecodeFrameStatus::HeaderDecodeFailed, None);
        }

        // Extract data length from header
        let len_high = Self::bits_to_byte(&header_decoded[16..24]);
        let len_low = Self::bits_to_byte(&header_decoded[24..32]);
        let data_len = ((len_high as usize) << 8) | (len_low as usize);

        // Validate data length
        if data_len == 0 || data_len > self.max_frame_bytes {
            warn!(
                "Invalid data_len={} at offset {}, skipping",
                data_len, self.buffer_offset
            );
            return (DecodeFrameStatus::InvalidDataLength, None);
        }

        // Calculate total frame size
        let total_bytes = 4 + data_len + 1; // header + data + crc
        let total_bits = total_bytes * 8;
        let total_samples = self.line_code.samples_for_bits(total_bits);

        if frame_samples.len() < total_samples {
            return (DecodeFrameStatus::NoEnoughSamples, None);
        }

        // Decode complete frame
        let frame_bits = self
            .line_code
            .decode(&frame_samples[..total_samples]);

        if frame_bits.len() < total_bits {
            warn!(
                "Line decode failed for frame at offset {}",
                self.buffer_offset
            );
            return (DecodeFrameStatus::HeaderDecodeFailed, None);
        }

        // Parse and validate frame
        match Frame::from_bits(&frame_bits) {
            Some(frame) => {
                debug!(
                    "✓ Frame decoded: seq={}, type={:?}, len={}",
                    frame.sequence,
                    frame.frame_type,
                    frame.data.len()
                );
                self.decoded_frames
                    .push(frame);

                // Return total length, including preamble
                (
                    DecodeFrameStatus::Success,
                    Some(self.preamble.len() + total_samples),
                )
            }
            None => {
                warn!("Frame CRC failed at offset {}", self.buffer_offset);
                (DecodeFrameStatus::CRCFailed, None)
            }
        }
    }

    /// Compute normalized cross-correlation between window and preamble
    /// some math
    fn compute_normalized_correlation(&self, window: &[f32]) -> f32 {
        if window.len() != self.preamble.len() {
            return 0.0;
        }

        let dot_product: f32 = window
            .iter()
            .zip(self.preamble.iter())
            .map(|(a, b)| a * b)
            .sum();

        let window_energy: f32 = window
            .iter()
            .map(|x| x * x)
            .sum::<f32>()
            .sqrt();

        if window_energy < 1e-6 || self.preamble_energy < 1e-6 {
            return 0.0;
        }

        dot_product / (window_energy * self.preamble_energy)
    }

    fn bits_to_byte(bits: &[u8]) -> u8 {
        let mut byte = 0u8;
        for (i, &bit) in bits
            .iter()
            .enumerate()
            .take(8)
        {
            if bit != 0 {
                byte |= 1 << (7 - i);
            }
        }
        byte
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phy::encoder::PhyEncoder;
    use crate::phy::line_coding::LineCodingKind;

    #[test]
    fn test_decoder() {
        let encoder = PhyEncoder::new(2, 2, LineCodingKind::FourBFiveB);
        let mut decoder = PhyDecoder::new(2, 2, LineCodingKind::FourBFiveB);

        let frame = Frame::new_data(1, vec![0x12, 0x34, 0x56, 0x78]);
        let samples = encoder.encode_frame(&frame);

        let decoded = decoder.process_samples(&samples);

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].sequence, 1);
        assert_eq!(decoded[0].data, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn test_multiple_frames() {
        let encoder = PhyEncoder::new(2, 2, LineCodingKind::FourBFiveB);
        let mut decoder = PhyDecoder::new(2, 2, LineCodingKind::FourBFiveB);

        let frames = vec![
            Frame::new_data(0, vec![0x01, 0x02]),
            Frame::new_data(1, vec![0x03, 0x04]),
            Frame::new_data(2, vec![0x05, 0x06]),
        ];

        let samples = encoder.encode_frames(&frames, 100);
        let decoded = decoder.process_samples(&samples);

        assert_eq!(decoded.len(), 3);
        for (i, frame) in decoded.iter().enumerate() {
            assert_eq!(frame.sequence, i as u8);
        }
    }

    #[test]
    fn test_noisy_channel() {
        let encoder = PhyEncoder::new(2, 2, LineCodingKind::FourBFiveB);
        let mut decoder = PhyDecoder::new(2, 2, LineCodingKind::FourBFiveB);

        let frame = Frame::new_data(0, vec![0xAA, 0xBB]);
        let mut samples = encoder.encode_frame(&frame);

        // NOISEEEEEEEEEEEEE
        for sample in samples.iter_mut() {
            *sample += (rand::random::<f32>() - 0.5) * 0.1;
        }

        let decoded = decoder.process_samples(&samples);

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].data, vec![0xAA, 0xBB]);
    }
}
