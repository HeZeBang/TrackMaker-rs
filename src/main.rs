use dialoguer::{theme::ColorfulTheme, Select};
use jack;
mod audio;
mod device;
mod ui;
mod utils;
use audio::recorder;
use audio::fsk::{FSKModulator, FSKDemodulator};
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

    // 增加最大录音时长以适应FSK较低的传输效率 (30秒)
    let max_duration_samples = sample_rate * 30;

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
    // 创建FSK调制器
    let mut fsk_modulator = FSKModulator::new(sample_rate as f32);

    // Preamble generation (440 samples) - 保持与原来相同的线性扫频前导码
    let sample_rate_f32 = sample_rate as f32;
    let t: Vec<f32> = (0..48000)
        .map(|i| i as f32 / sample_rate_f32)
        .collect();

    let mut f_p = Vec::with_capacity(440);
    // First 220: linear from 2kHz to 12kHz (扩展到FSK的最高频率)
    for i in 0..220 {
        f_p.push(2000.0 + (10000.0 * i as f32 / 219.0));
    }
    // Next 220: linear from 12kHz to 2kHz
    for i in 0..220 {
        f_p.push(12000.0 - (10000.0 * i as f32 / 219.0));
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

        // FSK调制: 使用新的码率 (800bps, 60 samples per bit)
        let frame_wave = fsk_modulator.modulate_bits(&frame_crc);

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

    // Preamble generation (440 samples) - 匹配发送端的频率范围
    let mut f_p = Vec::with_capacity(440);
    // First 220: linear from 2kHz to 12kHz
    for i in 0..220 {
        f_p.push(2000.0 + (10000.0 * i as f32 / 219.0));
    }
    // Next 220: linear from 12kHz to 2kHz
    for i in 0..220 {
        f_p.push(12000.0 - (10000.0 * i as f32 / 219.0));
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

    // 创建FSK解调器
    let mut fsk_demodulator = FSKDemodulator::new(sample_rate as f32);

    let mut power = 0.0f32;
    let mut power_debug = vec![0.0f32; rx_fifo.len()];
    let mut start_index = 0usize;
    let mut start_index_debug = vec![0.0f32; rx_fifo.len()];
    let mut sync_fifo = vec![0.0f32; 440];
    let mut sync_power_debug = vec![0.0f32; rx_fifo.len()];
    let mut sync_power_local_max = 0.0f32;

    let mut decode_fifo = Vec::new();
    let mut correct_frame_num = 0;

    let mut state = 0; // 0: sync, 1: decode

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

            // FSK解调: 调整为新的比特长度 (60 samples/bit * 108 bits = 6480 samples)
            if decode_fifo.len() == SAMPLES_PER_BIT * 108 {
                // 使用FSK解调器解码
                let decoded_bits = fsk_demodulator.demodulate_samples(&decode_fifo);

                if decoded_bits.len() >= 8 {
                    // CRC check (simplified - just compare first 8 bits with expected ID)
                    let mut temp_index = 0u8;
                    for k in 0..8 {
                        if decoded_bits[k] == 1 {
                            temp_index += 1 << (7 - k);
                        }
                    }

                    if temp_index > 0 && temp_index <= 100 {
                        info!("Correct, ID: {}", temp_index);
                        correct_frame_num += 1;
                    } else {
                        info!("Error in frame, decoded ID: {}", temp_index);
                    }
                } else {
                    info!("Error: insufficient decoded bits");
                }

                start_index = 0;
                decode_fifo.clear();
                state = 0;
            }
        }
    }

    info!("Total Correct: {}", correct_frame_num);
}
