/// Acoustic link sender implementation
use crate::acoustic::{
    config::{DATA_TYPE, PREAMBLE_FRE_MAX, PREAMBLE_FRE_MIN, PREAMBLE_LENGTH},
    encode,
    preamble::create_preamble,
};
use crate::transmission::{Frame, FrameManager, TextProcessor};
use tracing::info;

/// Configuration for PSK sender
pub struct SenderConfig {
    pub preamble_start_freq: f32,
    pub preamble_end_freq: f32,
    pub preamble_length: usize,
}

impl Default for SenderConfig {
    fn default() -> Self {
        Self {
            preamble_start_freq: PREAMBLE_FRE_MIN,
            preamble_end_freq: PREAMBLE_FRE_MAX,
            preamble_length: PREAMBLE_LENGTH,
        }
    }
}

/// PSK Sender for transmitting text data
pub struct PskSender {
    config: SenderConfig,
    frame_manager: FrameManager,
    preamble: Vec<f32>,
}

impl PskSender {
    pub fn new(config: SenderConfig) -> Self {
        let frame_manager = FrameManager::new_default();
        let preamble = create_preamble(
            config.preamble_start_freq,
            config.preamble_end_freq,
            config.preamble_length,
        );

        Self {
            config,
            frame_manager,
            preamble,
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
        let frames = self
            .frame_manager
            .create_frames(text_bytes);
        info!(
            "Text length: {} bytes, {} frames needed",
            text_bytes.len(),
            frames.len()
        );
        self.transmit_frames(&frames)
    }

    /// Transmit a collection of frames
    pub fn transmit_frames(&self, frames: &[Frame]) -> Vec<f32> {
        let mut output_track = Vec::new();

        // Process each frame using PSK
        for (i, frame) in frames.iter().enumerate() {
            let waveform = encode::encode(
                frame.data.clone(),
                &self.preamble,
                frame.id as usize,
                DATA_TYPE,
            );
            output_track.extend_from_slice(&waveform);
            info!(
                "Frame {}: ID={}, payload={} bytes",
                i + 1,
                frame.id,
                frame.data.len()
            );
        }

        info!(
            "Output track length: {} samples (acoustic modulation)",
            output_track.len()
        );
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
