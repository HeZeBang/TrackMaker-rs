/// PSK Receiver implementation
use crate::audio::psk::PskDemodulator;
use crate::transmission::{Frame, FrameConfig, FrameManager, PreambleManager, TextProcessor};
use std::path::Path;
use tracing::info;

/// Configuration for PSK receiver
pub struct ReceiverConfig {
    pub sample_rate: f32,
    pub carrier_freq: f32,
    pub symbol_rate: f32,
}

impl Default for ReceiverConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000.0,
            carrier_freq: 10000.0,
            symbol_rate: 1000.0,
        }
    }
}

/// Result of frame reception
#[derive(Debug)]
pub struct ReceptionResult {
    pub total_frames_detected: usize,
    pub correct_frames: usize,
    pub success_rate: f32,
    pub received_frames: Vec<Frame>,
}

impl ReceptionResult {
    pub fn new(total_detected: usize, correct: usize, frames: Vec<Frame>) -> Self {
        let success_rate = if total_detected > 0 {
            (correct as f32 / total_detected as f32) * 100.0
        } else {
            0.0
        };
        
        Self {
            total_frames_detected: total_detected,
            correct_frames: correct,
            success_rate,
            received_frames: frames,
        }
    }
}

/// PSK Receiver for receiving and decoding text data
pub struct PskReceiver {
    config: ReceiverConfig,
    demodulator: PskDemodulator,
    frame_manager: FrameManager,
    preamble_manager: PreambleManager,
    frame_config: FrameConfig,
}

impl PskReceiver {
    pub fn new(config: ReceiverConfig) -> Self {
        let demodulator = PskDemodulator::new(
            config.sample_rate,
            config.carrier_freq,
            config.symbol_rate,
        );
        
        let frame_config = FrameConfig::default();
        let frame_manager = FrameManager::new(frame_config.clone());
        
        let preamble_manager = PreambleManager::new(
            config.sample_rate,
            2000.0,  // Start at 2kHz
            10000.0, // End at 10kHz
            440,     // 440 samples duration
        );
        
        Self {
            config,
            demodulator,
            frame_manager,
            preamble_manager,
            frame_config,
        }
    }
    
    pub fn new_default() -> Self {
        Self::new(ReceiverConfig::default())
    }
    
    /// Process received signal and decode frames
    pub fn process_signal(&self, rx_signal: &[f32]) -> ReceptionResult {
        if rx_signal.is_empty() {
            info!("No signal received");
            return ReceptionResult::new(0, 0, Vec::new());
        }
        
        info!("Processing received signal with PSK demodulation...");
        
        let samples_per_symbol = (self.config.sample_rate / self.config.symbol_rate) as usize;
        let frame_length_samples = (self.frame_config.total_bits + self.frame_config.crc_bits) * samples_per_symbol;
        
        // Find frame starts using preamble correlation
        let frame_starts = self.preamble_manager.find_frame_starts(rx_signal, frame_length_samples);
        
        info!("Found {} potential frames", frame_starts.len());
        
        let mut received_frames = Vec::new();
        let mut correct_frame_count = 0;
        
        // Demodulate each detected frame
        for (frame_idx, &frame_start) in frame_starts.iter().enumerate() {
            let frame_end = frame_start + frame_length_samples;
            
            if frame_end <= rx_signal.len() {
                let frame_signal = &rx_signal[frame_start..frame_end];
                
                // Demodulate using PSK
                let demodulated_bits = self.demodulator.demodulate_bpsk(frame_signal);
                
                if let Some(frame) = Frame::from_bits(&demodulated_bits, &self.frame_config) {
                    received_frames.push(frame.clone());
                    info!("Frame {}: Correct, ID: {}", frame_idx + 1, frame.id);
                    correct_frame_count += 1;
                } else {
                    info!("Frame {}: Error in frame decoding", frame_idx + 1);
                }
            } else {
                info!("Frame {}: Signal too short for complete frame", frame_idx + 1);
            }
        }
        
        ReceptionResult::new(frame_starts.len(), correct_frame_count, received_frames)
    }
    
    /// Receive and decode text from signal
    pub fn receive_text(&self, rx_signal: &[f32]) -> Option<String> {
        let result = self.process_signal(rx_signal);
        
        info!("Total Correct Frames: {} / {}", result.correct_frames, result.total_frames_detected);
        
        if result.total_frames_detected > 0 {
            info!("Success Rate: {:.1}%", result.success_rate);
        }
        
        if result.received_frames.is_empty() {
            info!("No valid frames received");
            return None;
        }
        
        // Reconstruct text from received frames
        let reconstructed_bytes = self.frame_manager.reconstruct_data(result.received_frames);
        
        match String::from_utf8(reconstructed_bytes.clone()) {
            Ok(text) => {
                info!("=== RECEIVED TEXT ===");
                info!("{}", text);
                info!("=== END TEXT ===");
                Some(text)
            }
            Err(e) => {
                info!("Error converting to UTF-8: {}", e);
                info!("Raw bytes: {:?}", reconstructed_bytes);
                
                // Save raw bytes for debugging
                let tmp_dir = Path::new("tmp");
                let raw_file_path = tmp_dir.join("received_raw_bytes.bin");
                if let Err(e) = TextProcessor::save_raw_bytes(&reconstructed_bytes, &raw_file_path) {
                    info!("Failed to save raw bytes: {}", e);
                }
                
                None
            }
        }
    }
    
    /// Receive text and save with comparison
    pub fn receive_text_with_comparison(
        &self,
        rx_signal: &[f32],
        original_file_path: &str,
        output_dir: &Path,
    ) -> Option<String> {
        if let Some(received_text) = self.receive_text(rx_signal) {
            // Save received text and comparison
            match TextProcessor::save_received_text_with_comparison(
                &received_text,
                original_file_path,
                output_dir,
            ) {
                Ok(_comparison) => {
                    // Comparison results are already logged by TextProcessor
                }
                Err(e) => {
                    info!("Failed to save comparison files: {}", e);
                }
            }
            
            Some(received_text)
        } else {
            None
        }
    }
    
    /// Get the frame manager
    pub fn frame_manager(&self) -> &FrameManager {
        &self.frame_manager
    }
    
    /// Get the configuration
    pub fn config(&self) -> &ReceiverConfig {
        &self.config
    }
}
