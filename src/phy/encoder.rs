use super::frame::Frame;
use super::line_coding::{LineCode, LineCodingKind};
use tracing::{debug, info};

pub struct PhyEncoder {
    line_code: Box<dyn LineCode>,
    preamble: Vec<f32>,
}

impl PhyEncoder {
    /// Create a new physical layer encoder
    ///
    /// # Arguments
    /// * `samples_per_level` - Number of samples per Manchester level (not per bit)
    ///   For example, with 48000 Hz sample rate and 12000 bps bit rate:
    ///   - 对于曼彻斯特编码：samples_per_level = samples_per_level
    ///   - 对于 4B5B 编码：samples_per_level = 每个编码比特的采样数
    pub fn new(
        samples_per_level: usize,
        preamble_bytes: usize,
        line_coding_kind: LineCodingKind,
    ) -> Self {
        let line_code = line_coding_kind.create(samples_per_level);
        let preamble = line_code.generate_preamble(preamble_bytes);

        info!("PhyEncoder initialized:");
        info!("  - line coding: {}", line_coding_kind.name());
        info!("  - samples_per_level: {}", samples_per_level);
        info!(
            "  - preamble length: {} samples ({} bytes pattern)",
            preamble.len(),
            preamble_bytes
        );

        Self {
            line_code,
            preamble,
        }
    }

    /// Encode a frame into audio samples
    /// Returns: [Preamble] [Frame Data]
    pub fn encode_frame(&self, frame: &Frame) -> Vec<f32> {
        let frame_bits = frame.to_bits();
        let frame_samples = self
            .line_code
            .encode(&frame_bits);

        debug!(
            "Encoding frame: seq={}, data_len={}, total_bits={}, total_samples={}",
            frame.sequence,
            frame.data.len(),
            frame_bits.len(),
            self.preamble.len() + frame_samples.len()
        );

        let mut output =
            Vec::with_capacity(self.preamble.len() + frame_samples.len());
        output.extend_from_slice(&self.preamble);
        output.extend(frame_samples);

        output
    }

    /// Encode multiple frames with inter-frame gaps
    ///
    /// # Arguments
    /// * `frames` - Frames to encode
    /// * `inter_frame_gap_samples` - Number of silence samples between frames
    pub fn encode_frames(
        &self,
        frames: &[Frame],
        inter_frame_gap_samples: usize,
    ) -> Vec<f32> {
        let mut output = Vec::new();

        for (i, frame) in frames.iter().enumerate() {
            output.extend(self.encode_frame(frame));

            // Add inter-frame gap (except after last frame)
            if i < frames.len() - 1 {
                output.extend(vec![0.0; inter_frame_gap_samples]);
            }
        }

        debug!(
            "Encoded {} frames, total samples: {}",
            frames.len(),
            output.len()
        );
        output
    }

    /// Get preamble length in samples
    pub fn preamble_len(&self) -> usize {
        self.preamble.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phy::line_coding::LineCodingKind;

    #[test]
    fn test_encoder() {
        let encoder = PhyEncoder::new(2, 2, LineCodingKind::FourBFiveB);
        let frame = Frame::new_data(1, 1, 2, vec![0x12, 0x34, 0x56]);
        let samples = encoder.encode_frame(&frame);

        // Should have preamble + frame data
        assert!(samples.len() > encoder.preamble_len());
    }

    #[test]
    fn test_multiple_frames() {
        let encoder = PhyEncoder::new(2, 2, LineCodingKind::FourBFiveB);
        let frames = vec![
            Frame::new_data(0, 1, 2, vec![0x01, 0x02]),
            Frame::new_data(1, 1, 2, vec![0x03, 0x04]),
        ];

        let samples = encoder.encode_frames(&frames, 100);

        // Should have content
        assert!(samples.len() > 0);
    }
}
