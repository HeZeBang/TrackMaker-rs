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
use rand::{self, Rng, SeedableRng};
use tracing::info;
use ui::print_banner;
use ui::progress::{ProgressManager, templates};
use utils::consts::*;
use utils::logging::init_logging;

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
    
    // random data for 100 frame with 100 bit/frame
    let seed = 1u64; // seed 1 is a magic number
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
        sample_rate_f32,
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

    // Demodulate each detected frame
    for (frame_idx, &frame_start) in frame_starts.iter().enumerate() {
        let frame_end = frame_start + frame_length_samples;
        
        if frame_end <= rx_signal.len() {
            let frame_signal = &rx_signal[frame_start..frame_end];
            
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
}