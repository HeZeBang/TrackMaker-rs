/// PSK Sender implementation
use crate::audio::psk::PskModulator;
use crate::transmission::{Frame, FrameManager, PreambleManager, TextProcessor};
use rand::Rng;
use tracing::info;

/// Configuration for PSK sender
pub struct SenderConfig {
    pub sample_rate: f32,
    pub carrier_freq: f32,
    pub symbol_rate: f32,
    pub inter_frame_spacing_range: (usize, usize),
}

impl Default for SenderConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000.0,
            carrier_freq: 10000.0,
            symbol_rate: 1000.0,
            inter_frame_spacing_range: (0, 100),
        }
    }
}

/// PSK Sender for transmitting text data
pub struct PskSender {
    config: SenderConfig,
    modulator: PskModulator,
    frame_manager: FrameManager,
    preamble_manager: PreambleManager,
}

impl PskSender {
    pub fn new(config: SenderConfig) -> Self {
        let modulator = PskModulator::new(
            config.sample_rate,
            config.carrier_freq,
            config.symbol_rate,
        );
        
        let frame_manager = FrameManager::new_default();
        
        let preamble_manager = PreambleManager::new(
            config.sample_rate,
            2000.0,  // Start at 2kHz
            10000.0, // End at 10kHz
            440,     // 440 samples duration
        );
        
        Self {
            config,
            modulator,
            frame_manager,
            preamble_manager,
        }
    }
    
    pub fn new_default() -> Self {
        Self::new(SenderConfig::default())
    }
    
    /// Transmit text from file
    pub fn transmit_text_file(&self, file_path: &str) -> Vec<f32> {
        let text_message = TextProcessor::read_text_file(file_path);
        self.transmit_text(&text_message)
    }
    
    /// Transmit text data
    pub fn transmit_text(&self, text: &str) -> Vec<f32> {
        info!("Text to transmit: {}", text);
        
        let text_bytes = text.as_bytes();
        let frames = self.frame_manager.create_frames(text_bytes);
        
        info!("Text length: {} bytes, {} frames needed", text_bytes.len(), frames.len());
        
        self.transmit_frames(&frames)
    }
    
    /// Transmit a collection of frames
    pub fn transmit_frames(&self, frames: &[Frame]) -> Vec<f32> {
        let mut output_track = Vec::new();
        let preamble = self.preamble_manager.generate_preamble();
        let mut rng = rand::rng();
        
        // Process each frame using PSK
        for (i, frame) in frames.iter().enumerate() {
            // Get frame bits with CRC
            let frame_bits_with_crc = frame.get_bits_with_crc();
            
            // PSK Modulation
            let frame_wave = self.modulator.modulate_bpsk(&frame_bits_with_crc);
            
            // Add preamble
            let mut frame_wave_with_preamble = preamble.clone();
            frame_wave_with_preamble.extend(frame_wave);
            
            // Add random inter-frame spacing
            let (min_spacing, max_spacing) = self.config.inter_frame_spacing_range;
            let inter_frame_space1: usize = rng.random_range(min_spacing..max_spacing);
            let inter_frame_space2: usize = rng.random_range(min_spacing..max_spacing);
            
            output_track.extend(vec![0.0; inter_frame_space1]);
            output_track.extend(frame_wave_with_preamble);
            output_track.extend(vec![0.0; inter_frame_space2]);
            
            let bytes_per_frame = self.frame_manager.config().bytes_per_frame();
            info!("Frame {}: ID={}, data length={} bytes", i + 1, frame.id, bytes_per_frame);
        }
        
        info!("Output track length: {} samples (PSK modulated)", output_track.len());
        output_track
    }
    
    /// Get the frame manager
    pub fn frame_manager(&self) -> &FrameManager {
        &self.frame_manager
    }
    
    /// Get the configuration
    pub fn config(&self) -> &SenderConfig {
        &self.config
    }
}
