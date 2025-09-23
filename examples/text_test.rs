/// Test text transmission and reception without JACK audio
use trackmaker_rs::audio::psk::{PskModulator, PskDemodulator, utils as psk_utils};
use std::fs;
use std::time::{Instant, Duration};

#[derive(Debug)]
struct PerformanceMetrics {
    file_read_time: Duration,
    encoding_time: Duration,
    modulation_time: Duration,
    signal_generation_time: Duration,
    correlation_time: Duration,
    demodulation_time: Duration,
    text_reconstruction_time: Duration,
    total_time: Duration,
    
    // Data metrics
    original_bytes: usize,
    transmitted_frames: usize,
    received_frames: usize,
    signal_samples: usize,
    
    // Quality metrics
    frame_loss_rate: f32,
    byte_error_rate: f32,
    character_accuracy: f32,
}

impl PerformanceMetrics {
    fn new() -> Self {
        Self {
            file_read_time: Duration::ZERO,
            encoding_time: Duration::ZERO,
            modulation_time: Duration::ZERO,
            signal_generation_time: Duration::ZERO,
            correlation_time: Duration::ZERO,
            demodulation_time: Duration::ZERO,
            text_reconstruction_time: Duration::ZERO,
            total_time: Duration::ZERO,
            original_bytes: 0,
            transmitted_frames: 0,
            received_frames: 0,
            signal_samples: 0,
            frame_loss_rate: 0.0,
            byte_error_rate: 0.0,
            character_accuracy: 0.0,
        }
    }
    
    fn print_summary(&self) {
        println!("\n=== PERFORMANCE METRICS ===");
        println!("📊 Timing Performance:");
        println!("   File Read:           {:>8.2} ms", self.file_read_time.as_secs_f64() * 1000.0);
        println!("   Frame Encoding:      {:>8.2} ms", self.encoding_time.as_secs_f64() * 1000.0);
        println!("   PSK Modulation:      {:>8.2} ms", self.modulation_time.as_secs_f64() * 1000.0);
        println!("   Signal Generation:   {:>8.2} ms", self.signal_generation_time.as_secs_f64() * 1000.0);
        println!("   Correlation:         {:>8.2} ms", self.correlation_time.as_secs_f64() * 1000.0);
        println!("   PSK Demodulation:    {:>8.2} ms", self.demodulation_time.as_secs_f64() * 1000.0);
        println!("   Text Reconstruction: {:>8.2} ms", self.text_reconstruction_time.as_secs_f64() * 1000.0);
        println!("   Total Processing:    {:>8.2} ms", self.total_time.as_secs_f64() * 1000.0);
        
        println!("\n📈 Data Throughput:");
        println!("   Original Data:       {:>8} bytes", self.original_bytes);
        println!("   Signal Samples:      {:>8} samples", self.signal_samples);
        println!("   Data Rate:           {:>8.2} KB/s", 
                (self.original_bytes as f64 / 1024.0) / self.total_time.as_secs_f64());
        println!("   Sample Rate:         {:>8.2} MSamples/s", 
                (self.signal_samples as f64 / 1_000_000.0) / self.total_time.as_secs_f64());
        
        println!("\n📡 Transmission Quality:");
        println!("   Frames Sent:         {:>8}", self.transmitted_frames);
        println!("   Frames Received:     {:>8}", self.received_frames);
        println!("   Frame Loss Rate:     {:>8.2}%", self.frame_loss_rate * 100.0);
        println!("   Byte Error Rate:     {:>8.2}%", self.byte_error_rate * 100.0);
        println!("   Character Accuracy:  {:>8.2}%", self.character_accuracy * 100.0);
        
        println!("\n⚡ Performance Summary:");
        if self.frame_loss_rate == 0.0 {
            println!("   ✅ Perfect transmission - no frame loss");
        } else if self.frame_loss_rate < 0.05 {
            println!("   ✅ Excellent transmission quality");
        } else if self.frame_loss_rate < 0.1 {
            println!("   ⚠️  Good transmission quality");
        } else {
            println!("   ❌ Poor transmission quality");
        }
        
        if self.total_time.as_millis() < 100 {
            println!("   ⚡ Very fast processing");
        } else if self.total_time.as_millis() < 500 {
            println!("   🚀 Fast processing");
        } else {
            println!("   🐌 Slow processing");
        }
        println!("========================");
    }
}

fn main() {
    let total_start = Instant::now();
    let mut metrics = PerformanceMetrics::new();
    
    println!("Testing PSK text transmission with performance analysis...");
    
    // PSK Configuration
    let sample_rate = 48000.0;
    let carrier_freq = 10000.0;
    let symbol_rate = 1000.0;
    
    let modulator = PskModulator::new(sample_rate, carrier_freq, symbol_rate);
    let demodulator = PskDemodulator::new(sample_rate, carrier_freq, symbol_rate);
    
    // Read text from file with timing
    let read_start = Instant::now();
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
    metrics.file_read_time = read_start.elapsed();
    
    let text_bytes = text_message.as_bytes();
    metrics.original_bytes = text_bytes.len();
    
    println!("Original text: {} chars, {} bytes", text_message.chars().count(), text_bytes.len());
    
    // Calculate frames needed
    let bytes_per_frame = 11; // 88 bits / 8 = 11 bytes
    let total_frames = (text_bytes.len() + bytes_per_frame - 1) / bytes_per_frame;
    metrics.transmitted_frames = total_frames;
    
    println!("Frames needed: {}", total_frames);
    
    // Generate preamble
    let preamble = psk_utils::generate_chirp_preamble(
        sample_rate,
        2000.0,
        10000.0,
        440
    );
    
    let mut signal = Vec::new();
    
    // Create frames and modulate with timing
    let encoding_start = Instant::now();
    let mut frames = Vec::new();
    
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
        frames.push(frame_crc);
    }
    metrics.encoding_time = encoding_start.elapsed();
    
    // Modulation timing
    let modulation_start = Instant::now();
    let mut modulated_frames = Vec::new();
    for frame in &frames {
        let frame_wave = modulator.modulate_bpsk(frame);
        modulated_frames.push(frame_wave);
    }
    metrics.modulation_time = modulation_start.elapsed();
    
    // Signal generation timing
    let signal_gen_start = Instant::now();
    for (frame_idx, frame_wave) in modulated_frames.iter().enumerate() {
        // Add preamble and frame
        signal.extend(&preamble);
        signal.extend(frame_wave);
        signal.extend(vec![0.0; 100]); // Inter-frame spacing
        
        if frame_idx < 5 || frame_idx >= total_frames - 5 {
            println!("Frame {}: ID={}, samples={}", frame_idx + 1, frame_idx + 1, frame_wave.len());
        } else if frame_idx == 5 {
            println!("... (showing first and last 5 frames) ...");
        }
    }
    metrics.signal_generation_time = signal_gen_start.elapsed();
    metrics.signal_samples = signal.len();
    
    println!("Total signal length: {} samples", signal.len());
    
    // Now demodulate the signal
    println!("\n=== DEMODULATION ===");
    
    let samples_per_symbol = (sample_rate / symbol_rate) as usize;
    let frame_length_samples = 108 * samples_per_symbol;
    let preamble_length = preamble.len();
    
    // Cross-correlate to find frames with timing
    let correlation_start = Instant::now();
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
    metrics.correlation_time = correlation_start.elapsed();
    
    println!("Found {} potential frames", frame_starts.len());
    
    // Demodulate frames with timing
    let demodulation_start = Instant::now();
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
                    if frame_idx < 5 || frame_idx >= frame_starts.len() - 5 {
                        println!("Frame {}: ID={}, demodulated successfully", frame_idx + 1, frame_id);
                    } else if frame_idx == 5 {
                        println!("... (showing first and last 5 frames) ...");
                    }
                    correct_frames += 1;
                } else {
                    println!("Frame {}: Invalid ID={}", frame_idx + 1, frame_id);
                }
            } else {
                println!("Frame {}: Insufficient bits", frame_idx + 1);
            }
        }
    }
    metrics.demodulation_time = demodulation_start.elapsed();
    metrics.received_frames = correct_frames;
    
    println!("Correct frames: {} / {}", correct_frames, frame_starts.len());
    
    // Calculate quality metrics
    metrics.frame_loss_rate = if metrics.transmitted_frames > 0 {
        (metrics.transmitted_frames - metrics.received_frames) as f32 / metrics.transmitted_frames as f32
    } else {
        0.0
    };
    
    // Reconstruct text with timing
    let reconstruction_start = Instant::now();
    if !received_frames.is_empty() {
        received_frames.sort_by_key(|(id, _)| *id);
        
        let mut reconstructed_text = Vec::new();
        for (frame_id, data_bytes) in received_frames {
            if frame_id <= 5 || frame_id > metrics.transmitted_frames as u8 - 5 {
                println!("Processing frame ID={}, data={:?}", frame_id, data_bytes);
            } else if frame_id == 6 {
                println!("... (processing frames 6-{}) ...", metrics.transmitted_frames - 5);
            }
            
            for &byte in &data_bytes {
                if byte != 0 {
                    reconstructed_text.push(byte);
                } else {
                    break;
                }
            }
        }
        
        metrics.text_reconstruction_time = reconstruction_start.elapsed();
        metrics.total_time = total_start.elapsed();
        
        match String::from_utf8(reconstructed_text.clone()) {
            Ok(text) => {
                println!("\n=== RECEIVED TEXT ===");
                
                // Calculate accuracy metrics
                let original_chars: Vec<char> = text_message.chars().collect();
                let received_chars: Vec<char> = text.chars().collect();
                
                let mut matching_chars = 0;
                let min_len = std::cmp::min(original_chars.len(), received_chars.len());
                for i in 0..min_len {
                    if original_chars[i] == received_chars[i] {
                        matching_chars += 1;
                    }
                }
                
                metrics.character_accuracy = if original_chars.len() > 0 {
                    matching_chars as f32 / original_chars.len() as f32
                } else {
                    0.0
                };
                
                let mut byte_errors = 0;
                let original_bytes = text_message.as_bytes();
                let received_bytes = text.as_bytes();
                let min_byte_len = std::cmp::min(original_bytes.len(), received_bytes.len());
                for i in 0..min_byte_len {
                    if original_bytes[i] != received_bytes[i] {
                        byte_errors += 1;
                    }
                }
                byte_errors += (original_bytes.len() as i32 - received_bytes.len() as i32).abs() as usize;
                
                metrics.byte_error_rate = if original_bytes.len() > 0 {
                    byte_errors as f32 / original_bytes.len() as f32
                } else {
                    0.0
                };
                
                // Only show part of text if it's long
                if text.len() > 200 {
                    println!("{}...\n[Text truncated, showing first 200 chars]", &text[..200]);
                } else {
                    println!("{}", text);
                }
                println!("=== END ===");
                
                // Compare with original
                if text == text_message {
                    println!("✅ TEXT TRANSMISSION SUCCESSFUL!");
                } else {
                    println!("⚠️  Text transmission has differences:");
                    println!("   Expected length: {} chars", text_message.chars().count());
                    println!("   Received length: {} chars", text.chars().count());
                    
                    // Find first difference
                    for (i, (orig_char, recv_char)) in original_chars.iter().zip(received_chars.iter()).enumerate() {
                        if orig_char != recv_char {
                            println!("   First difference at position {}: '{}' vs '{}'", i, orig_char, recv_char);
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                println!("❌ UTF-8 conversion error: {}", e);
                println!("Raw bytes: {:?}", &reconstructed_text[..std::cmp::min(50, reconstructed_text.len())]);
                if reconstructed_text.len() > 50 {
                    println!("... ({} total bytes)", reconstructed_text.len());
                }
                
                metrics.character_accuracy = 0.0;
                metrics.byte_error_rate = 1.0;
            }
        }
    } else {
        metrics.text_reconstruction_time = reconstruction_start.elapsed();
        metrics.total_time = total_start.elapsed();
        metrics.character_accuracy = 0.0;
        metrics.byte_error_rate = 1.0;
        println!("❌ No valid frames received");
    }
    
    // Print performance summary
    metrics.print_summary();
}
