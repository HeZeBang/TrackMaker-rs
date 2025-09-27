use dialoguer::{Input, Select, theme::ColorfulTheme};
use jack;
mod acoustic;
mod audio;
mod device;
mod transmission;
mod ui;
mod utils;
use audio::recorder;
use device::jack::print_jack_info;
use std::path::{Path, PathBuf};
use tracing::info;
use transmission::{PskReceiver, PskSender};
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

    let selections = &[
        "Sender (play via JACK)",
        "Receiver (record via JACK)",
        "Export transmission to WAV",
        "Decode transmission from WAV",
    ];
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select mode")
        .default(0)
        .items(&selections[..])
        .interact()
        .unwrap();

    match selection {
        0 => {
            run_sender(shared, progress_manager, sample_rate as u32);
        }
        1 => {
            run_receiver(
                shared,
                progress_manager,
                sample_rate as u32,
                max_duration_samples as u32,
            );
        }
        2 => export_transmission_to_wav(sample_rate as u32),
        3 => decode_transmission_from_wav(),
        _ => unreachable!(),
    }

    tracing::info!("Exiting gracefully...");
    if let Err(err) = active_client.deactivate() {
        tracing::error!("Error deactivating client: {}", err);
    }
}

fn run_sender(
    shared: recorder::AppShared,
    progress_manager: ProgressManager,
    _sample_rate: u32,
) {
    // Create PSK sender with default configuration
    let sender = PskSender::new_default();

    // Transmit text from file
    let text_file_path = "assets/think-different.txt";
    let output_track = sender.transmit_text_file(text_file_path);

    let output_track_len = output_track.len();

    // Copy to shared playback buffer
    {
        let mut playback = shared
            .playback_buffer
            .lock()
            .unwrap();
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
}

fn run_receiver(
    shared: recorder::AppShared,
    progress_manager: ProgressManager,
    _sample_rate: u32,
    max_recording_duration_samples: u32,
) {
    // Create PSK receiver with default configuration
    let receiver = PskReceiver::new_default();

    progress_manager
        .create_bar(
            "recording",
            max_recording_duration_samples as u64,
            templates::RECORDING,
            "receiver",
        )
        .unwrap();

    // Start recording
    *shared
        .app_state
        .lock()
        .unwrap() = recorder::AppState::Recording;

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

    // Get received signal
    let rx_signal: Vec<f32> = {
        let record = shared
            .record_buffer
            .lock()
            .unwrap();
        record
            .iter()
            .copied()
            .collect()
    };

    // Process the received signal and save results
    let output_dir = Path::new("tmp");
    receiver.receive_text_with_comparison(
        &rx_signal,
        "assets/think-different.txt",
        output_dir,
    );
}

fn export_transmission_to_wav(sample_rate: u32) {
    let sender = PskSender::new_default();
    let text_file_path = "assets/think-different.txt";
    let waveform = sender.transmit_text_file(text_file_path);

    if waveform.is_empty() {
        info!("No data generated for export");
        return;
    }

    let default_path = PathBuf::from("tmp/transmission.wav");
    let default_path_str = default_path
        .display()
        .to_string();

    let output_path: String = Input::new()
        .with_prompt("Output WAV file path")
        .default(default_path_str)
        .interact_text()
        .unwrap();

    let output_path = PathBuf::from(output_path);
    match acoustic::io::write_to_wav(&waveform, sample_rate, &output_path) {
        Ok(_) => info!("Exported transmission to {}", output_path.display()),
        Err(err) => tracing::error!(
            "Failed to export transmission to {}: {err}",
            output_path.display()
        ),
    }
}

fn decode_transmission_from_wav() {
    let default_path = PathBuf::from("tmp/transmission.wav");
    let default_path_str = default_path
        .display()
        .to_string();

    let input_path: String = Input::new()
        .with_prompt("Input WAV file path")
        .default(default_path_str)
        .interact_text()
        .unwrap();

    let input_path = PathBuf::from(input_path);
    let signal = match acoustic::io::read_wav(&input_path) {
        Ok(samples) => samples,
        Err(err) => {
            tracing::error!(
                "Failed to read input WAV {}: {err}",
                input_path.display()
            );
            return;
        }
    };

    if signal.is_empty() {
        tracing::warn!("Input WAV contained no samples");
        return;
    }

    let receiver = PskReceiver::new_default();
    let output_dir = Path::new("tmp");
    match receiver.receive_text_with_comparison(
        &signal,
        "assets/think-different.txt",
        output_dir,
    ) {
        Some(text) => info!(
            "Decoded transmission from {} ({} bytes)",
            input_path.display(),
            text.len()
        ),
        None => tracing::error!(
            "Failed to decode transmission from {}",
            input_path.display()
        ),
    }
}
