use super::frame::Frame;
use super::line_coding::{LineCode, LineCodingKind};
use crate::mac;
use crate::phy::FrameType;
use crate::utils::consts::{MAX_FRAME_DATA_SIZE, PHY_HEADER_BYTES};
use tracing::{debug, trace, warn};

enum DecoderState {
    Searching,
    Decoding(usize), // Stores the start of a potential frame
}

pub struct PhyDecoder {
    line_code: Box<dyn LineCode>,
    preamble: Vec<f32>,
    state: DecoderState,

    // Correlation-based sync
    correlation_threshold: f32,
    preamble_energy: f32,

    // Sample buffer for processing
    sample_buffer: Vec<f32>,
    buffer_offset: usize, // Current processing position in buffer

    max_frame_bytes: usize,

    decoded_frames: Vec<Frame>,
    local_addr: mac::types::MacAddr,
}

impl PhyDecoder {
    pub fn new(
        samples_per_level: usize,
        preamble_bytes: usize,
        line_coding_kind: LineCodingKind,
        local_addr: mac::types::MacAddr,
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
            state: DecoderState::Searching,
            // TODO: adjust threshold
            correlation_threshold: 0.9, // Increased threshold
            preamble_energy,
            sample_buffer: Vec::new(),
            buffer_offset: 0,
            max_frame_bytes: MAX_FRAME_DATA_SIZE * 2, // 1x for encoder raw data + header + CRC...
            decoded_frames: Vec::new(),
            local_addr,
        }
    }

    // entry point for processing incoming samples
    pub fn process_samples(&mut self, samples: &[f32]) -> Vec<Frame> {
        self.decoded_frames.clear();
        self.sample_buffer
            .extend_from_slice(samples);

        loop {
            let processed_len = match self.state {
                DecoderState::Searching => self.search_for_preamble(),
                DecoderState::Decoding(frame_start_offset) => {
                    self.decode_frame(frame_start_offset)
                }
            };

            if let Some(len) = processed_len {
                self.buffer_offset += len;
            } else {
                // Not enough data to continue, break the loop
                break;
            }
        }

        // Clean up processed part of the buffer
        if self.buffer_offset > 0 {
            let keep_overlap = self
                .preamble
                .len()
                .saturating_sub(1);
            let drain_end = self
                .buffer_offset
                .saturating_sub(keep_overlap);

            if drain_end > 0 {
                self.sample_buffer
                    .drain(..drain_end);
                self.buffer_offset -= drain_end;

                // Adjust decoding offset if it's active
                if let DecoderState::Decoding(start) = &mut self.state {
                    *start = start.saturating_sub(drain_end);
                }
            }
        }

        self.decoded_frames.clone()
    }

    pub fn reset(&mut self) {
        self.sample_buffer.clear();
        self.buffer_offset = 0;
        self.state = DecoderState::Searching;
        self.line_code.reset();
    }

    /// Scans the buffer for a preamble.
    /// Returns Some(bytes_consumed) or None if more data is needed.
    fn search_for_preamble(&mut self) -> Option<usize> {
        let search_area = &self.sample_buffer[self.buffer_offset..];
        if search_area.len() < self.preamble.len() {
            return None; // Not enough data to search
        }

        let window_count = search_area.len() - self.preamble.len() + 1;

        for i in 0..window_count {
            let window = &search_area[i..i + self.preamble.len()];
            let correlation = self.compute_normalized_correlation(window);

            if correlation >= self.correlation_threshold {
                debug!(
                    "Preamble detected at offset {} (relative: {}) (corr={:.3})",
                    self.buffer_offset + i,
                    i,
                    correlation
                );
                // Preamble found, switch to decoding state
                let frame_start_offset =
                    self.buffer_offset + i + self.preamble.len();
                self.state = DecoderState::Decoding(frame_start_offset);
                // Consume buffer up to the start of the preamble
                return Some(i);
            }
        }

        // No preamble found in the searched area. Consume the searched part.
        Some(window_count)
    }

    /// Tries to decode a full frame from the buffer.
    /// Returns Some(bytes_consumed) or None if more data is needed.
    fn decode_frame(&mut self, frame_start_offset: usize) -> Option<usize> {
        // The number of samples consumed *before* this attempt is the start of the preamble.
        // The preamble itself has been consumed.
        let preamble_start_offset = frame_start_offset - self.preamble.len();

        // Not enough data for even the header
        let header_bits = 8 * PHY_HEADER_BYTES;
        let header_samples = self
            .line_code
            .samples_for_bits(header_bits);
        if self.sample_buffer.len() < frame_start_offset + header_samples {
            return None; // Need more data
        }

        // Decode header
        let header_data = &self.sample_buffer
            [frame_start_offset..frame_start_offset + header_samples];
        let header_decoded = self
            .line_code
            .decode(header_data);

        let (data_len_, _crc, data_type, _seq, _src, dst) =
            match Frame::parse_header(&header_decoded) {
                Some(vals) => vals,
                None => {
                    warn!(
                        "Failed to parse header at offset {}. Returning to search.",
                        preamble_start_offset
                    );
                    self.state = DecoderState::Searching;
                    return Some(header_samples); // Consume 1 sample to avoid getting stuck
                }
            };
        let data_len = data_len_ as usize;

        if data_type == FrameType::Data && data_len == 0
            || data_len > self.max_frame_bytes
        {
            warn!(
                "Invalid data_len={} at offset {}. Returning to search.",
                data_len, preamble_start_offset
            );
            self.state = DecoderState::Searching;
            return Some(1); // Consume 1 sample
        }

        // Check if we have enough data for the full frame
        let total_bytes = PHY_HEADER_BYTES + data_len; // header + data + crc
        let total_bits = total_bytes * 8;
        let total_samples = self
            .line_code
            .samples_for_bits(total_bits);

        if self.sample_buffer.len() < frame_start_offset + total_samples {
            return None; // Need more data
        }

        // Decode and parse the full frame
        let frame_data = &self.sample_buffer
            [frame_start_offset..frame_start_offset + total_samples];
        let frame_bits = self
            .line_code
            .decode(frame_data);

        let consumed_len = self.preamble.len()
            + self
                .line_code
                .samples_for_bits(frame_bits.len());

        if frame_bits.len() < total_bits {
            warn!(
                "Line decode failed for frame at offset {}. Returning to search.",
                preamble_start_offset
            );
            self.state = DecoderState::Searching;
            return Some(consumed_len);
        }

        if dst != self.local_addr {
            debug!(
                "Frame not for us (dst={}). Ignoring and returning to search.",
                dst
            );
            self.state = DecoderState::Searching;
            return Some(consumed_len);
        }

        match Frame::from_bits(&frame_bits) {
            Some(frame) => {
                debug!(
                    "✓ Frame decoded: seq={}, type={:?}, len={}, src={}, dst={}",
                    frame.sequence,
                    frame.frame_type,
                    frame.data.len(),
                    frame.src,
                    frame.dst
                );
                self.decoded_frames
                    .push(frame);
                self.state = DecoderState::Searching; // Go back to searching for the next frame
                Some(consumed_len)
            }
            None => {
                warn!(
                    "Frame CRC failed at offset {}. Returning to search.",
                    preamble_start_offset
                );
                self.state = DecoderState::Searching;
                // Consume the failed frame to move on
                Some(consumed_len)
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
}
