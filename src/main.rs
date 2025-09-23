use dialoguer::{theme::ColorfulTheme, Select};
use jack;
mod audio;
mod device;
mod ui;
mod utils;
use audio::recorder;
use audio::psk::{PskModulator, PskDemodulator, utils as psk_utils};
use device::jack::{
    print_jack_info,
};
use rand::{self, Rng};
use tracing::info;
use ui::print_banner;
use ui::progress::{ProgressManager, templates};
use utils::consts::*;
use utils::logging::init_logging;
use std::fs;
use std::path::Path;

use crate::device::jack::connect_system_ports;

fn main() {
    init_logging();
    print_banner();

    let selections = &["Sender", "Receiver"];
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select mode")
        .default(0)
        .items(&selections[..])
        .interact()
        .unwrap();

    let (client, status) = jack::Client::new(
        JACK_CLIENT_NAME,
        jack::ClientOptions::NO_START_SERVER,
    )
    .unwrap();
    tracing::info!("JACK client status: {:?}", status);
    let (sample_rate, _buffer_size) = print_jack_info(&client);

    let max_duration_samples = sample_rate * 15;

    // Shared State
    let shared = recorder::AppShared::new(max_duration_samples);
    let shared_cb = shared.clone();

    let in_port = client
        .register_port(INPUT_PORT_NAME, jack::AudioIn::default())
        .unwrap();
    let out_port = client
        .register_port(OUTPUT_PORT_NAME, jack::AudioOut::default())
        .unwrap();

    let in_port_name = in_port.name().unwrap();
    let out_port_name = out_port.name().unwrap();

    // Process Callback
    let process_cb = recorder::build_process_closure(
        in_port,
        out_port,
        shared_cb,
        max_duration_samples,
    );
    let process = jack::contrib::ClosureProcessHandler::new(process_cb);

    let active_client = client
        .activate_async((), process)
        .unwrap();

    let progress_manager = ProgressManager::new();

    connect_system_ports(
        active_client.as_client(),
        in_port_name.as_str(),
        out_port_name.as_str(),
    );

    if selection == 0 {
        // Sender
        run_sender(shared, progress_manager, sample_rate as u32);
    } else {
        // Receiver
        run_receiver(shared, progress_manager, sample_rate as u32, max_duration_samples as u32);
    }

    tracing::info!("Exiting gracefully...");
    if let Err(err) = active_client.deactivate() {
        tracing::error!("Error deactivating client: {}", err);
    }
}

fn run_sender(
    shared: recorder::AppShared,
    progress_manager: ProgressManager,
    sample_rate: u32,
) {
    // PSK Configuration
    let sample_rate_f32 = sample_rate as f32;
    let carrier_freq = 10000.0; // 10kHz carrier
    let symbol_rate = 1000.0;   // 1000 symbols/second
    
    let modulator = PskModulator::new(sample_rate_f32, carrier_freq, symbol_rate);
    
    // Read text data from file
    let text_file_path = "assets/think-different.txt";
    let text_message = match fs::read_to_string(text_file_path) {
        Ok(content) => {
            info!("Successfully read text from: {}", text_file_path);
            content.trim().to_string() // Remove trailing whitespace
        }
        Err(e) => {
            info!("Failed to read file {}: {}, using fallback text", text_file_path, e);
            "Hello World! This is a fallback message for PSK transmission. 你好世界！".to_string()
        }
    };
    
    let text_bytes = text_message.as_bytes();
    
    let mut output_track = Vec::new();
    
    // Calculate number of frames needed (each frame carries 88 bits of data: 100 bits - 8 ID bits - 4 padding)
    let bits_per_frame = 88; // 100 total - 8 ID bits - 4 padding
    let bytes_per_frame = bits_per_frame / 8; // 11 bytes per frame
    
    let total_frames = (text_bytes.len() + bytes_per_frame - 1) / bytes_per_frame; // Round up
    let mut frames = Vec::new();
    
    info!("Text to transmit: {}", text_message);
    info!("Text length: {} bytes, {} frames needed", text_bytes.len(), total_frames);
    
    // Split text into frames
    for frame_idx in 0..total_frames {
        let mut frame_bits = vec![0u8; 100];
        
        // Set frame ID (first 8 bits)
        let frame_id = (frame_idx + 1) as u8;
        for bit_idx in 0..8 {
            frame_bits[bit_idx] = ((frame_id >> (7 - bit_idx)) & 1) as u8;
        }
        
        // Add data bits (next 88 bits = 11 bytes)
        let start_byte = frame_idx * bytes_per_frame;
        let end_byte = std::cmp::min(start_byte + bytes_per_frame, text_bytes.len());
        
        for byte_idx in start_byte..end_byte {
            let byte_value = text_bytes[byte_idx];
            let frame_byte_idx = byte_idx - start_byte;
            let bit_start = 8 + frame_byte_idx * 8; // Start after ID bits
            
            for bit_idx in 0..8 {
                if bit_start + bit_idx < 96 { // Leave 4 bits for padding/CRC
                    frame_bits[bit_start + bit_idx] = ((byte_value >> (7 - bit_idx)) & 1) as u8;
                }
            }
        }
        
        frames.push(frame_bits);
    }

    // Generate chirp preamble for synchronization (440 samples)
    let preamble = psk_utils::generate_chirp_preamble(
        sample_rate_f32,
        2000.0,  // Start at 2kHz
        10000.0, // End at 10kHz
        440      // 440 samples duration
    );

    let mut rng = rand::rng();

    // Process each frame using PSK
    for (i, frame) in frames.iter().enumerate() {
        // Add CRC8 (simplified implementation)
        let mut frame_crc = frame.clone();
        frame_crc.extend_from_slice(&[0u8; 8]); // Add 8 CRC bits (placeholder)

        // PSK Modulation
        let frame_wave = modulator.modulate_bpsk(&frame_crc);

        // Add preamble
        let mut frame_wave_pre = preamble.clone();
        frame_wave_pre.extend(frame_wave);

        // Add random inter-frame spacing
        let inter_frame_space1: usize = rng.random_range(0..100);
        let inter_frame_space2: usize = rng.random_range(0..100);

        output_track.extend(vec![0.0; inter_frame_space1]);
        output_track.extend(frame_wave_pre);
        output_track.extend(vec![0.0; inter_frame_space2]);
        
        info!("Frame {}: ID={}, data length={} bytes", i + 1, i + 1, bytes_per_frame);
    }

    let output_track_len = output_track.len();

    {
        let mut playback = shared.playback_buffer.lock().unwrap();
        playback.extend(output_track);
        info!(
            "Output track length: {} samples (PSK modulated)",
            playback.len()
        );
    }

    progress_manager
        .create_bar(
            "playback",
            output_track_len as u64,
            templates::PLAYBACK,
            "sender",
        )
        .unwrap();

    *shared.app_state.lock().unwrap() = recorder::AppState::Playing;

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));

        ui::update_progress(&shared, output_track_len, &progress_manager);

        let state = { shared.app_state.lock().unwrap().clone() };
        if let recorder::AppState::Idle = state {
            progress_manager.finish_all();
            break;
        }
    }
}

fn run_receiver(
    shared: recorder::AppShared,
    progress_manager: ProgressManager,
    sample_rate: u32,
    max_recording_duration_samples: u32,
) {
    // PSK Configuration
    let sample_rate_f32 = sample_rate as f32;
    let carrier_freq = 10000.0; // 10kHz carrier
    let symbol_rate = 1000.0;   // 1000 symbols/second
    
    let demodulator = PskDemodulator::new(sample_rate_f32, carrier_freq, symbol_rate);

    // Generate chirp preamble for synchronization (same as sender)
    let preamble = psk_utils::generate_chirp_preamble(
        sample_rate_f32,
        2000.0,  // Start at 2kHz
        10000.0, // End at 10kHz
        440      // 440 samples duration
    );

    progress_manager
        .create_bar(
            "recording",
            max_recording_duration_samples as u64,
            templates::RECORDING,
            "receiver",
        )
        .unwrap();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));

        ui::update_progress(
            &shared,
            max_recording_duration_samples as usize,
            &progress_manager,
        );

        let state = {
            shared
                .app_state
                .lock()
                .unwrap()
                .clone()
        };
        if let recorder::AppState::Idle = state {
            progress_manager.finish_all();
            break;
        }
    }

    let rx_signal: Vec<f32> = {
        let record = shared.record_buffer.lock().unwrap();
        record.iter().copied().collect()
    };

    if rx_signal.is_empty() {
        info!("No signal received");
        return;
    }

    info!("Processing received signal with PSK demodulation...");

    let mut correct_frame_num = 0;
    let samples_per_symbol = (sample_rate_f32 / symbol_rate) as usize;
    let frame_length_samples = 108 * samples_per_symbol; // 108 bits per frame (100 + 8 CRC)
    let preamble_length = preamble.len();

    // Cross-correlate with preamble to find frame starts
    let correlation = psk_utils::cross_correlate(&rx_signal, &preamble);
    
    // Find correlation peaks above threshold
    let correlation_threshold = correlation.iter().fold(0.0f32, |acc, &x| acc.max(x)) * 0.3;
    
    let mut frame_starts = Vec::new();
    let mut i = 0;
    while i < correlation.len() {
        if correlation[i] > correlation_threshold {
            // Found a potential frame start
            frame_starts.push(i + preamble_length); // Frame starts after preamble
            
            // Skip ahead to avoid detecting the same frame multiple times
            i += frame_length_samples;
        } else {
            i += 1;
        }
    }

    info!("Found {} potential frames", frame_starts.len());

    // Store received text data
    let mut received_frames: Vec<(u8, Vec<u8>)> = Vec::new(); // (frame_id, data_bytes)

    // Demodulate each detected frame
    for (frame_idx, &frame_start) in frame_starts.iter().enumerate() {
        let frame_end = frame_start + frame_length_samples;
        
        if frame_end <= rx_signal.len() {
            let frame_signal = &rx_signal[frame_start..frame_end];
            
            // Demodulate using PSK
            let demodulated_bits = demodulator.demodulate_bpsk(frame_signal);
            
            if demodulated_bits.len() >= 96 { // At least 96 bits (8 ID + 88 data)
                // Extract frame ID from first 8 bits
                let mut frame_id = 0u8;
                for k in 0..8 {
                    if demodulated_bits[k] == 1 {
                        frame_id += 1 << (7 - k);
                    }
                }

                if frame_id > 0 {
                    // Extract data bytes (11 bytes = 88 bits)
                    let mut data_bytes = Vec::new();
                    for byte_idx in 0..11 {
                        let mut byte_value = 0u8;
                        for bit_idx in 0..8 {
                            let bit_pos = 8 + byte_idx * 8 + bit_idx; // Start after ID bits
                            if bit_pos < demodulated_bits.len() && demodulated_bits[bit_pos] == 1 {
                                byte_value |= 1 << (7 - bit_idx);
                            }
                        }
                        data_bytes.push(byte_value);
                    }
                    
                    received_frames.push((frame_id, data_bytes));
                    info!("Frame {}: Correct, ID: {}", frame_idx + 1, frame_id);
                    correct_frame_num += 1;
                } else {
                    info!("Frame {}: Error in frame, decoded ID: {}", frame_idx + 1, frame_id);
                }
            } else {
                info!("Frame {}: Insufficient bits demodulated", frame_idx + 1);
            }
        } else {
            info!("Frame {}: Signal too short for complete frame", frame_idx + 1);
        }
    }

    info!("Total Correct Frames: {} / {}", correct_frame_num, frame_starts.len());
    
    // Calculate success rate
    if !frame_starts.is_empty() {
        let success_rate = (correct_frame_num as f32 / frame_starts.len() as f32) * 100.0;
        info!("Success Rate: {:.1}%", success_rate);
    }
    
    // Reconstruct text from received frames
    if !received_frames.is_empty() {
        // Sort frames by ID
        received_frames.sort_by_key(|(id, _)| *id);
        
        let mut reconstructed_text = Vec::new();
        let mut last_frame_id = 0u8;
        
        for (frame_id, data_bytes) in received_frames {
            // Check for missing frames
            if frame_id != last_frame_id + 1 && last_frame_id != 0 {
                info!("Warning: Missing frame(s) between {} and {}", last_frame_id, frame_id);
            }
            
            // Add non-zero bytes to reconstructed text
            for &byte in &data_bytes {
                if byte != 0 { // Stop at null bytes (padding)
                    reconstructed_text.push(byte);
                } else {
                    break;
                }
            }
            
            last_frame_id = frame_id;
        }
        
        // Convert to string and display
        match String::from_utf8(reconstructed_text.clone()) {
            Ok(text) => {
                info!("=== RECEIVED TEXT ===");
                info!("{}", text);
                info!("=== END TEXT ===");
                
                // Save received text to tmp directory
                let tmp_dir = Path::new("tmp");
                if !tmp_dir.exists() {
                    if let Err(e) = fs::create_dir_all(tmp_dir) {
                        info!("Failed to create tmp directory: {}", e);
                    }
                }
                
                let received_file_path = tmp_dir.join("received_text.txt");
                match fs::write(&received_file_path, &text) {
                    Ok(_) => {
                        info!("Received text saved to: {}", received_file_path.display());
                        
                        // Also save the original text for comparison
                        let original_file_path = tmp_dir.join("original_text.txt");
                        let original_text = match fs::read_to_string("assets/think-different.txt") {
                            Ok(content) => content.trim().to_string(),
                            Err(_) => "Original text not available".to_string(),
                        };
                        
                        if let Err(e) = fs::write(&original_file_path, &original_text) {
                            info!("Failed to save original text: {}", e);
                        } else {
                            info!("Original text saved to: {}", original_file_path.display());
                            
                            // Compare texts
                            if text.trim() == original_text.trim() {
                                info!("✅ TEXT TRANSMISSION PERFECT MATCH!");
                            } else {
                                info!("⚠️  Text transmission has differences");
                                info!("Original length: {} bytes", original_text.len());
                                info!("Received length: {} bytes", text.len());
                                
                                // Find first difference
                                let orig_chars: Vec<char> = original_text.chars().collect();
                                let recv_chars: Vec<char> = text.chars().collect();
                                let min_len = std::cmp::min(orig_chars.len(), recv_chars.len());
                                
                                for i in 0..min_len {
                                    if orig_chars[i] != recv_chars[i] {
                                        info!("First difference at position {}: '{}' vs '{}'", 
                                             i, orig_chars[i], recv_chars[i]);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        info!("Failed to save received text: {}", e);
                    }
                }
            }
            Err(e) => {
                info!("Error converting to UTF-8: {}", e);
                info!("Raw bytes: {:?}", reconstructed_text);
                
                // Save raw bytes for debugging
                let tmp_dir = Path::new("tmp");
                if !tmp_dir.exists() {
                    let _ = fs::create_dir_all(tmp_dir);
                }
                let raw_file_path = tmp_dir.join("received_raw_bytes.bin");
                if let Err(e) = fs::write(&raw_file_path, &reconstructed_text) {
                    info!("Failed to save raw bytes: {}", e);
                } else {
                    info!("Raw bytes saved to: {}", raw_file_path.display());
                }
            }
        }
    } else {
        info!("No valid frames received");
    }
}