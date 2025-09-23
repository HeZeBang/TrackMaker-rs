use crate::audio::psk::{PskModulator, PskDemodulator, utils as psk_utils};
use rand::{Rng, SeedableRng};
use std::collections::HashMap;

#[test]
fn test_psk_modulation_basic() {
    let sample_rate = 48000.0;
    let carrier_freq = 10000.0;
    let symbol_rate = 1000.0;
    
    let modulator = PskModulator::new(sample_rate, carrier_freq, symbol_rate);
    let demodulator = PskDemodulator::new(sample_rate, carrier_freq, symbol_rate);
    
    // Test with simple bit pattern
    let test_bits = vec![1, 0, 1, 1, 0, 0, 1, 0];
    
    // Modulate
    let modulated = modulator.modulate_bpsk(&test_bits);
    
    // Demodulate
    let demodulated = demodulator.demodulate_bpsk(&modulated);
    
    println!("Original:   {:?}", test_bits);
    println!("Demodulated: {:?}", demodulated);
    
    // Should match exactly for clean signal
    assert_eq!(test_bits.len(), demodulated.len());
    
    // Count correct bits
    let correct_bits = test_bits.iter()
        .zip(demodulated.iter())
        .filter(|(a, b)| a == b)
        .count();
    
    let accuracy = correct_bits as f32 / test_bits.len() as f32;
    println!("Accuracy: {:.1}%", accuracy * 100.0);
    
    // Should have high accuracy for clean signal
    assert!(accuracy > 0.8, "Accuracy too low: {:.1}%", accuracy * 100.0);
}

#[test]
fn test_psk_with_noise() {
    use rand::{Rng, SeedableRng};
    
    let sample_rate = 48000.0;
    let carrier_freq = 10000.0;
    let symbol_rate = 1000.0;
    
    let modulator = PskModulator::new(sample_rate, carrier_freq, symbol_rate);
    let demodulator = PskDemodulator::new(sample_rate, carrier_freq, symbol_rate);
    
    // Test with random bit pattern
    let mut rng = rand::rngs::StdRng::from_seed([42u8; 32]);
    let test_bits: Vec<u8> = (0..16).map(|_| rng.random_range(0..=1)).collect();
    
    // Modulate
    let mut modulated = modulator.modulate_bpsk(&test_bits);
    
    // Add some noise
    let noise_level = 0.1;
    for sample in modulated.iter_mut() {
        *sample += rng.random_range(-noise_level..noise_level);
    }
    
    // Demodulate
    let demodulated = demodulator.demodulate_bpsk(&modulated);
    
    println!("Original:   {:?}", test_bits);
    println!("Demodulated: {:?}", demodulated);
    
    // Count correct bits
    let correct_bits = test_bits.iter()
        .zip(demodulated.iter())
        .filter(|(a, b)| a == b)
        .count();
    
    let accuracy = correct_bits as f32 / test_bits.len() as f32;
    println!("Accuracy with noise: {:.1}%", accuracy * 100.0);
    
    // Should still have reasonable accuracy with low noise
    assert!(accuracy > 0.6, "Accuracy too low with noise: {:.1}%", accuracy * 100.0);
}

#[test]
fn test_direct_vector_transmission() {
    println!("Running direct vector test...");
    
    // PSK Configuration
    let sample_rate = 48000.0;
    let carrier_freq = 10000.0;
    let symbol_rate = 1000.0;
    
    let modulator = PskModulator::new(sample_rate, carrier_freq, symbol_rate);
    let demodulator = PskDemodulator::new(sample_rate, carrier_freq, symbol_rate);
    
    // Generate test data (same as sender)
    let seed = 1u64;
    let mut rng = rand::rngs::StdRng::from_seed([seed as u8; 32]);

    let mut output_track = Vec::new();

    // 100 frames, each 100 bits
    let mut frames = vec![vec![0u8; 100]; 100];

    // Fill with random 0s and 1s
    for i in 0..100 {
        for j in 0..100 {
            frames[i][j] = rng.random_range(0..=1);
        }
    }

    // Set first 8 bits to id
    for i in 0..100 {
        let id = i + 1; // 1-indexed like MATLAB
        for j in 0..8 {
            frames[i][j] = ((id >> (7 - j)) & 1) as u8;
        }
    }

    // Generate chirp preamble for synchronization (440 samples)
    let preamble = psk_utils::generate_chirp_preamble(
        sample_rate,
        2000.0,  // Start at 2kHz
        10000.0, // End at 10kHz
        440      // 440 samples duration
    );

    // Process each frame using PSK
    for i in 0..100 {
        let frame = &frames[i];

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
    }

    println!("Generated signal length: {} samples", output_track.len());

    // Now demodulate the generated signal
    let mut correct_frame_num = 0;
    let samples_per_symbol = (sample_rate / symbol_rate) as usize;
    let frame_length_samples = 108 * samples_per_symbol; // 108 bits per frame (100 + 8 CRC)
    let preamble_length = preamble.len();

    // Cross-correlate with preamble to find frame starts
    let correlation = psk_utils::cross_correlate(&output_track, &preamble);
    
    // Find correlation peaks above threshold
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

    // Demodulate each detected frame
    for (_frame_idx, &frame_start) in frame_starts.iter().enumerate() {
        let frame_end = frame_start + frame_length_samples;
        
        if frame_end <= output_track.len() {
            let frame_signal = &output_track[frame_start..frame_end];
            
            // Demodulate using PSK
            let demodulated_bits = demodulator.demodulate_bpsk(frame_signal);
            
            if demodulated_bits.len() >= 8 {
                // Extract frame ID from first 8 bits
                let mut frame_id = 0u8;
                for k in 0..8 {
                    if demodulated_bits[k] == 1 {
                        frame_id += 1 << (7 - k);
                    }
                }

                if frame_id > 0 && frame_id <= 100 {
                    correct_frame_num += 1;
                    
                    // Compare original vs demodulated data for this frame
                    let original_frame = &frames[(frame_id - 1) as usize];
                    let mut bit_errors = 0;
                    for k in 8..std::cmp::min(100, demodulated_bits.len()) {
                        if demodulated_bits[k] != original_frame[k] {
                            bit_errors += 1;
                        }
                    }
                    if bit_errors > 0 {
                        println!("  Frame {}: {} bit errors in data", frame_id, bit_errors);
                    }
                }
            }
        }
    }

    println!("=== Test Results ===");
    println!("Total Generated Frames: 100");
    println!("Total Detected Frames: {}", frame_starts.len());
    println!("Total Correct Frames: {}", correct_frame_num);
    
    // Calculate success rates
    if !frame_starts.is_empty() {
        let detection_rate = (frame_starts.len() as f32 / 100.0) * 100.0;
        let success_rate = (correct_frame_num as f32 / frame_starts.len() as f32) * 100.0;
        let overall_success_rate = (correct_frame_num as f32 / 100.0) * 100.0;
        
        println!("Frame Detection Rate: {:.1}%", detection_rate);
        println!("Demodulation Success Rate: {:.1}%", success_rate);
        println!("Overall Success Rate: {:.1}%", overall_success_rate);
        
        // Test assertions
        assert_eq!(frame_starts.len(), 100, "Should detect all 100 frames");
        assert_eq!(correct_frame_num, 100, "Should correctly demodulate all frames");
        assert!(overall_success_rate >= 95.0, "Overall success rate should be at least 95%");
    } else {
        panic!("No frames detected!");
    }
}

#[test]
fn test_text_file_transmission() {
    println!("Running text file transmission test...");
    
    // Use a smaller test text to keep test fast
    let test_text = "Hello, world! This is a test message for PSK transmission. \
                     It contains various characters: 123456789, symbols: !@#$%^&*(), \
                     and newlines:\nLine 1\nLine 2\nEnd.";
    
    println!("Test text length: {} characters", test_text.len());
    
    // Convert text to bytes and then to bits
    let text_bytes = test_text.as_bytes();
    let mut text_bits = Vec::new();
    for byte in text_bytes {
        for i in 0..8 {
            text_bits.push((byte >> (7 - i)) & 1);
        }
    }
    
    // PSK Configuration
    let sample_rate = 48000.0;
    let carrier_freq = 10000.0;
    let symbol_rate = 1000.0;
    
    let modulator = PskModulator::new(sample_rate, carrier_freq, symbol_rate);
    let demodulator = PskDemodulator::new(sample_rate, carrier_freq, symbol_rate);
    
    // Generate chirp preamble
    let preamble = psk_utils::generate_chirp_preamble(
        sample_rate,
        2000.0,
        10000.0,
        440
    );
    
    // Frame parameters
    let bits_per_frame = 100;
    let header_bits = 16;
    let data_bits_per_frame = bits_per_frame - header_bits;
    
    let total_frames = (text_bits.len() + data_bits_per_frame - 1) / data_bits_per_frame;
    println!("Total frames needed: {}", total_frames);
    
    let mut output_track = Vec::new();
    
    // Process each frame
    for frame_idx in 0..total_frames {
        let mut frame_bits = vec![0u8; bits_per_frame];
        
        // Add frame header
        let frame_num = (frame_idx + 1) as u16;
        let total_frames_u16 = total_frames as u16;
        
        // Frame number (8 bits)
        for i in 0..8 {
            frame_bits[i] = ((frame_num >> (7 - i)) & 1) as u8;
        }
        
        // Total frames (8 bits)
        for i in 0..8 {
            frame_bits[8 + i] = ((total_frames_u16 >> (7 - i)) & 1) as u8;
        }
        
        // Add data bits
        let start_bit = frame_idx * data_bits_per_frame;
        let end_bit = std::cmp::min(start_bit + data_bits_per_frame, text_bits.len());
        
        for (i, &bit) in text_bits[start_bit..end_bit].iter().enumerate() {
            frame_bits[header_bits + i] = bit;
        }
        
        // PSK Modulation
        let frame_wave = modulator.modulate_bpsk(&frame_bits);
        
        // Add preamble and spacing
        let mut frame_wave_pre = preamble.clone();
        frame_wave_pre.extend(frame_wave);
        
        output_track.extend(vec![0.0; 50]);
        output_track.extend(frame_wave_pre);
        output_track.extend(vec![0.0; 50]);
    }
    
    println!("Generated signal length: {} samples", output_track.len());
    
    // Demodulate the signal
    let mut received_frames = HashMap::new();
    let samples_per_symbol = (sample_rate / symbol_rate) as usize;
    let frame_length_samples = bits_per_frame * samples_per_symbol;
    let preamble_length = preamble.len();
    
    // Cross-correlate to find frames
    let correlation = psk_utils::cross_correlate(&output_track, &preamble);
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
    
    // Demodulate each frame
    for &frame_start in &frame_starts {
        let frame_end = frame_start + frame_length_samples;
        
        if frame_end <= output_track.len() {
            let frame_signal = &output_track[frame_start..frame_end];
            let demodulated_bits = demodulator.demodulate_bpsk(frame_signal);
            
            if demodulated_bits.len() >= header_bits {
                // Extract frame number and total frames
                let mut frame_num = 0u16;
                for k in 0..8 {
                    if demodulated_bits[k] == 1 {
                        frame_num += 1 << (7 - k);
                    }
                }
                
                let mut total_frames_decoded = 0u16;
                for k in 0..8 {
                    if demodulated_bits[8 + k] == 1 {
                        total_frames_decoded += 1 << (7 - k);
                    }
                }
                
                if frame_num > 0 && frame_num <= total_frames as u16 && total_frames_decoded == total_frames as u16 {
                    let data_bits: Vec<u8> = demodulated_bits[header_bits..std::cmp::min(bits_per_frame, demodulated_bits.len())].to_vec();
                    received_frames.insert(frame_num, data_bits);
                }
            }
        }
    }
    
    // Reconstruct the text
    let mut reconstructed_bits = Vec::new();
    for frame_num in 1..=total_frames {
        if let Some(data_bits) = received_frames.get(&(frame_num as u16)) {
            reconstructed_bits.extend(data_bits);
        } else {
            // Add zeros for missing frame
            reconstructed_bits.extend(vec![0u8; data_bits_per_frame]);
        }
    }
    
    // Convert bits back to bytes
    let mut reconstructed_bytes = Vec::new();
    for chunk in reconstructed_bits.chunks(8) {
        if chunk.len() == 8 {
            let mut byte = 0u8;
            for (i, &bit) in chunk.iter().enumerate() {
                if bit == 1 {
                    byte |= 1 << (7 - i);
                }
            }
            reconstructed_bytes.push(byte);
        }
    }
    
    // Trim to original length
    reconstructed_bytes.truncate(text_bytes.len());
    
    // Convert back to string
    let reconstructed_text = String::from_utf8_lossy(&reconstructed_bytes).to_string();
    
    // Compare results
    let mut byte_errors = 0;
    for (&orig, &recon) in text_bytes.iter().zip(reconstructed_bytes.iter()) {
        if orig != recon {
            byte_errors += 1;
        }
    }
    
    println!("=== Text Transmission Test Results ===");
    println!("Original text length: {} bytes", text_bytes.len());
    println!("Reconstructed text length: {} bytes", reconstructed_bytes.len());
    println!("Total frames sent: {}", total_frames);
    println!("Total frames decoded correctly: {}", received_frames.len());
    println!("Byte errors: {} / {} ({:.2}%)", byte_errors, text_bytes.len(), (byte_errors as f32 / text_bytes.len() as f32) * 100.0);
    
    if byte_errors == 0 {
        println!("✅ Perfect reconstruction! Text matches exactly.");
        println!("Original:      {:?}", test_text);
        println!("Reconstructed: {:?}", reconstructed_text);
    } else {
        println!("❌ Text reconstruction has errors.");
        println!("Original:      {:?}", test_text);
        println!("Reconstructed: {:?}", reconstructed_text);
    }
    
    // Test assertions
    assert_eq!(frame_starts.len(), total_frames, "Should detect all frames");
    assert_eq!(received_frames.len(), total_frames, "Should decode all frames correctly");
    assert_eq!(byte_errors, 0, "Should have no byte errors");
    assert_eq!(test_text, reconstructed_text, "Reconstructed text should match original exactly");
}

#[test]
fn test_think_different_file_transmission() {
    use std::fs;
    
    println!("Running think-different.txt file transmission test...");
    
    // Try to read the actual think-different.txt file
    let file_path = "assets/think-different.txt";
    let original_text = match fs::read_to_string(file_path) {
        Ok(content) => content,
        Err(_) => {
            // If file doesn't exist, skip this test
            println!("Skipping test: {} not found", file_path);
            return;
        }
    };
    
    println!("Original text length: {} characters", original_text.len());
    
    // Convert text to bytes and then to bits
    let text_bytes = original_text.as_bytes();
    let mut text_bits = Vec::new();
    for byte in text_bytes {
        for i in 0..8 {
            text_bits.push((byte >> (7 - i)) & 1);
        }
    }
    
    // PSK Configuration
    let sample_rate = 48000.0;
    let carrier_freq = 10000.0;
    let symbol_rate = 1000.0;
    
    let modulator = PskModulator::new(sample_rate, carrier_freq, symbol_rate);
    let demodulator = PskDemodulator::new(sample_rate, carrier_freq, symbol_rate);
    
    // Generate chirp preamble
    let preamble = psk_utils::generate_chirp_preamble(
        sample_rate,
        2000.0,
        10000.0,
        440
    );
    
    // Frame parameters
    let bits_per_frame = 100;
    let header_bits = 16;
    let data_bits_per_frame = bits_per_frame - header_bits;
    
    let total_frames = (text_bits.len() + data_bits_per_frame - 1) / data_bits_per_frame;
    println!("Total frames needed: {}", total_frames);
    
    let mut output_track = Vec::new();
    
    // Process each frame
    for frame_idx in 0..total_frames {
        let mut frame_bits = vec![0u8; bits_per_frame];
        
        // Add frame header
        let frame_num = (frame_idx + 1) as u16;
        let total_frames_u16 = total_frames as u16;
        
        // Frame number (8 bits)
        for i in 0..8 {
            frame_bits[i] = ((frame_num >> (7 - i)) & 1) as u8;
        }
        
        // Total frames (8 bits)
        for i in 0..8 {
            frame_bits[8 + i] = ((total_frames_u16 >> (7 - i)) & 1) as u8;
        }
        
        // Add data bits
        let start_bit = frame_idx * data_bits_per_frame;
        let end_bit = std::cmp::min(start_bit + data_bits_per_frame, text_bits.len());
        
        for (i, &bit) in text_bits[start_bit..end_bit].iter().enumerate() {
            frame_bits[header_bits + i] = bit;
        }
        
        // PSK Modulation
        let frame_wave = modulator.modulate_bpsk(&frame_bits);
        
        // Add preamble and spacing
        let mut frame_wave_pre = preamble.clone();
        frame_wave_pre.extend(frame_wave);
        
        output_track.extend(vec![0.0; 50]);
        output_track.extend(frame_wave_pre);
        output_track.extend(vec![0.0; 50]);
    }
    
    println!("Generated signal length: {} samples", output_track.len());
    
    // Demodulate the signal
    let mut received_frames = HashMap::new();
    let samples_per_symbol = (sample_rate / symbol_rate) as usize;
    let frame_length_samples = bits_per_frame * samples_per_symbol;
    let preamble_length = preamble.len();
    
    // Cross-correlate to find frames
    let correlation = psk_utils::cross_correlate(&output_track, &preamble);
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
    
    // Demodulate each frame
    for &frame_start in &frame_starts {
        let frame_end = frame_start + frame_length_samples;
        
        if frame_end <= output_track.len() {
            let frame_signal = &output_track[frame_start..frame_end];
            let demodulated_bits = demodulator.demodulate_bpsk(frame_signal);
            
            if demodulated_bits.len() >= header_bits {
                // Extract frame number and total frames
                let mut frame_num = 0u16;
                for k in 0..8 {
                    if demodulated_bits[k] == 1 {
                        frame_num += 1 << (7 - k);
                    }
                }
                
                let mut total_frames_decoded = 0u16;
                for k in 0..8 {
                    if demodulated_bits[8 + k] == 1 {
                        total_frames_decoded += 1 << (7 - k);
                    }
                }
                
                if frame_num > 0 && frame_num <= total_frames as u16 && total_frames_decoded == total_frames as u16 {
                    let data_bits: Vec<u8> = demodulated_bits[header_bits..std::cmp::min(bits_per_frame, demodulated_bits.len())].to_vec();
                    received_frames.insert(frame_num, data_bits);
                }
            }
        }
    }
    
    // Reconstruct the text
    let mut reconstructed_bits = Vec::new();
    for frame_num in 1..=total_frames {
        if let Some(data_bits) = received_frames.get(&(frame_num as u16)) {
            reconstructed_bits.extend(data_bits);
        } else {
            // Add zeros for missing frame
            reconstructed_bits.extend(vec![0u8; data_bits_per_frame]);
        }
    }
    
    // Convert bits back to bytes
    let mut reconstructed_bytes = Vec::new();
    for chunk in reconstructed_bits.chunks(8) {
        if chunk.len() == 8 {
            let mut byte = 0u8;
            for (i, &bit) in chunk.iter().enumerate() {
                if bit == 1 {
                    byte |= 1 << (7 - i);
                }
            }
            reconstructed_bytes.push(byte);
        }
    }
    
    // Trim to original length
    reconstructed_bytes.truncate(text_bytes.len());
    
    // Convert back to string
    let reconstructed_text = String::from_utf8_lossy(&reconstructed_bytes).to_string();
    
    // Compare results
    let mut byte_errors = 0;
    for (&orig, &recon) in text_bytes.iter().zip(reconstructed_bytes.iter()) {
        if orig != recon {
            byte_errors += 1;
        }
    }
    
    println!("=== Think Different File Test Results ===");
    println!("Original text length: {} bytes", text_bytes.len());
    println!("Reconstructed text length: {} bytes", reconstructed_bytes.len());
    println!("Total frames sent: {}", total_frames);
    println!("Total frames detected: {}", frame_starts.len());
    println!("Total frames decoded correctly: {}", received_frames.len());
    println!("Byte errors: {} / {} ({:.2}%)", byte_errors, text_bytes.len(), (byte_errors as f32 / text_bytes.len() as f32) * 100.0);
    
    if byte_errors == 0 {
        println!("✅ Perfect reconstruction! Think Different text matches exactly.");
    } else {
        println!("❌ Text reconstruction has errors.");
    }
    
    // Test assertions
    assert_eq!(frame_starts.len(), total_frames, "Should detect all frames");
    assert_eq!(received_frames.len(), total_frames, "Should decode all frames correctly");
    assert_eq!(byte_errors, 0, "Should have no byte errors");
    assert_eq!(original_text, reconstructed_text, "Reconstructed text should match original exactly");
}
