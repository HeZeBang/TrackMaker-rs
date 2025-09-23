/// Frame structure and management for PSK transmission
use crate::audio::psk::utils as psk_utils;

/// Configuration for frame structure
#[derive(Clone)]
pub struct FrameConfig {
    pub total_bits: usize,
    pub id_bits: usize,
    pub data_bits: usize,
    pub crc_bits: usize,
}

impl Default for FrameConfig {
    fn default() -> Self {
        Self {
            total_bits: 100,
            id_bits: 8,
            data_bits: 88,
            crc_bits: 8,
        }
    }
}

impl FrameConfig {
    pub fn bytes_per_frame(&self) -> usize {
        self.data_bits / 8
    }
}

/// A single frame with ID and data
#[derive(Clone, Debug)]
pub struct Frame {
    pub id: u8,
    pub data: Vec<u8>,
    pub bits: Vec<u8>,
}

impl Frame {
    /// Create a new frame with the given ID and data
    pub fn new(id: u8, data: Vec<u8>, config: &FrameConfig) -> Self {
        let mut frame_bits = vec![0u8; config.total_bits];
        
        // Set frame ID (first 8 bits)
        for bit_idx in 0..config.id_bits {
            frame_bits[bit_idx] = ((id >> (config.id_bits - 1 - bit_idx)) & 1) as u8;
        }
        
        // Add data bits
        for (byte_idx, &byte_value) in data.iter().enumerate() {
            let bit_start = config.id_bits + byte_idx * 8;
            for bit_idx in 0..8 {
                if bit_start + bit_idx < config.id_bits + config.data_bits {
                    frame_bits[bit_start + bit_idx] = ((byte_value >> (7 - bit_idx)) & 1) as u8;
                }
            }
        }
        
        Self {
            id,
            data,
            bits: frame_bits,
        }
    }
    
    /// Extract frame from demodulated bits
    pub fn from_bits(bits: &[u8], config: &FrameConfig) -> Option<Self> {
        if bits.len() < config.id_bits + config.data_bits {
            return None;
        }
        
        // Extract frame ID from first bits
        let mut frame_id = 0u8;
        for k in 0..config.id_bits {
            if bits[k] == 1 {
                frame_id += 1 << (config.id_bits - 1 - k);
            }
        }
        
        if frame_id == 0 {
            return None;
        }
        
        // Extract data bytes
        let mut data_bytes = Vec::new();
        let bytes_per_frame = config.bytes_per_frame();
        for byte_idx in 0..bytes_per_frame {
            let mut byte_value = 0u8;
            for bit_idx in 0..8 {
                let bit_pos = config.id_bits + byte_idx * 8 + bit_idx;
                if bit_pos < bits.len() && bits[bit_pos] == 1 {
                    byte_value |= 1 << (7 - bit_idx);
                }
            }
            data_bytes.push(byte_value);
        }
        
        Some(Self {
            id: frame_id,
            data: data_bytes,
            bits: bits.to_vec(),
        })
    }
    
    /// Get frame bits with CRC (placeholder implementation)
    pub fn get_bits_with_crc(&self) -> Vec<u8> {
        let mut bits_with_crc = self.bits.clone();
        bits_with_crc.extend_from_slice(&[0u8; 8]); // Add 8 CRC bits (placeholder)
        bits_with_crc
    }
}

/// Frame manager for splitting data into frames and reconstructing
pub struct FrameManager {
    config: FrameConfig,
}

impl FrameManager {
    pub fn new(config: FrameConfig) -> Self {
        Self { config }
    }
    
    pub fn new_default() -> Self {
        Self::new(FrameConfig::default())
    }
    
    /// Split text bytes into frames
    pub fn create_frames(&self, data: &[u8]) -> Vec<Frame> {
        let bytes_per_frame = self.config.bytes_per_frame();
        let total_frames = (data.len() + bytes_per_frame - 1) / bytes_per_frame;
        let mut frames = Vec::new();
        
        for frame_idx in 0..total_frames {
            let start_byte = frame_idx * bytes_per_frame;
            let end_byte = std::cmp::min(start_byte + bytes_per_frame, data.len());
            
            let frame_data = data[start_byte..end_byte].to_vec();
            let frame_id = (frame_idx + 1) as u8;
            
            frames.push(Frame::new(frame_id, frame_data, &self.config));
        }
        
        frames
    }
    
    /// Reconstruct data from received frames
    pub fn reconstruct_data(&self, mut frames: Vec<Frame>) -> Vec<u8> {
        // Sort frames by ID
        frames.sort_by_key(|f| f.id);
        
        let mut reconstructed_data = Vec::new();
        
        for frame in frames {
            // Add non-zero bytes to reconstructed data
            for &byte in &frame.data {
                if byte != 0 { // Stop at null bytes (padding)
                    reconstructed_data.push(byte);
                } else {
                    break;
                }
            }
        }
        
        reconstructed_data
    }
    
    /// Get the configuration
    pub fn config(&self) -> &FrameConfig {
        &self.config
    }
}

/// Preamble generator and detector
pub struct PreambleManager {
    sample_rate: f32,
    start_freq: f32,
    end_freq: f32,
    duration_samples: usize,
}

impl PreambleManager {
    pub fn new(sample_rate: f32, start_freq: f32, end_freq: f32, duration_samples: usize) -> Self {
        Self {
            sample_rate,
            start_freq,
            end_freq,
            duration_samples,
        }
    }
    
    /// Generate chirp preamble for synchronization
    pub fn generate_preamble(&self) -> Vec<f32> {
        psk_utils::generate_chirp_preamble(
            self.sample_rate,
            self.start_freq,
            self.end_freq,
            self.duration_samples,
        )
    }
    
    /// Find frame starts using cross-correlation
    pub fn find_frame_starts(&self, signal: &[f32], frame_length_samples: usize) -> Vec<usize> {
        let preamble = self.generate_preamble();
        let correlation = psk_utils::cross_correlate(signal, &preamble);
        
        // Find correlation peaks above threshold
        let correlation_threshold = correlation.iter().fold(0.0f32, |acc, &x| acc.max(x)) * 0.3;
        
        let mut frame_starts = Vec::new();
        let mut i = 0;
        while i < correlation.len() {
            if correlation[i] > correlation_threshold {
                // Found a potential frame start
                frame_starts.push(i + self.duration_samples); // Frame starts after preamble
                
                // Skip ahead to avoid detecting the same frame multiple times
                i += frame_length_samples;
            } else {
                i += 1;
            }
        }
        
        frame_starts
    }
    
    pub fn preamble_length(&self) -> usize {
        self.duration_samples
    }
}

impl Default for PreambleManager {
    fn default() -> Self {
        Self::new(
            48000.0, // Default sample rate
            2000.0,  // Start at 2kHz
            10000.0, // End at 10kHz
            440,     // 440 samples duration
        )
    }
}
