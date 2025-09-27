/// Acoustic link receiver implementation
use crate::acoustic::{
    config::{
        DATA_TYPE, FRAME_DATA_SAMPLES, HEAD_LEN, HEADER_SAMPLES,
        PREAMBLE_FRE_MAX, PREAMBLE_FRE_MIN, PREAMBLE_LENGTH,
    },
    decode::{decode, head_decode},
    preamble::{create_preamble, detect_signal},
};
use crate::transmission::{Frame, FrameManager, TextProcessor};
use std::path::Path;
use tracing::info;

/// Configuration for PSK receiver
pub struct ReceiverConfig {
    pub preamble_start_freq: f32,
    pub preamble_end_freq: f32,
    pub preamble_length: usize,
}

impl Default for ReceiverConfig {
    fn default() -> Self {
        Self {
            preamble_start_freq: PREAMBLE_FRE_MIN,
            preamble_end_freq: PREAMBLE_FRE_MAX,
            preamble_length: PREAMBLE_LENGTH,
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
    pub fn new(
        total_detected: usize,
        correct: usize,
        frames: Vec<Frame>,
    ) -> Self {
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
    frame_manager: FrameManager,
    preamble: Vec<f32>,
}

impl PskReceiver {
    pub fn new(config: ReceiverConfig) -> Self {
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
        Self::new(ReceiverConfig::default())
    }

    /// Process received signal and decode frames
    pub fn process_signal(&self, rx_signal: &[f32]) -> ReceptionResult {
        if rx_signal.is_empty() {
            info!("No signal received");
            return ReceptionResult::new(0, 0, Vec::new());
        }

        info!("Processing received signal with acoustic demodulation...");

        let mut cursor = 0;
        let mut total_candidates = 0;
        let mut received_frames = Vec::new();

        while cursor + self.preamble.len() + HEADER_SAMPLES + FRAME_DATA_SAMPLES
            <= rx_signal.len()
        {
            let window = &rx_signal[cursor..];
            let detections = detect_signal(&self.preamble, window);

            if detections.is_empty() {
                break;
            }

            let preamble_offset = cursor + detections[0];
            let header_start = preamble_offset + self.preamble.len();
            let header_end = header_start + HEADER_SAMPLES;

            if header_end > rx_signal.len() {
                break;
            }

            let header_slice = &rx_signal[header_start..header_end];
            let header = head_decode(header_slice);

            if header.len() < HEAD_LEN {
                cursor = header_end;
                continue;
            }

            let frame_type = header[2];
            let frame_index = header[3];

            if frame_type != DATA_TYPE as u8 {
                cursor = header_end;
                continue;
            }

            let data_start = header_end;
            let data_end = data_start + FRAME_DATA_SAMPLES;

            if data_end > rx_signal.len() {
                break;
            }

            total_candidates += 1;
            let (data, right) = decode(&rx_signal[data_start..data_end]);

            if right {
                info!("Frame {} decoded successfully", frame_index);
                received_frames.push(Frame {
                    id: frame_index,
                    data,
                });
            } else {
                info!("Frame {} failed Reed-Solomon decoding", frame_index);
            }

            cursor = data_end;
        }

        let correct_frame_count = received_frames.len();
        ReceptionResult::new(
            total_candidates,
            correct_frame_count,
            received_frames,
        )
    }

    /// Receive and decode text from signal
    pub fn receive_text(&self, rx_signal: &[f32]) -> Option<String> {
        let result = self.process_signal(rx_signal);

        info!(
            "Total Correct Frames: {} / {}",
            result.correct_frames, result.total_frames_detected
        );

        if result.total_frames_detected > 0 {
            info!("Success Rate: {:.1}%", result.success_rate);
        }

        if result
            .received_frames
            .is_empty()
        {
            info!("No valid frames received");
            return None;
        }

        let reconstructed_bytes = self
            .frame_manager
            .reconstruct_data(result.received_frames);

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
                if let Err(e) = TextProcessor::save_raw_bytes(
                    &reconstructed_bytes,
                    &raw_file_path,
                ) {
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
