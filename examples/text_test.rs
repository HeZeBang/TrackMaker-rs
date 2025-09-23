/// Test text transmission and reception without JACK audio
use trackmaker_rs::audio::psk::{PskModulator, PskDemodulator, utils as psk_utils};
use std::fs;

fn main() {
    
    println!("Testing PSK text transmission...");
    
    // PSK Configuration
    let sample_rate = 48000.0;
    let carrier_freq = 10000.0;
    let symbol_rate = 1000.0;
    
    let modulator = PskModulator::new(sample_rate, carrier_freq, symbol_rate);
    let demodulator = PskDemodulator::new(sample_rate, carrier_freq, symbol_rate);
    
    // Read text from file
    let text_file_path = "assets/think-different.txt";
    let text_message = match fs::read_to_string(text_file_path) {
        Ok(content) => {
            println!("Successfully read text from: {}", text_file_path);
            content.trim().to_string()
        }
        Err(e) => {
            println!("Failed to read file {}: {}, using fallback text", text_file_path, e);
            "Hello World! 你好世界！".to_string()
        }
    };
    let text_bytes = text_message.as_bytes();
    
    println!("Original text: {}", text_message);
    println!("Text length: {} bytes", text_bytes.len());
    
    // Calculate frames needed
    let bytes_per_frame = 11; // 88 bits / 8 = 11 bytes
    let total_frames = (text_bytes.len() + bytes_per_frame - 1) / bytes_per_frame;
    
    println!("Frames needed: {}", total_frames);
    
    // Generate preamble
    let preamble = psk_utils::generate_chirp_preamble(
        sample_rate,
        2000.0,
        10000.0,
        440
    );
    
    let mut signal = Vec::new();
    
    // Create frames and modulate
    for frame_idx in 0..total_frames {
        let mut frame_bits = vec![0u8; 100];
        
        // Set frame ID
        let frame_id = (frame_idx + 1) as u8;
        for bit_idx in 0..8 {
            frame_bits[bit_idx] = ((frame_id >> (7 - bit_idx)) & 1) as u8;
        }
        
        // Add data bits
        let start_byte = frame_idx * bytes_per_frame;
        let end_byte = std::cmp::min(start_byte + bytes_per_frame, text_bytes.len());
        
        for byte_idx in start_byte..end_byte {
            let byte_value = text_bytes[byte_idx];
            let frame_byte_idx = byte_idx - start_byte;
            let bit_start = 8 + frame_byte_idx * 8;
            
            for bit_idx in 0..8 {
                if bit_start + bit_idx < 96 {
                    frame_bits[bit_start + bit_idx] = ((byte_value >> (7 - bit_idx)) & 1) as u8;
                }
            }
        }
        
        // Add CRC (placeholder)
        let mut frame_crc = frame_bits.clone();
        frame_crc.extend_from_slice(&[0u8; 8]);
        
        // Modulate
        let frame_wave = modulator.modulate_bpsk(&frame_crc);
        
        // Add preamble and frame
        signal.extend(&preamble);
        signal.extend(&frame_wave);
        signal.extend(vec![0.0; 100]); // Inter-frame spacing
        
        println!("Frame {}: ID={}, bytes={}", frame_idx + 1, frame_id, end_byte - start_byte);
    }
    
    println!("Total signal length: {} samples", signal.len());
    
    // Now demodulate the signal
    println!("\n=== DEMODULATION ===");
    
    let samples_per_symbol = (sample_rate / symbol_rate) as usize;
    let frame_length_samples = 108 * samples_per_symbol;
    let preamble_length = preamble.len();
    
    // Cross-correlate to find frames
    let correlation = psk_utils::cross_correlate(&signal, &preamble);
    let correlation_threshold = correlation.iter().fold(0.0f32, |acc, &x| acc.max(x)) * 0.3;
    
    let mut frame_starts = Vec::new();
    let mut i = 0;
    while i < correlation.len() {
        if correlation[i] > correlation_threshold {
            frame_starts.push(i + preamble_length);
            i += frame_length_samples;
        } else {
            i += 1;
        }
    }
    
    println!("Found {} potential frames", frame_starts.len());
    
    // Demodulate frames
    let mut received_frames: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut correct_frames = 0;
    
    for (frame_idx, &frame_start) in frame_starts.iter().enumerate() {
        let frame_end = frame_start + frame_length_samples;
        
        if frame_end <= signal.len() {
            let frame_signal = &signal[frame_start..frame_end];
            let demodulated_bits = demodulator.demodulate_bpsk(frame_signal);
            
            if demodulated_bits.len() >= 96 {
                // Extract frame ID
                let mut frame_id = 0u8;
                for k in 0..8 {
                    if demodulated_bits[k] == 1 {
                        frame_id += 1 << (7 - k);
                    }
                }
                
                if frame_id > 0 {
                    // Extract data bytes
                    let mut data_bytes = Vec::new();
                    for byte_idx in 0..11 {
                        let mut byte_value = 0u8;
                        for bit_idx in 0..8 {
                            let bit_pos = 8 + byte_idx * 8 + bit_idx;
                            if bit_pos < demodulated_bits.len() && demodulated_bits[bit_pos] == 1 {
                                byte_value |= 1 << (7 - bit_idx);
                            }
                        }
                        data_bytes.push(byte_value);
                    }
                    
                    received_frames.push((frame_id, data_bytes));
                    println!("Frame {}: ID={}, demodulated successfully", frame_idx + 1, frame_id);
                    correct_frames += 1;
                } else {
                    println!("Frame {}: Invalid ID={}", frame_idx + 1, frame_id);
                }
            } else {
                println!("Frame {}: Insufficient bits", frame_idx + 1);
            }
        }
    }
    
    println!("Correct frames: {} / {}", correct_frames, frame_starts.len());
    
    // Reconstruct text
    if !received_frames.is_empty() {
        received_frames.sort_by_key(|(id, _)| *id);
        
        let mut reconstructed_text = Vec::new();
        for (frame_id, data_bytes) in received_frames {
            println!("Processing frame ID={}, data={:?}", frame_id, data_bytes);
            for &byte in &data_bytes {
                if byte != 0 {
                    reconstructed_text.push(byte);
                } else {
                    break;
                }
            }
        }
        
        match String::from_utf8(reconstructed_text.clone()) {
            Ok(text) => {
                println!("\n=== RECEIVED TEXT ===");
                println!("{}", text);
                println!("=== END ===");
                
                // Compare with original
                if text == text_message {
                    println!("✅ TEXT TRANSMISSION SUCCESSFUL!");
                } else {
                    println!("❌ Text mismatch!");
                    println!("Expected: {}", text_message);
                    println!("Received: {}", text);
                }
            }
            Err(e) => {
                println!("❌ UTF-8 conversion error: {}", e);
                println!("Raw bytes: {:?}", reconstructed_text);
            }
        }
    } else {
        println!("❌ No valid frames received");
    }
}
