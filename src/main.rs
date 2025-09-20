use jack;
mod audio;
mod device;
mod ui;
mod utils;
use audio::recorder;
use device::jack::{
    connect_input_from_first_system_output,
    connect_output_to_first_system_input, disconnect_input_sources,
    disconnect_output_sinks, print_jack_info,
};
use rand::{self, Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use tracing::info;
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

    let max_duration_samples = sample_rate * DEFAULT_RECORD_SECONDS * 3;

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
    let crc8_poly = 0x1D7u16;

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
        let inter_frame_space1: usize = rng.gen_range(0..100);
        let inter_frame_space2: usize = rng.gen_range(0..100);

        output_track.extend(vec![0.0; inter_frame_space1]);
        output_track.extend(frame_wave_pre);
        output_track.extend(vec![0.0; inter_frame_space2]);
    }

    let output_track_len = output_track.len();

    {
        let mut playback = shared
            .playback_buffer
            .lock()
            .unwrap();

        playback.extend(output_track);
        info!("Output track length: {} samples", playback.len());
    }


    progress_manager
        .create_bar(
            "playback",
            output_track_len as u64,
            templates::PLAYBACK,
            out_port_name.as_str(),
        )
        .unwrap();

    *shared
        .app_state
        .lock()
        .unwrap() = recorder::AppState::Playing;

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

    tracing::info!("Exiting gracefully...");
    if let Err(err) = active_client.deactivate() {
        tracing::error!("Error deactivating client: {}", err);
    }
}
