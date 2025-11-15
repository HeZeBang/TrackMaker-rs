use dialoguer::{Select, theme::ColorfulTheme, Input};
use jack;
use std::fs;
use tracing::{debug, error, info, trace, warn};

mod audio;
mod device;
mod phy;
mod mac;
mod ui;
mod utils;

use audio::recorder;
use device::jack::{connect_system_ports, print_jack_info};
use ui::print_banner;
use ui::progress::{ProgressManager, templates};
use utils::consts::*;
use utils::logging::init_logging;
use rand::Rng;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use phy::{Frame, FrameType, LineCodingKind, PhyDecoder, PhyEncoder};

fn main() {
    init_logging();
    print_banner();

    let (client, status) = jack::Client::new(
        format!("{}_{:04}", JACK_CLIENT_NAME, rand::rng().random_range(0..10000)).as_str(),
        jack::ClientOptions::NO_START_SERVER,
    )
    .unwrap();
    tracing::info!("JACK client status: {:?}", status);
    let (sample_rate, _buffer_size) = print_jack_info(&client);

    if sample_rate as u32 != SAMPLE_RATE {
        warn!(
            "Sample rate mismatch! Expected {}, got {}",
            SAMPLE_RATE,
            sample_rate
        );
        warn!("Physical layer is designed for {} Hz", SAMPLE_RATE);
    }

    let max_duration_samples = sample_rate * DEFAULT_RECORD_SECONDS;

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

    let selections = &["Send File", "Receive File", "Test (No JACK - Loopback)"];
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select mode")
        .default(0)
        .items(&selections[..])
        .interact()
        .unwrap();

    let line_coding_options = [
        LineCodingKind::FourBFiveB,
        LineCodingKind::Manchester,
    ];
    let line_coding_labels = ["4B5B (NRZ)", "Manchester (Bi-phase)"];
    let line_coding_idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select line coding scheme")
        .default(0)
        .items(&line_coding_labels)
        .interact()
        .unwrap();
    let line_coding = line_coding_options[line_coding_idx];

    {
        shared
            .record_buffer
            .lock()
            .unwrap()
            .clear();
    }

    if selection == 0 {
        // Sender
        let tx_addr = Input::<mac::types::MacAddr>::with_theme(&ColorfulTheme::default())
            .with_prompt("Enter local sender addr")
            .default(1)
            .interact()
            .unwrap();
        let rx_addr = Input::<mac::types::MacAddr>::with_theme(&ColorfulTheme::default())
            .with_prompt("Enter remote receiver addr")
            .default(2)
            .interact()
            .unwrap();
        run_sender(shared, progress_manager, sample_rate as u32, line_coding, tx_addr, rx_addr);
    } else if selection == 1 {
        // Receiver
        let rx_addr = Input::<mac::types::MacAddr>::with_theme(&ColorfulTheme::default())
            .with_prompt("Enter local receiver addr")
            .default(2)
            .interact()
            .unwrap();
        let tx_addr = Input::<mac::types::MacAddr>::with_theme(&ColorfulTheme::default())
            .with_prompt("Enter remote sender addr")
            .default(1)
            .interact()
            .unwrap();
        run_receiver(
            shared,
            progress_manager,
            max_duration_samples as u32,
            line_coding,
            rx_addr,
            tx_addr
        );
    } else {
        // Test mode (no JACK)
        test_transmission(line_coding);
        return;
    }

    info!("Exiting gracefully...");
    if let Err(err) = active_client.deactivate() {
        error!("Error deactivating client: {}", err);
    }
}

fn run_sender(
    shared: recorder::AppShared,
    progress_manager: ProgressManager,
    sample_rate: u32,
    line_coding: LineCodingKind,
    sender_addr: mac::types::MacAddr,
    receiver_addr: mac::types::MacAddr,
) {
    info!("=== Sender Mode (with Stop-and-Wait) ===");
    info!("Using line coding: {}", line_coding.name());

    // Read input file
    let input_path = "INPUT.bin";
    let file_data = match fs::read(input_path) {
        Ok(data) => {
            info!("Read {} bytes from {}", data.len(), input_path);
            data
        }
        Err(e) => {
            error!("Failed to read {}: {}", input_path, e);
            return;
        }
    };

    // Create PHY encoder and decoder (for ACKs)
    let encoder = PhyEncoder::new(
        SAMPLES_PER_LEVEL,
        PREAMBLE_PATTERN_BYTES,
        line_coding,
    );
    let mut decoder = PhyDecoder::new(
        SAMPLES_PER_LEVEL,
        PREAMBLE_PATTERN_BYTES,
        line_coding,
        sender_addr,
    );

    // Split data into frames
    let mut frames = Vec::new();
    let mut seq = 0u8;
    for chunk in file_data.chunks(MAX_FRAME_DATA_SIZE) {
        let frame = Frame::new_data(seq, sender_addr, receiver_addr, chunk.to_vec()); // TODO: change this!
        frames.push(frame);
        seq = seq.wrapping_add(1);
    }

    let mut state = mac::CSMAState::Idle;
    info!("Created {} frames to send.", frames.len());

    let total_frames = frames.len();
    let mut frames_sent = 0;

    let sender_progress = progress_manager
        .create_bar(
            "sender",
            total_frames as u64,
            templates::SENDER,
            "sender",
        )
        .unwrap();

    let overall_start_time = std::time::Instant::now();

    for frame_to_send in &frames {
        state = mac::CSMAState::Sensing;
        *shared.app_state.lock().unwrap() = recorder::AppState::Recording;
        let mut stage = 0;

        'csma_loop: loop {
            match state {
                mac::CSMAState::Sensing => {
                    std::thread::sleep(std::time::Duration::from_millis(ENERGY_DETECTION_SAMPLES as u64 * 1000 / sample_rate as u64));
                    let recorded_samples = { shared.record_buffer.lock().unwrap().clone() };
                    match mac::is_channel_busy(&recorded_samples) {
                        Some(true) => {
                            trace!("Channel busy detected during sensing.");
                            shared.record_buffer.lock().unwrap().clear();
                        }
                        Some(false) => {
                            state = mac::CSMAState::WaitingForDIFS;
                            shared.record_buffer.lock().unwrap().clear();
                        }
                        None => {
                            trace!("Not enough samples to determine channel state during sensing.");
                            continue 'csma_loop;
                        }
                    }
                }
                mac::CSMAState::Backoff(mut counter) => {
                    if counter > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(SLOT_TIME_MS));
                        match mac::is_channel_busy(&{ shared.record_buffer.lock().unwrap().clone() }) {
                            Some(true) => {
                                trace!("Channel busy detected during backoff.");
                                shared.record_buffer.lock().unwrap().clear();
                                state = mac::CSMAState::BackoffPaused(counter);
                            }
                            Some(false) => {
                                // Channel idle, continue countdown
                                shared.record_buffer.lock().unwrap().clear();
                                counter -= 1;
                                state = mac::CSMAState::Backoff(counter);
                            }
                            None => {
                                trace!("Not enough samples to determine channel state during backoff.");
                            }
                        }
                    } else {
                        state = mac::CSMAState::Transmitting;
                    }
                }
                mac::CSMAState::BackoffPaused(counter) => {
                    match mac::is_channel_busy(&{ shared.record_buffer.lock().unwrap().clone() }) {
                        Some(true) => {
                            trace!("Channel still busy during backoff pause.");
                            std::thread::sleep(std::time::Duration::from_millis(SLOT_TIME_MS / 2));
                            shared.record_buffer.lock().unwrap().clear();
                            state = mac::CSMAState::BackoffPaused(counter);
                        }
                        Some(false) => {
                            trace!("Channel idle again, resuming backoff.");
                            shared.record_buffer.lock().unwrap().clear();

                            // 等待一个 DIFS 周期
                            std::thread::sleep(std::time::Duration::from_millis(DIFS_DURATION_MS));
                            
                            // DIFS 结束后，必须再次检查信道，因为可能有别人在我们等待时开始发送
                            if let Some(false) = mac::is_channel_busy(&{ shared.record_buffer.lock().unwrap().clone() }) {
                                // 如果信道在 DIFS 后仍然空闲，那么我们可以恢复倒计时
                                trace!("DIFS wait over, channel still idle. Resuming backoff.");
                                state = mac::CSMAState::Backoff(counter);
                            } else {
                                // 如果在 DIFS 期间信道又变忙了，我们必须保持 Paused 状态
                                trace!("Channel became busy during DIFS wait. Staying paused.");
                            }
                            shared.record_buffer.lock().unwrap().clear();
                        }
                        None => {
                            trace!("Not enough samples to determine channel state during backoff pause.");
                        }
                    }
                }
                mac::CSMAState::WaitingForDIFS => {
                    std::thread::sleep(std::time::Duration::from_millis(DIFS_DURATION_MS));

                    match mac::is_channel_busy(&{ shared.record_buffer.lock().unwrap().clone() }) {
                        Some(false) => {
                            trace!("DIFS wait is over and channel is still idle. Starting backoff.");
                            let cw = (CW_MIN as u16 * 2_u16.pow(stage as u32)).min(CW_MAX as u16) as u8;
                            state = mac::CSMAState::Backoff(rand::random_range(0..=cw));
                        }
                        Some(true) => {
                            trace!("Channel became busy during DIFS wait. Returning to sensing.");
                            state = mac::CSMAState::Sensing;
                        }
                        None => !unreachable!(), // We always have enough samples after DIFS
                    }
                    shared.record_buffer.lock().unwrap().clear();
                }
                mac::CSMAState::Transmitting => {
                    debug!("Channel idle, proceeding to transmit frame seq: {}", frame_to_send.sequence);
                    // 1. Encode and send the frame
                    let output_track = encoder.encode_frames(&[frame_to_send.clone()], INTER_FRAME_GAP_SAMPLES);
                    {
                        let mut playback = shared.playback_buffer.lock().unwrap();
                        playback.clear();
                        playback.extend(output_track);
                    }
                    *shared.app_state.lock().unwrap() = recorder::AppState::Playing;

                    // Wait for playback to finish
                    while let recorder::AppState::Playing = { shared.app_state.lock().unwrap().clone() } {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    debug!("Frame {} sent, waiting for ACK...", frame_to_send.sequence);

                    // 2. Switch to recording to wait for ACK
                    {
                        // Clear previous recordings before listening for ACK
                        let mut rec_buf = shared.record_buffer.lock().unwrap();
                        rec_buf.clear();
                    }
                    *shared.app_state.lock().unwrap() = recorder::AppState::Recording;
                    state = mac::CSMAState::WaitingForAck;
                }
                mac::CSMAState::WaitingForAck => {
                    let mut processed_samples_len = 0;
                    let ack_wait_start = std::time::Instant::now();
                    // Timeout for ACK
                    let ack_timeout = std::time::Duration::from_millis(ACK_TIMEOUT_MS);

                    // 3. ACK waiting loop
                    'ack_wait_loop: loop {
                        if ack_wait_start.elapsed() > ack_timeout {
                            warn!("ACK timeout for seq: {}", frame_to_send.sequence);
                            stage += 1;
                            let cw = (CW_MIN as u16 * 2_u16.pow(stage as u32)).min(CW_MAX as u16) as u8; // BEB
                            state = mac::CSMAState::Backoff(rand::random_range(0..=cw));
                            break 'ack_wait_loop; // Timed out, retransmit
                        }

                        std::thread::sleep(std::time::Duration::from_millis(5));

                        let current_samples = { shared.record_buffer.lock().unwrap().clone() };

                        if current_samples.len() > processed_samples_len {
                            let new_samples = &current_samples[processed_samples_len..];
                            let decoded_frames = decoder.process_samples(new_samples);
                            processed_samples_len = current_samples.len();

                            for ack_frame in decoded_frames {
                                if ack_frame.frame_type == FrameType::Ack && ack_frame.sequence == frame_to_send.sequence {
                                    debug!("ACK received for seq: {}", frame_to_send.sequence);
                                    frames_sent += 1;
                                    progress_manager.inc("sender", 1).unwrap();
                                    break 'csma_loop; // ACK OK, send next frame
                                } else {
                                    warn!(
                                        "Received unexpected frame while waiting for ACK {}: type={:?}, seq={}",
                                        frame_to_send.sequence,
                                        ack_frame.frame_type,
                                        ack_frame.sequence
                                    );
                                }
                            }
                        }
                    } // end ack_wait_loop
                }
                mac::CSMAState::Idle => unreachable!(),
            } // end retransmit_loop
        } // end csma_loop
    } // end for frame_to_send

    progress_manager.finish("sender", "All frames acknowledged").unwrap();
    let total_duration = overall_start_time.elapsed().as_secs_f32();
    info!(
        "🎉 All {} frames transmitted and acknowledged in {:.2} seconds.",
        total_frames, total_duration
    );

    // // Save final received signal for debugging
    // if let Err(e) = utils::dump::dump_to_wav(
    //     "./tmp/sender_final_ack_recording.wav",
    //     &utils::dump::AudioData {
    //         sample_rate,
    //         audio_data: shared.record_buffer.lock().unwrap().clone(),
    //         duration: shared.record_buffer.lock().unwrap().len() as f32 / sample_rate as f32,
    //         channels: 1,
    //     },
    // ) {
    //     warn!("Failed to save sender's final recording: {}", e);
    // }
}

fn run_receiver(
    shared: recorder::AppShared,
    progress_manager: ProgressManager,
    max_recording_duration_samples: u32,
    line_coding: LineCodingKind,
    receiver_addr: mac::types::MacAddr,
    sender_addr: mac::types::MacAddr,
) {
    info!("=== Receiver Mode ===");
    info!("Using line coding: {}", line_coding.name());

    // Create decoder and encoder for ACKs
    let mut decoder = PhyDecoder::new(
        SAMPLES_PER_LEVEL,
        PREAMBLE_PATTERN_BYTES,
        line_coding,
        receiver_addr,
    );
    let encoder = PhyEncoder::new(
        SAMPLES_PER_LEVEL,
        PREAMBLE_PATTERN_BYTES,
        line_coding,
    );

    let mut all_data = Vec::new();
    let mut received_sequences = std::collections::HashSet::new();
    let mut processed_samples_len = 0;

    let progress_bar = progress_manager
        .create_bar(
            "recording",
            max_recording_duration_samples as u64,
            templates::RECORDING,
            "receiver",
        )
        .unwrap();

    *shared.app_state.lock().unwrap() = recorder::AppState::Recording;

    let start_time = std::time::Instant::now();
    let recording_timeout = std::time::Duration::from_secs(60); // Increased timeout

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    // Ctrl+C 设置标志
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");

    'main_loop: loop {
        if !running.load(Ordering::SeqCst) { break; }

        // Check for overall timeout
        if start_time.elapsed() > recording_timeout {
            info!("Receiver timeout reached. Exiting.");
            break 'main_loop;
        }

        // Wait for some audio to be recorded
        std::thread::sleep(std::time::Duration::from_millis(50));

        let current_samples = {
            let buffer = shared.record_buffer.lock().unwrap();
            buffer.clone()
        };

        if current_samples.len() > processed_samples_len {
            let new_samples = &current_samples[processed_samples_len..];
            let decoded_frames = decoder.process_samples(new_samples);
            processed_samples_len = current_samples.len();

            for frame in decoded_frames {
                if frame.frame_type == FrameType::Data {
                    if !received_sequences.contains(&frame.sequence) {
                        debug!("Received new DATA frame with seq: {}", frame.sequence);
                        // Store data and mark sequence as received
                        all_data.push((frame.sequence, frame.data.clone()));
                        received_sequences.insert(frame.sequence);
                    } else {
                        info!("Received duplicate DATA frame with seq: {}, re-sending ACK.", frame.sequence);
                    }

                    // Always send an ACK for a data frame, even if it's a duplicate.
                    // This handles the case where our ACK was lost and the sender retransmitted.
                    debug!("Sending ACK for seq: {}", frame.sequence);
                    let ack_frame = Frame::new_ack(frame.sequence, receiver_addr, sender_addr); // TODO: change this to actual sender addr
                    let ack_track = encoder.encode_frames(&[ack_frame], 0);

                    // Put ACK in playback buffer
                    {
                        let mut playback = shared.playback_buffer.lock().unwrap();
                        playback.clear();
                        playback.extend(ack_track);
                    }

                    // Switch to playing state
                    *shared.app_state.lock().unwrap() = recorder::AppState::Playing;

                    // Wait for ACK playback to complete
                    while let recorder::AppState::Playing = { shared.app_state.lock().unwrap().clone() } {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    debug!("ACK sent for seq: {}", frame.sequence);

                    // After sending ACK, switch back to recording for the next frame
                    *shared.app_state.lock().unwrap() = recorder::AppState::Recording;
                    debug!("Switched back to recording mode.");
                }
            }
        }

        progress_manager.set_position("recording", processed_samples_len as u64).unwrap();

        // Check if user manually stopped (e.g., by letting recording finish)
        let state = { shared.app_state.lock().unwrap().clone() };
        if let recorder::AppState::Idle = state {
            info!("Recording finished by user or duration limit.");
            break 'main_loop;
        }
    }

    let elapsed = start_time.elapsed().as_secs_f32();
    info!("Receiver loop finished in {:.2} seconds", elapsed);
    progress_manager.finish("recording", "Finished").unwrap();

    // Final processing for any remaining samples
    let final_samples = {
        let buffer = shared.record_buffer.lock().unwrap();
        buffer.clone()
    };
    if final_samples.len() > processed_samples_len {
        let remaining_samples = &final_samples[processed_samples_len..];
        let decoded_frames = decoder.process_samples(remaining_samples);
        for frame in decoded_frames {
             if frame.frame_type == FrameType::Data && !received_sequences.contains(&frame.sequence) {
                info!("Decoded final DATA frame with seq: {}", frame.sequence);
                all_data.push((frame.sequence, frame.data.clone()));
                received_sequences.insert(frame.sequence);
            }
        }
    }

    info!("Total unique data frames received: {}", all_data.len());

    // Save recorded signal to WAV
    let sample_rate = SAMPLE_RATE;
    if let Err(e) = utils::dump::dump_to_wav(
        "./tmp/receiver_input.wav",
        &utils::dump::AudioData {
            sample_rate,
            audio_data: final_samples.clone(),
            duration: final_samples.len() as f32 / sample_rate as f32,
            channels: 1,
        },
    ) {
        warn!("Failed to save receiver WAV: {}", e);
    } else {
        info!("Saved received signal to ./tmp/receiver_input.wav");
    }

    // Reconstruct file data
    all_data.sort_by_key(|k| k.0);
    let output_data: Vec<u8> = all_data.into_iter().flat_map(|(_, data)| data).collect();

    debug!("Reconstructed {} bytes", output_data.len());

    // Write to output file
    let output_path = "OUTPUT.bin";
    match fs::write(output_path, &output_data) {
        Ok(_) => debug!("Written to {}", output_path),
        Err(e) => error!("Failed to write {}: {}", output_path, e),
    }
}

fn test_transmission(line_coding: LineCodingKind) {
    info!("=== Test Mode (Loopback without JACK) ===");
    info!("Using line coding: {}", line_coding.name());

    // Create test data
    let test_text = format!(
        "114514Hello, Project 2! This is a test of cable-based transmission using {} line coding.",
        line_coding.name()
    );
    let test_data = test_text.into_bytes();
    info!("Test data: {} bytes", test_data.len());
    info!("Content: {}", String::from_utf8_lossy(&test_data));

    // Create encoder and decoder
    let encoder = PhyEncoder::new(
        SAMPLES_PER_LEVEL,
        PREAMBLE_PATTERN_BYTES,
        line_coding,
    );
    let mut decoder = PhyDecoder::new(
        SAMPLES_PER_LEVEL,
        PREAMBLE_PATTERN_BYTES,
        line_coding,
        2
    );

    // Create frames
    let mut frames = Vec::new();
    let mut seq = 0u8;

    for chunk in test_data.chunks(MAX_FRAME_DATA_SIZE) {
        let frame = Frame::new_data(seq, 0, 1, chunk.to_vec());
        frames.push(frame);
        seq = seq.wrapping_add(1);
    }

    info!("Created {} frames", frames.len());

    // Encode
    let samples = encoder.encode_frames(&frames, INTER_FRAME_GAP_SAMPLES);
    info!(
        "Encoded to {} samples ({:.2} seconds at {} Hz)",
        samples.len(),
        samples.len() as f32 / SAMPLE_RATE as f32,
        SAMPLE_RATE
    );

    // Save to WAV for inspection
    if let Err(e) = utils::dump::dump_to_wav(
        "./tmp/project2_test.wav",
        &utils::dump::AudioData {
            sample_rate: SAMPLE_RATE,
            audio_data: samples.clone(),
            duration: samples.len() as f32 / SAMPLE_RATE as f32,
            channels: 1,
        },
    ) {
        warn!("Failed to save WAV: {}", e);
    } else {
        info!("Saved test signal to ./tmp/project2_test.wav");
    }

    // Decode
    let decoded_frames = decoder.process_samples(&samples);
    info!("Decoded {} frames", decoded_frames.len());

    // Reconstruct data
    let mut decoded_data = Vec::new();
    for frame in decoded_frames {
        decoded_data.extend_from_slice(&frame.data);
    }

    // Compare
    if decoded_data == test_data {
        info!("✅ Test PASSED - Data matches perfectly!");
    } else {
        error!("❌ Test FAILED - Data mismatch");
        info!("Original: {} bytes", test_data.len());
        info!("Decoded:  {} bytes", decoded_data.len());

        // Find first difference
        for i in 0..test_data
            .len()
            .min(decoded_data.len())
        {
            if test_data[i] != decoded_data[i] {
                info!(
                    "First difference at byte {}: expected {:#04x}, got {:#04x}",
                    i,
                    test_data[i],
                    decoded_data
                        .get(i)
                        .unwrap_or(&0)
                );
                break;
            }
        }
    }

    // Performance stats
    let total_bits = test_data.len() * 8;
    let duration_s = samples.len() as f32 / SAMPLE_RATE as f32;
    let effective_bitrate = total_bits as f32 / duration_s;

    info!("Performance:");
    info!("  - Total bits: {}", total_bits);
    info!("  - Duration: {:.3} seconds", duration_s);
    info!("  - Effective bit rate: {:.0} bps", effective_bitrate);
    info!(
        "  - Overhead: {:.1}%",
        (1.0 - effective_bitrate / BIT_RATE as f32) * 100.0
    );
}
