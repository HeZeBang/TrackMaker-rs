use super::frame::Frame;
use super::line_coding::{LineCode, LineCodingKind};
use crate::mac;
use crate::phy::FrameType;
use crate::utils::consts::{MAX_FRAME_DATA_SIZE, PHY_HEADER_BYTES};
use tracing::{debug, trace, warn};

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

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

        let preamble_len = self.preamble.len();
        let window_count = search_area.len() - preamble_len + 1;

        // Calculate initial energy
        let mut window_energy: f32 = search_area[0..preamble_len]
            .iter()
            .map(|x| x * x)
            .sum();

        for i in 0..window_count {
            let window = &search_area[i..i + preamble_len];

            // Optimization: Skip dot product if energy is too low
            let correlation = if window_energy < 1e-6 {
                0.0
            } else {
                let dot_product = self.compute_dot_product(window);
                dot_product / (window_energy.sqrt() * self.preamble_energy)
            };

            if correlation >= self.correlation_threshold {
                debug!(
                    "Preamble detected at offset {} (relative: {}) (corr={:.3})",
                    self.buffer_offset + i,
                    i,
                    correlation
                );

                // Refine alignment by searching for the Sync Word (last byte: 0x5A)
                // The Sync Word is at the end of the preamble.
                let sync_bits = 8;
                let sync_len = self
                    .line_code
                    .samples_for_bits(sync_bits);
                let sync_pattern =
                    &self.preamble[self.preamble.len() - sync_len..];

                // Calculate Sync Pattern Energy for normalization
                let sync_energy: f32 = sync_pattern
                    .iter()
                    .map(|x| x * x)
                    .sum::<f32>()
                    .sqrt();

                // Search window: +/- 1 bit width
                let search_margin = self
                    .line_code
                    .samples_for_bits(1);
                let expected_start = i + self.preamble.len() - sync_len;

                let start_search = expected_start.saturating_sub(search_margin);
                let end_search = (expected_start + search_margin)
                    .min(search_area.len() - sync_len);

                let mut best_corr = -1.0;
                let mut best_offset = expected_start;

                for j in start_search..=end_search {
                    let window = &search_area[j..j + sync_len];

                    let mut dot = 0.0;
                    let mut win_energy = 0.0;
                    for (w, p) in window
                        .iter()
                        .zip(sync_pattern.iter())
                    {
                        dot += w * p;
                        win_energy += w * w;
                    }
                    let corr = if win_energy > 1e-6 && sync_energy > 1e-6 {
                        dot / (win_energy.sqrt() * sync_energy)
                    } else {
                        0.0
                    };

                    if corr > best_corr {
                        best_corr = corr;
                        best_offset = j;
                    }
                }

                debug!(
                    "Refined alignment: {} -> {} (corr: {:.3})",
                    expected_start, best_offset, best_corr
                );

                // Preamble found, switch to decoding state
                let frame_start_offset =
                    self.buffer_offset + best_offset + sync_len;
                self.state = DecoderState::Decoding(frame_start_offset);
                // Consume buffer up to the start of the preamble
                return Some(i);
            }

            // Update energy for next iteration
            if i + 1 < window_count {
                let leaving = search_area[i];
                let entering = search_area[i + preamble_len];
                window_energy =
                    window_energy - leaving * leaving + entering * entering;
                // Prevent negative energy due to floating point errors
                if window_energy < 0.0 {
                    window_energy = 0.0;
                }
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
                "Line decode failed for frame(last valid {}/{}). Consumed {} samples",
                frame_bits.len(),
                total_bits,
                consumed_len
            );
            self.state = DecoderState::Searching;
            return Some(consumed_len);
        }

        if dst != self.local_addr {
            debug!(
                "Frame not for us (dst={}, type={:?}). Consumed {} samples",
                dst, data_type, consumed_len
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

    fn compute_dot_product(&self, window: &[f32]) -> f32 {
        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("avx") {
                unsafe { self.compute_dot_product_avx(window) }
            } else {
                self.compute_dot_product_scalar(window)
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            self.compute_dot_product_scalar(window)
        }
    }

    fn compute_dot_product_scalar(&self, window: &[f32]) -> f32 {
        window
            .iter()
            .zip(self.preamble.iter())
            .map(|(w, p)| w * p)
            .sum()
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx")]
    unsafe fn compute_dot_product_avx(&self, window: &[f32]) -> f32 {
        unsafe {
            let len = window.len();
            let preamble = &self.preamble;

            let mut dot_vec = _mm256_setzero_ps();

            let mut i = 0;
            while i + 8 <= len {
                let w = _mm256_loadu_ps(window.as_ptr().add(i));
                let p = _mm256_loadu_ps(preamble.as_ptr().add(i));

                dot_vec = _mm256_add_ps(dot_vec, _mm256_mul_ps(w, p));

                i += 8;
            }

            let dot_low = _mm256_castps256_ps128(dot_vec);
            let dot_high = _mm256_extractf128_ps(dot_vec, 1);
            let dot_128 = _mm_add_ps(dot_low, dot_high);

            let mut dot_arr = [0.0f32; 4];
            _mm_storeu_ps(dot_arr.as_mut_ptr(), dot_128);
            let mut dot_sum: f32 = dot_arr.iter().sum();

            while i < len {
                let w = *window.get_unchecked(i);
                let p = *preamble.get_unchecked(i);
                dot_sum += w * p;
                i += 1;
            }

            dot_sum
        }
    }
}
