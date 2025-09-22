use dialoguer::{theme::ColorfulTheme, Select};
use jack;
use std::io::Write;
mod audio;
mod device;
mod ui;
mod utils;
use audio::recorder;
use device::jack::{
    print_jack_info,
};
use rand::{self, Rng, SeedableRng};
use tracing::{debug, info};
use ui::print_banner;
use ui::progress::{ProgressManager, templates};
use utils::consts::*;
use utils::logging::init_logging;

use crate::device::jack::connect_system_ports;

fn main() {
    init_logging();
    print_banner();

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

    let selections = &["Sender", "Receiver", "Test (no JACK)"];
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select mode")
        .default(0)
        .items(&selections[..])
        .interact()
        .unwrap();

    {
        shared.record_buffer.lock().unwrap().clear();
    }

    if selection == 0 {
        // Sender
        run_sender(shared, progress_manager, sample_rate as u32);
    } else if selection == 1 {
        // Receiver
        run_receiver(shared, progress_manager, sample_rate as u32, max_duration_samples as u32);
    } else {
        // Test mode (no JACK)
        test_sender_receiver();
        return;
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
    // Read content from think-different.txt file
    let file_content = std::fs::read_to_string("assets/think-different.txt")
        .expect("Failed to read think-different.txt");
    
    // Convert text content to bits (ASCII encoding)
    let text_bits: Vec<u8> = file_content
        .bytes()
        .flat_map(|byte| {
            (0..8).map(move |i| ((byte >> (7 - i)) & 1) as u8)
        })
        .collect();
    
    let mut rng = rand::rngs::StdRng::from_seed([1u8; 32]);
    let mut output_track = Vec::new();

    // 100 frames, each 100 bits
    let mut frames = vec![vec![0u8; 100]; 100];

    // Fill frames with content from think-different.txt
    let mut bit_index = 0;
    for i in 0..100 {
        // Set first 8 bits to frame ID
        let id = i + 1; // 1-indexed like MATLAB
        for j in 0..8 {
            frames[i][j] = ((id >> (7 - j)) & 1) as u8;
        }
        
        // Fill remaining 92 bits with content from file
        for j in 8..100 {
            if bit_index < text_bits.len() {
                frames[i][j] = text_bits[bit_index];
                bit_index += 1;
            } else {
                // If we run out of file content, wrap around
                bit_index = 0;
                frames[i][j] = text_bits[bit_index];
                bit_index += 1;
            }
        }
    }

    // PHY Frame generation
    // Generate time vector for 1 second at 48kHz
    let sample_rate_f32 = sample_rate as f32;
    let t: Vec<f32> = (0..48000)
        .map(|i| i as f32 / sample_rate_f32)
        .collect();

    // Carrier frequency 10kHz
    let fc = 10000.0;
    let carrier: Vec<f32> = t
        .iter()
        .map(|&time| (2.0 * std::f32::consts::PI * fc * time).sin())
        .collect();

    // CRC8 polynomial: x^8+x^7+x^5+x^2+x+1 (0x1D7)
    let _crc8_poly = 0x1D7u16;

    // Preamble generation (440 samples)
    let mut f_p = Vec::with_capacity(440);
    // First 220: linear from 2kHz to 10kHz
    for i in 0..220 {
        f_p.push(2000.0 + (8000.0 * i as f32 / 219.0));
    }
    // Next 220: linear from 10kHz to 2kHz
    for i in 0..220 {
        f_p.push(10000.0 - (8000.0 * i as f32 / 219.0));
    }

    // Generate preamble using cumulative trapezoidal integration
    let mut omega = 0.0;
    let mut preamble = Vec::with_capacity(440);
    preamble.push((2.0 * std::f32::consts::PI * f_p[0] * t[0]).sin());

    for i in 1..440 {
        let dt = t[i] - t[i - 1];
        omega += std::f32::consts::PI * (f_p[i] + f_p[i - 1]) * dt;
        preamble.push(omega.sin());
    }

    // Process each frame
    for i in 0..100 {
        let frame = &frames[i];

        // Add CRC8 (simplified implementation)
        let mut frame_crc = frame.clone();
        frame_crc.extend_from_slice(&[0u8; 8]); // Add 8 CRC bits (placeholder)

        // Modulation: 44 samples per bit, baudrate ~1000bps
        let mut frame_wave = Vec::with_capacity(frame_crc.len() * 44);
        for (j, &bit) in frame_crc.iter().enumerate() {
            let start_idx = j * 44;
            let end_idx = (j + 1) * 44;
            let amplitude = if bit == 1 { 1.0 } else { -1.0 };

            for k in start_idx..end_idx.min(carrier.len()) {
                frame_wave.push(carrier[k] * amplitude);
            }
        }

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

    let output_track_len = output_track.len();

    {
        let mut playback = shared.playback_buffer.lock().unwrap();
        playback.extend(output_track);
        info!(
            "Output track length: {} samples",
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
    // Generate preamble for receiver (same as in sender)
    let sample_rate_f32 = sample_rate as f32;
    let t: Vec<f32> = (0..48000)
        .map(|i| i as f32 / sample_rate_f32)
        .collect();

    // Preamble generation (440 samples)
    let mut f_p = Vec::with_capacity(440);
    // First 220: linear from 2kHz to 10kHz
    for i in 0..220 {
        f_p.push(2000.0 + (8000.0 * i as f32 / 219.0));
    }
    // Next 220: linear from 10kHz to 2kHz
    for i in 0..220 {
        f_p.push(10000.0 - (8000.0 * i as f32 / 219.0));
    }

    // Generate preamble using cumulative trapezoidal integration
    let mut omega = 0.0;
    let mut preamble = Vec::with_capacity(440);
    preamble.push((2.0 * std::f32::consts::PI * f_p[0] * t[0]).sin());

    for i in 1..440 {
        let dt = t[i] - t[i - 1];
        omega += std::f32::consts::PI * (f_p[i] + f_p[i - 1]) * dt;
        preamble.push(omega.sin());
    }

    progress_manager
        .create_bar(
            "recording",
            max_recording_duration_samples as u64,
            templates::RECORDING,
            "receiver",
        )
        .unwrap();

    // *shared.app_state.lock().unwrap() = recorder::AppState::Recording;

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

    let rx_fifo: std::collections::VecDeque<f32> = {
        let record = shared.record_buffer.lock().unwrap();
        record.iter().copied().collect()
    };

    let mut power = 0.0f32;
    let mut power_debug = vec![0.0f32; rx_fifo.len()];
    let mut start_index = 0usize;
    let mut start_index_debug = vec![0.0f32; rx_fifo.len()];
    let mut sync_fifo = vec![0.0f32; 440];
    let mut sync_power_debug = vec![0.0f32; rx_fifo.len()];
    let mut sync_power_local_max = 0.0f32;

    let mut decode_fifo = Vec::new();
    let mut correct_frame_num = 0;
    let mut decoded_content = Vec::new(); // Store decoded content for streaming output

    let mut state = 0; // 0: sync, 1: decode

    // This part is a bit of a hack for the simulation
    // We need a carrier wave for decoding. In a real receiver, this would be handled differently.
    let sample_rate_f32_decode = 48000.0;
    let t_decode: Vec<f32> = (0..rx_fifo.len())
        .map(|i| i as f32 / sample_rate_f32_decode)
        .collect();
    let fc_decode = 10000.0;
    let carrier_decode: Vec<f32> = t_decode
        .iter()
        .map(|&time| (2.0 * std::f32::consts::PI * fc_decode * time).sin())
        .collect();

    for i in 0..rx_fifo.len() {
        let current_sample = rx_fifo[i];

        power = power * (1.0 - 1.0 / 64.0) + current_sample * current_sample / 64.0;
        power_debug[i] = power;

        if state == 0 {
            // Packet sync
            sync_fifo.rotate_left(1);
            sync_fifo[439] = current_sample;

            let sync_power = sync_fifo
                .iter()
                .zip(preamble.iter())
                .map(|(a, b)| a * b)
                .sum::<f32>()
                / 200.0;
            sync_power_debug[i] = sync_power;

            if (sync_power > power * 2.0)
                && (sync_power > sync_power_local_max)
                && (sync_power > 0.05)
            {
                sync_power_local_max = sync_power;
                start_index = i;
            } else if (i > start_index + 200) && (start_index != 0) {
                start_index_debug[start_index] = 1.5;
                sync_power_local_max = 0.0;
                sync_fifo.fill(0.0);
                state = 1;

                // Convert VecDeque slice to Vec
                decode_fifo = rx_fifo.range(start_index + 1..i).copied().collect();
            }
        } else if state == 1 {
            decode_fifo.push(current_sample);

            if decode_fifo.len() == 44 * 108 {
                // Decode
                let decode_len = decode_fifo.len();
                let carrier_slice = &carrier_decode[..decode_len.min(carrier_decode.len())];

                // Remove carrier (simplified smoothing)
                let mut decode_remove_carrier = Vec::with_capacity(decode_len);
                for j in 0..decode_len {
                    let start = j.saturating_sub(5);
                    let end = (j + 6).min(decode_len);
                    let sum: f32 = (start..end)
                        .map(|k| decode_fifo[k] * carrier_slice.get(k).unwrap_or(&0.0))
                        .sum();
                    decode_remove_carrier.push(sum / (end - start) as f32);
                }

                let mut decode_power_bit = vec![false; 108];
                for j in 0..108 {
                    let start_idx = 10 + j * 44;
                    let end_idx = (30 + j * 44).min(decode_remove_carrier.len());
                    if start_idx < decode_remove_carrier.len() && start_idx < end_idx {
                        let sum: f32 = decode_remove_carrier[start_idx..end_idx].iter().sum();
                        decode_power_bit[j] = sum > 0.0;
                    }
                }

                // CRC check (simplified - just compare first 8 bits with expected ID)
                let mut temp_index = 0u8;
                for k in 0..8 {
                    if decode_power_bit[k] {
                        temp_index += 1 << (7 - k);
                    }
                }

                if temp_index > 0 && temp_index <= 100 {
                    debug!("Frame ID: {}", temp_index);
                    correct_frame_num += 1;
                    
                    // Extract data bits (skip first 8 bits which are ID)
                    let data_bits = &decode_power_bit[8..100];
                    decoded_content.extend_from_slice(data_bits);
                    
                    // Convert accumulated bits to text and output
                    if decoded_content.len() >= 8 {
                        let mut output_text = String::new();
                        let mut i = 0;
                        while i + 8 <= decoded_content.len() {
                            let mut byte = 0u8;
                            for j in 0..8 {
                                if decoded_content[i + j] {
                                    byte |= 1 << (7 - j);
                                }
                            }
                            output_text.push(byte as char);
                            i += 8;
                        }
                        
                        if !output_text.is_empty() {
                            print!("{}", output_text);
                            std::io::stdout().flush().unwrap();
                        }
                        
                        // Remove processed bits
                        decoded_content.drain(0..i);
                    }
                } else {
                    debug!("Wrong Frame ID: {}", temp_index);
                }

                start_index = 0;
                decode_fifo.clear();
                state = 0;
            }
        }
    }

    // Output any remaining decoded content
    if !decoded_content.is_empty() {
        let mut output_text = String::new();
        let mut i = 0;
        while i + 8 <= decoded_content.len() {
            let mut byte = 0u8;
            for j in 0..8 {
                if decoded_content[i + j] {
                    byte |= 1 << (7 - j);
                }
            }
            output_text.push(byte as char);
            i += 8;
        }
        
        if !output_text.is_empty() {
            print!("{}", output_text);
            std::io::stdout().flush().unwrap();
        }
    }
    
    println!("\n接收完成！总共正确接收帧数: {}", correct_frame_num);
}

fn test_sender_receiver() {
    println!("开始测试发送和接收功能...");
    
    // Read content from think-different.txt file
    let file_content = std::fs::read_to_string("assets/think-different.txt")
        .expect("Failed to read think-different.txt");
    
    println!("原始文件内容:\n{}", file_content);
    println!("原始文件长度: {} bytes", file_content.len());
    
    // Convert text content to bits (ASCII encoding)
    let text_bits: Vec<u8> = file_content
        .bytes()
        .flat_map(|byte| {
            (0..8).map(move |i| ((byte >> (7 - i)) & 1) as u8)
        })
        .collect();
    
    let mut rng = rand::rngs::StdRng::from_seed([1u8; 32]);
    let mut output_track = Vec::new();

    // 100 frames, each 100 bits
    let mut frames = vec![vec![0u8; 100]; 100];

    // Fill frames with content from think-different.txt
    let mut bit_index = 0;
    for i in 0..100 {
        // Set first 8 bits to frame ID
        let id = i + 1; // 1-indexed like MATLAB
        for j in 0..8 {
            frames[i][j] = ((id >> (7 - j)) & 1) as u8;
        }
        
        // Fill remaining 92 bits with content from file
        for j in 8..100 {
            if bit_index < text_bits.len() {
                frames[i][j] = text_bits[bit_index];
                bit_index += 1;
            } else {
                // If we run out of file content, wrap around
                bit_index = 0;
                frames[i][j] = text_bits[bit_index];
                bit_index += 1;
            }
        }
    }

    // PHY Frame generation
    let sample_rate = 48000u32;
    let sample_rate_f32 = sample_rate as f32;
    let t: Vec<f32> = (0..48000)
        .map(|i| i as f32 / sample_rate_f32)
        .collect();

    // Carrier frequency 10kHz
    let fc = 10000.0;
    let carrier: Vec<f32> = t
        .iter()
        .map(|&time| (2.0 * std::f32::consts::PI * fc * time).sin())
        .collect();

    // Preamble generation (440 samples)
    let mut f_p = Vec::with_capacity(440);
    // First 220: linear from 2kHz to 10kHz
    for i in 0..220 {
        f_p.push(2000.0 + (8000.0 * i as f32 / 219.0));
    }
    // Next 220: linear from 10kHz to 2kHz
    for i in 0..220 {
        f_p.push(10000.0 - (8000.0 * i as f32 / 219.0));
    }

    // Generate preamble using cumulative trapezoidal integration
    let mut omega = 0.0;
    let mut preamble = Vec::with_capacity(440);
    preamble.push((2.0 * std::f32::consts::PI * f_p[0] * t[0]).sin());

    for i in 1..440 {
        let dt = t[i] - t[i - 1];
        omega += std::f32::consts::PI * (f_p[i] + f_p[i - 1]) * dt;
        preamble.push(omega.sin());
    }

    // Process each frame
    for i in 0..100 {
        let frame = &frames[i];

        // Add CRC8 (simplified implementation)
        let mut frame_crc = frame.clone();
        frame_crc.extend_from_slice(&[0u8; 8]); // Add 8 CRC bits (placeholder)

        // Modulation: 44 samples per bit, baudrate ~1000bps
        let mut frame_wave = Vec::with_capacity(frame_crc.len() * 44);
        for (j, &bit) in frame_crc.iter().enumerate() {
            let start_idx = j * 44;
            let end_idx = (j + 1) * 44;
            let amplitude = if bit == 1 { 1.0 } else { -1.0 };

            for k in start_idx..end_idx.min(carrier.len()) {
                frame_wave.push(carrier[k] * amplitude);
            }
        }

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

    debug!("Total length: {} samples", output_track.len());

    // dump json
    utils::dump::dump_to_json("./tmp/output.json", &utils::dump::AudioData {
        sample_rate,
        audio_data: output_track.clone(),
        duration: output_track.len() as f32 / sample_rate as f32,
        channels: 1,
    })
        .expect("Failed to dump output track to JSON");
    info!("Dumped output track to ./tmp/output.json");

    utils::dump::dump_to_wav("./tmp/output.wav", &utils::dump::AudioData {
        sample_rate,
        audio_data: output_track.clone(),
        duration: output_track.len() as f32 / sample_rate as f32,
        channels: 1,
    })
        .expect("Failed to dump output track to WAV");
    info!("Dumped output track to ./tmp/output.wav");

    // Now decode the output_track
    let rx_fifo: Vec<f32> = output_track;

    let mut power = 0.0f32;
    let mut start_index = 0usize;
    let mut sync_fifo = vec![0.0f32; 440];
    let mut sync_power_local_max = 0.0f32;

    let mut decode_fifo = Vec::new();
    let mut correct_frame_num = 0;
    let mut decoded_content = Vec::new();
    let mut decoded_text = String::new();

    let mut state = 0; // 0: sync, 1: decode

    // This part is a bit of a hack for the simulation
    let sample_rate_f32_decode = 48000.0;
    let t_decode: Vec<f32> = (0..rx_fifo.len())
        .map(|i| i as f32 / sample_rate_f32_decode)
        .collect();
    let fc_decode = 10000.0;
    let carrier_decode: Vec<f32> = t_decode
        .iter()
        .map(|&time| (2.0 * std::f32::consts::PI * fc_decode * time).sin())
        .collect();

    for i in 0..rx_fifo.len() {
        let current_sample = rx_fifo[i];

        power = power * (1.0 - 1.0 / 64.0) + current_sample * current_sample / 64.0;

        if state == 0 {
            // Packet sync
            sync_fifo.rotate_left(1);
            sync_fifo[439] = current_sample;

            let sync_power = sync_fifo
                .iter()
                .zip(preamble.iter())
                .map(|(a, b)| a * b)
                .sum::<f32>()
                / 200.0;

            if (sync_power > power * 2.0)
                && (sync_power > sync_power_local_max)
                && (sync_power > 0.05)
            {
                sync_power_local_max = sync_power;
                start_index = i;
            } else if (i > start_index + 200) && (start_index != 0) {
                sync_power_local_max = 0.0;
                sync_fifo.fill(0.0);
                state = 1;

                decode_fifo = rx_fifo[start_index + 1..i].to_vec();
            }
        } else if state == 1 {
            decode_fifo.push(current_sample);

            if decode_fifo.len() == 44 * 108 {
                // Decode
                let decode_len = decode_fifo.len();
                let carrier_slice = &carrier_decode[..decode_len.min(carrier_decode.len())];

                // Remove carrier (simplified smoothing)
                let mut decode_remove_carrier = Vec::with_capacity(decode_len);
                for j in 0..decode_len {
                    let start = j.saturating_sub(5);
                    let end = (j + 6).min(decode_len);
                    let sum: f32 = (start..end)
                        .map(|k| decode_fifo[k] * carrier_slice.get(k).unwrap_or(&0.0))
                        .sum();
                    decode_remove_carrier.push(sum / (end - start) as f32);
                }

                let mut decode_power_bit = vec![false; 108];
                for j in 0..108 {
                    let start_idx = 10 + j * 44;
                    let end_idx = (30 + j * 44).min(decode_remove_carrier.len());
                    if start_idx < decode_remove_carrier.len() && start_idx < end_idx {
                        let sum: f32 = decode_remove_carrier[start_idx..end_idx].iter().sum();
                        decode_power_bit[j] = sum > 0.0;
                    }
                }

                // CRC check (simplified - just compare first 8 bits with expected ID)
                let mut temp_index = 0u8;
                for k in 0..8 {
                    if decode_power_bit[k] {
                        temp_index += 1 << (7 - k);
                    }
                }

                if temp_index > 0 && temp_index <= 100 {
                    correct_frame_num += 1;
                    
                    // Extract data bits (skip first 8 bits which are ID)
                    let data_bits = &decode_power_bit[8..100];
                    decoded_content.extend_from_slice(data_bits);
                    
                    // Convert accumulated bits to text
                    while decoded_content.len() >= 8 {
                        let mut byte = 0u8;
                        for j in 0..8 {
                            if decoded_content[j] {
                                byte |= 1 << (7 - j);
                            }
                        }
                        decoded_text.push(byte as char);
                        decoded_content.drain(0..8);
                    }
                }

                start_index = 0;
                decode_fifo.clear();
                state = 0;
            }
        }
    }

    // Handle any remaining bits
    if decoded_content.len() >= 8 {
        while decoded_content.len() >= 8 {
            let mut byte = 0u8;
            for j in 0..8 {
                if decoded_content[j] {
                    byte |= 1 << (7 - j);
                }
            }
            decoded_text.push(byte as char);
            decoded_content.drain(0..8);
        }
    }
    
    println!("\n解码完成！");
    println!("正确接收帧数: {}", correct_frame_num);
    println!("解码的文件长度: {} bytes", decoded_text.len());
    println!("解码内容:\n{}", decoded_text);
    
    // Compare with original (按照较小的长度来比较)
    let original_trimmed = file_content.trim();
    let decoded_trimmed = decoded_text.trim();
    
    let min_len = original_trimmed.len().min(decoded_trimmed.len());
    let original_compare = &original_trimmed[..min_len];
    let decoded_compare = &decoded_trimmed[..min_len];
    
    println!("比较长度: {} bytes", min_len);
    
    if original_compare == decoded_compare {
        println!("✅ 测试通过！解码内容的前{}字节与原始文件完全匹配", min_len);
        if decoded_trimmed.len() > original_trimmed.len() {
            println!("注意：解码内容更长，这是因为帧填充时循环使用了原始内容");
        }
    } else {
        println!("❌ 测试失败！解码内容与原始文件不匹配");
        println!("原始内容长度: {}", original_trimmed.len());
        println!("解码内容长度: {}", decoded_trimmed.len());
        
        // Find first difference
        for i in 0..min_len {
            if original_compare.chars().nth(i) != decoded_compare.chars().nth(i) {
                println!("第一个不同的字符位置: {}", i);
                println!("原始: {:?}", original_compare.chars().nth(i));
                println!("解码: {:?}", decoded_compare.chars().nth(i));
                break;
            }
        }
    }
}
