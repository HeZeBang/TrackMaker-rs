use dialoguer::{Select, theme::ColorfulTheme};
use jack;
use std::fs;
use tracing::info;

mod audio;
mod device;
mod phy;
mod ui;
mod utils;

use audio::recorder;
use device::jack::{connect_system_ports, print_jack_info};
use ui::print_banner;
use ui::progress::{ProgressManager, templates};
use utils::consts::*;
use utils::logging::init_logging;

use phy::{Frame, LineCodingKind, PhyDecoder, PhyEncoder};

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

    if sample_rate as u32 != SAMPLE_RATE {
        tracing::warn!(
            "Sample rate mismatch! Expected {}, got {}",
            SAMPLE_RATE,
            sample_rate
        );
        tracing::warn!("Physical layer is designed for {} Hz", SAMPLE_RATE);
    }

    let max_duration_samples = sample_rate * 10; // 30 seconds max

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
        LineCodingKind::Manchester,
        LineCodingKind::FourBFiveB,
    ];
    let line_coding_labels = ["Manchester (Bi-phase)", "4B5B (NRZ)"];
    let line_coding_idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select line coding scheme")
        .default(0)
        .items(&line_coding_labels)
        .interact()
        .unwrap();
    let line_coding = line_coding_options[line_coding_idx];
    info!("Selected line coding: {}", line_coding.name());

    {
        shared
            .record_buffer
            .lock()
            .unwrap()
            .clear();
    }

    if selection == 0 {
        // Sender
        run_sender(shared, progress_manager, sample_rate as u32, line_coding);
    } else if selection == 1 {
        // Receiver
        run_receiver(
            shared,
            progress_manager,
            max_duration_samples as u32,
            line_coding,
        );
    } else {
        // Test mode (no JACK)
        test_transmission(line_coding);
        return;
    }

    info!("Exiting gracefully...");
    if let Err(err) = active_client.deactivate() {
        tracing::error!("Error deactivating client: {}", err);
    }
}

fn run_sender(
    shared: recorder::AppShared,
    progress_manager: ProgressManager,
    sample_rate: u32,
    line_coding: LineCodingKind,
) {
    info!("=== Sender Mode ===");
    info!("Using line coding: {}", line_coding.name());

    // Read input file
    let input_path = "INPUT.bin";
    let file_data = match fs::read(input_path) {
        Ok(data) => {
            info!("Read {} bytes from {}", data.len(), input_path);
            data
        }
        Err(e) => {
            tracing::error!("Failed to read {}: {}", input_path, e);
            return;
        }
    };

    // Create PHY encoder
    let encoder = PhyEncoder::new(
        SAMPLES_PER_LEVEL,
        PREAMBLE_PATTERN_BYTES,
        line_coding,
    );

    // Split data into frames
    let mut frames = Vec::new();
    let mut seq = 0u8;

    info!(
        "Splitting data into frames (max {} bytes each)",
        MAX_FRAME_DATA_SIZE
    );
    for chunk in file_data.chunks(MAX_FRAME_DATA_SIZE) {
        let frame = Frame::new_data(seq, chunk.to_vec());
        frames.push(frame);
        seq = seq.wrapping_add(1);
    }

    info!(
        "Created {} frames from {} bytes",
        frames.len(),
        file_data.len()
    );

    // Encode frames to audio samples
    let output_track = encoder.encode_frames(&frames, INTER_FRAME_GAP_SAMPLES);
    let output_track_len = output_track.len();

    info!(
        "Encoded to {} samples ({:.2} seconds)",
        output_track_len,
        output_track_len as f32 / sample_rate as f32
    );

    // Save encoded signal to WAV
    if let Err(e) = utils::dump::dump_to_wav(
        "./tmp/sender_output.wav",
        &utils::dump::AudioData {
            sample_rate,
            audio_data: output_track.clone(),
            duration: output_track_len as f32 / sample_rate as f32,
            channels: 1,
        },
    ) {
        tracing::warn!("Failed to save sender WAV: {}", e);
    } else {
        info!("Saved sender signal to ./tmp/sender_output.wav");
    }

    // Calculate theoretical transmission time
    let total_bits = file_data.len() * 8;
    let theoretical_time = total_bits as f32 / BIT_RATE as f32;
    info!(
        "Theoretical transmission time: {:.2} seconds (at {} bps)",
        theoretical_time, BIT_RATE
    );

    {
        let mut playback = shared
            .playback_buffer
            .lock()
            .unwrap();
        playback.extend(output_track);
    }

    progress_manager
        .create_bar(
            "playback",
            output_track_len as u64,
            templates::PLAYBACK,
            "sender",
        )
        .unwrap();

    *shared
        .app_state
        .lock()
        .unwrap() = recorder::AppState::Playing;

    let start_time = std::time::Instant::now();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));

        ui::update_progress(&shared, output_track_len, &progress_manager);

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

    let elapsed = start_time
        .elapsed()
        .as_secs_f32();
    info!("Transmission completed in {:.2} seconds", elapsed);
}

fn run_receiver(
    shared: recorder::AppShared,
    progress_manager: ProgressManager,
    max_recording_duration_samples: u32,
    line_coding: LineCodingKind,
) {
    info!("=== Receiver Mode ===");
    info!("Using line coding: {}", line_coding.name());

    // Create decoder
    let mut decoder = PhyDecoder::new(
        SAMPLES_PER_LEVEL,
        PREAMBLE_PATTERN_BYTES,
        line_coding,
    );
    let mut all_frames: Vec<Frame> = Vec::new();
    let mut processed_samples_len = 0;

    progress_manager
        .create_bar(
            "recording",
            max_recording_duration_samples as u64,
            templates::RECORDING,
            "receiver",
        )
        .unwrap();

    *shared.app_state.lock().unwrap() = recorder::AppState::Recording;

    let start_time = std::time::Instant::now();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));

        let current_samples = {
            let buffer = shared.record_buffer.lock().unwrap();
            buffer.clone()
        };

        if current_samples.len() > processed_samples_len {
            let new_samples = &current_samples[processed_samples_len..];
            let decoded_frames = decoder.process_samples(new_samples);
            if !decoded_frames.is_empty() {
                info!("Decoded {} new frames", decoded_frames.len());
                all_frames.extend(decoded_frames);
            }
            processed_samples_len = current_samples.len();
        }

        ui::update_progress(
            &shared,
            max_recording_duration_samples as usize,
            &progress_manager,
        );

        let state = { shared.app_state.lock().unwrap().clone() };
        if let recorder::AppState::Idle = state {
            progress_manager.finish_all();
            break;
        }
    }

    let elapsed = start_time.elapsed().as_secs_f32();
    info!("Recording completed in {:.2} seconds", elapsed);

    // Final processing for any remaining samples
    let final_samples = {
        let buffer = shared.record_buffer.lock().unwrap();
        buffer.clone()
    };
    if final_samples.len() > processed_samples_len {
        let remaining_samples = &final_samples[processed_samples_len..];
        let decoded_frames = decoder.process_samples(remaining_samples);
        if !decoded_frames.is_empty() {
            info!("Decoded {} final frames", decoded_frames.len());
            all_frames.extend(decoded_frames);
        }
    }

    info!("Total decoded frames: {}", all_frames.len());

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
        tracing::warn!("Failed to save receiver WAV: {}", e);
    } else {
        info!("Saved received signal to ./tmp/receiver_input.wav");
    }

    // Reconstruct file data
    let mut output_data = Vec::new();
    let mut expected_seq = 0u8;
    let mut frame_errors = 0;

    // Sort frames by sequence number to handle out-of-order decoding
    all_frames.sort_by_key(|f| f.sequence);
    all_frames.dedup_by_key(|f| f.sequence);

    for frame in all_frames {
        if frame.sequence != expected_seq {
            tracing::warn!(
                "Frame sequence mismatch: expected {}, got {}",
                expected_seq,
                frame.sequence
            );
            frame_errors += 1;
        }
        output_data.extend_from_slice(&frame.data);
        expected_seq = frame.sequence.wrapping_add(1);
    }

    info!("Reconstructed {} bytes", output_data.len());

    if frame_errors > 0 {
        tracing::warn!("⚠️  {} frame sequence errors detected", frame_errors);
    }

    // Write to output file
    let output_path = "OUTPUT.bin";
    match fs::write(output_path, &output_data) {
        Ok(_) => info!("✅ Written to {}", output_path),
        Err(e) => tracing::error!("Failed to write {}: {}", output_path, e),
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
    );

    // Create frames
    let mut frames = Vec::new();
    let mut seq = 0u8;

    for chunk in test_data.chunks(MAX_FRAME_DATA_SIZE) {
        let frame = Frame::new_data(seq, chunk.to_vec());
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
        tracing::warn!("Failed to save WAV: {}", e);
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
        tracing::error!("❌ Test FAILED - Data mismatch");
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
