// Physical layer decoder for Project 2
// Handles preamble detection, synchronization, and frame decoding

use super::frame::Frame;
use super::line_coding::{ManchesterDecoder, generate_preamble};
use tracing::{debug, warn};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq)]
enum DecoderState {
    Sync,      // Searching for preamble
    Decode,    // Decoding frame data
}

pub struct PhyDecoder {
    manchester: ManchesterDecoder,
    samples_per_level: usize,
    preamble: Vec<f32>,
    
    // State machine
    state: DecoderState,
    sync_fifo: VecDeque<f32>,
    decode_buffer: Vec<f32>,
    
    // Power tracking for carrier sensing
    power: f32,
    power_alpha: f32,  // Exponential averaging factor
    
    // Sync detection
    sync_threshold_factor: f32,
    sync_min_power: f32,
    sync_power_local_max: f32,
    potential_sync_index: usize,
    current_index: usize,
    // New: normalized correlation threshold for preamble detection
    correlation_threshold: f32,
    
    // Decoded frames
    decoded_frames: Vec<Frame>,
}

impl PhyDecoder {
    /// Create a new physical layer decoder
    /// 
    /// # Arguments
    /// * `samples_per_level` - Must match encoder's samples_per_level
    /// * `preamble_bytes` - Must match encoder's preamble_bytes
    pub fn new(samples_per_level: usize, preamble_bytes: usize) -> Self {
        let preamble = generate_preamble(samples_per_level, preamble_bytes);
        let preamble_len = preamble.len();
        
        Self {
            manchester: ManchesterDecoder::new(samples_per_level),
            samples_per_level,
            preamble,
            state: DecoderState::Sync,
            sync_fifo: VecDeque::with_capacity(preamble_len),
            decode_buffer: Vec::new(),
            power: 0.0,
            power_alpha: 1.0 / 64.0,  // Smoothing factor
            sync_threshold_factor: 2.0,  // sync_power must be > power * this
            sync_min_power: 0.05,
            sync_power_local_max: 0.0,
            potential_sync_index: 0,
            current_index: 0,
            correlation_threshold: 0.85, // strong match requirement
            decoded_frames: Vec::new(),
        }
    }

    /// Process incoming samples and extract frames
    /// Returns vector of successfully decoded frames
    pub fn process_samples(&mut self, samples: &[f32]) -> Vec<Frame> {
        self.decoded_frames.clear();
        
        for &sample in samples {
            self.current_index += 1;
            
            // Update running power estimate
            self.update_power(sample);
            
            match self.state {
                DecoderState::Sync => self.process_sync(sample),
                DecoderState::Decode => self.process_decode(sample),
            }
        }
        
        self.decoded_frames.clone()
    }

    /// Get current channel power (for carrier sensing)
    pub fn get_channel_power(&self) -> f32 {
        self.power
    }

    /// Check if channel is busy (for CSMA)
    /// threshold is the minimum power level to consider channel busy
    pub fn is_channel_busy(&self, threshold: f32) -> bool {
        self.power > threshold
    }

    /// Reset decoder state
    pub fn reset(&mut self) {
        self.state = DecoderState::Sync;
        self.sync_fifo.clear();
        self.decode_buffer.clear();
        self.power = 0.0;
        self.sync_power_local_max = 0.0;
        self.potential_sync_index = 0;
        self.current_index = 0;
        self.manchester.reset();
    }

    fn update_power(&mut self, sample: f32) {
        // Exponential moving average of power
        self.power = self.power * (1.0 - self.power_alpha) + 
                     sample * sample * self.power_alpha;
    }

    fn process_sync(&mut self, sample: f32) {
        // Maintain sliding window of preamble length
        if self.sync_fifo.len() >= self.preamble.len() {
            self.sync_fifo.pop_front();
        }
        self.sync_fifo.push_back(sample);

        if self.sync_fifo.len() == self.preamble.len() {
            // New: normalized cross-correlation detection
            let num: f32 = self.sync_fifo
                .iter()
                .zip(self.preamble.iter())
                .map(|(a, b)| a * b)
                .sum();
            let denom_a = self.sync_fifo.iter().map(|a| a * a).sum::<f32>().sqrt();
            let denom_b = self.preamble.iter().map(|b| b * b).sum::<f32>().sqrt();
            let corr = if denom_a > 0.0 && denom_b > 0.0 { num / (denom_a * denom_b) } else { 0.0 };
            if corr >= self.correlation_threshold {
                debug!("Preamble detected (corr={:.3})", corr);
                self.state = DecoderState::Decode;
                self.decode_buffer.clear();
                self.sync_fifo.clear();
                return;
            }

            // Calculate correlation with preamble
            let sync_power: f32 = self.sync_fifo
                .iter()
                .zip(self.preamble.iter())
                .map(|(a, b)| a * b)
                .sum::<f32>() / (self.preamble.len() as f32 * 0.5);  // Normalize

            // Check if this is a potential preamble detection
            if sync_power > self.power * self.sync_threshold_factor
                && sync_power > self.sync_power_local_max
                && sync_power > self.sync_min_power
            {
                self.sync_power_local_max = sync_power;
                self.potential_sync_index = self.current_index;
                debug!("Potential preamble at index {}, sync_power={:.3}, power={:.3}", 
                       self.potential_sync_index, sync_power, self.power);
            } else if self.current_index > self.potential_sync_index + self.preamble.len()
                && self.potential_sync_index != 0
            {
                // We've passed the peak, start decoding
                debug!("Preamble confirmed at index {}", self.potential_sync_index);
                self.state = DecoderState::Decode;
                self.decode_buffer.clear();
                self.sync_power_local_max = 0.0;
                
                // Collect samples after preamble that are already in fifo
                let offset = self.current_index - self.potential_sync_index;
                if offset < self.preamble.len() {
                    let skip = self.preamble.len() - offset;
                    if skip < self.sync_fifo.len() {
                        self.decode_buffer.extend(self.sync_fifo.iter().skip(skip));
                    }
                }
                self.sync_fifo.clear();
            }
        }
    }

    fn process_decode(&mut self, sample: f32) {
        self.decode_buffer.push(sample);

        // We need at least the minimum frame size
        // Minimum frame: type(1) + seq(1) + len(2) + crc(1) = 5 bytes = 40 bits
        // In Manchester: 40 bits * 2 levels * samples_per_level
        let min_frame_samples = 40 * 2 * self.samples_per_level;

        // Try to decode when we have at least minimum frame size
        // We'll try to decode progressively as we need to know the actual length
        if self.decode_buffer.len() >= min_frame_samples {
            // First, decode the header to get the length
            let header_bits_needed = 4 * 8;  // type + seq + len (2 bytes)
            let header_samples_needed = header_bits_needed * 2 * self.samples_per_level;
            
            if self.decode_buffer.len() >= header_samples_needed {
                let header_bits = self.manchester.decode(&self.decode_buffer[..header_samples_needed]);
                
                if header_bits.len() >= 32 {
                    // Parse length field (bytes 2-3)
                    let len_high = Self::bits_to_byte(&header_bits[16..24]);
                    let len_low = Self::bits_to_byte(&header_bits[24..32]);
                    let data_len = ((len_high as usize) << 8) | (len_low as usize);
                    
                    // Calculate total frame size
                    let total_frame_bytes = 4 + data_len + 1;  // header + data + crc
                    let total_frame_bits = total_frame_bytes * 8;
                    let total_frame_samples = total_frame_bits * 2 * self.samples_per_level;
                    
                    debug!("Frame header decoded: data_len={}, total_frame_samples={}", 
                           data_len, total_frame_samples);
                    
                    // Check if we have enough samples
                    if self.decode_buffer.len() >= total_frame_samples {
                        // Decode the entire frame
                        let frame_samples = &self.decode_buffer[..total_frame_samples];
                        let frame_bits = self.manchester.decode(frame_samples);
                        
                        // Try to parse frame
                        match Frame::from_bits(&frame_bits) {
                            Some(frame) => {
                                debug!("Frame decoded successfully: seq={}, type={:?}, data_len={}", 
                                       frame.sequence, frame.frame_type, frame.data.len());
                                self.decoded_frames.push(frame);
                            }
                            None => {
                                warn!("Frame CRC check failed or invalid format");
                            }
                        }
                        
                        // Reset to sync mode
                        self.state = DecoderState::Sync;
                        self.decode_buffer.clear();
                        self.potential_sync_index = 0;
                        return;
                    }
                }
            }
        }

        // Timeout: if decode buffer gets too large, something went wrong
        let max_frame_samples = 2048 * 2 * self.samples_per_level;  // Max ~2KB frame
        if self.decode_buffer.len() > max_frame_samples {
            warn!("Decode buffer overflow, resetting to sync");
            self.state = DecoderState::Sync;
            self.decode_buffer.clear();
            self.potential_sync_index = 0;
        }
    }

    fn bits_to_byte(bits: &[u8]) -> u8 {
        let mut byte = 0u8;
        for (i, &bit) in bits.iter().enumerate().take(8) {
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

    #[test]
    fn test_decoder() {
        let encoder = PhyEncoder::new(2, 2);
        let mut decoder = PhyDecoder::new(2, 2);
        
        let frame = Frame::new_data(1, vec![0x12, 0x34, 0x56, 0x78]);
        let samples = encoder.encode_frame(&frame);
        
        let decoded = decoder.process_samples(&samples);
        
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].sequence, 1);
        assert_eq!(decoded[0].data, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn test_multiple_frames() {
        let encoder = PhyEncoder::new(2, 2);
        let mut decoder = PhyDecoder::new(2, 2);
        
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
}
