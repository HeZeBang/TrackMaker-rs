use audio::recorder;
use device::jack::{
    connect_input_from_first_system_output,
    connect_output_to_first_system_input, disconnect_input_sources,
    disconnect_output_sinks, print_jack_info,
};
use jack;
use tracing::info;
use trackmaker_rs::*;
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

    let recording_duration_samples = sample_rate * DEFAULT_RECORD_SECONDS;
    tracing::info!(
        "Recording duration: {} samples ({} seconds)",
        recording_duration_samples,
        DEFAULT_RECORD_SECONDS
    );

    // Shared State
    let shared = recorder::AppShared::new(recording_duration_samples);
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
        recording_duration_samples,
    );
    let process = jack::contrib::ClosureProcessHandler::new(process_cb);

    let active_client = client
        .activate_async((), process)
        .unwrap();

    // Recording
    connect_input_from_first_system_output(
        active_client.as_client(),
        &in_port_name,
    );

    let progress_manager = ProgressManager::new();
    progress_manager
        .create_bar(
            "recording",
            recording_duration_samples as u64,
            templates::RECORDING,
            in_port_name.as_str(),
        )
        .unwrap();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));

        ui::update_progress(
            &shared,
            recording_duration_samples,
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

    // Playback
    disconnect_input_sources(active_client.as_client(), &in_port_name);
    connect_output_to_first_system_input(
        active_client.as_client(),
        &out_port_name,
    );

    // Copy to playback buffer
    {
        let mut recorded = shared
            .record_buffer
            .lock()
            .unwrap();
        let mut playback = shared
            .playback_buffer
            .lock()
            .unwrap();

        playback.extend(recorded.drain(..));
    }

    progress_manager
        .create_bar(
            "playback",
            recording_duration_samples as u64,
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

        ui::update_progress(
            &shared,
            recording_duration_samples,
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

    // Play music and record in the same time
    disconnect_output_sinks(active_client.as_client(), &out_port_name);
    connect_system_ports(
        active_client.as_client(),
        &in_port_name,
        &out_port_name,
    );

    // Copy to playback buffer
    {
        let mut playback = shared
            .playback_buffer
            .lock()
            .unwrap();

        info!("Filling playback buffer with music from sample.flac");

        let mut music = Vec::new();
        audio::decoder::decode_flac_to_f32("./assets/sample.flac")
            .unwrap_or_else(|_| {
                tracing::warn!("Failed to decode sample.flac, using silence");
                vec![0.0; recording_duration_samples as usize]
            })
            .into_iter()
            .take(recording_duration_samples as usize)
            .for_each(|s| music.push(s));

        info!("Music length: {} samples", music.len());

        playback.extend(music.drain(..));
    }

    progress_manager
        .create_bar(
            "playrec",
            recording_duration_samples as u64,
            templates::PLAYREC,
            out_port_name.as_str(),
        )
        .unwrap();

    *shared
        .app_state
        .lock()
        .unwrap() = recorder::AppState::RecordingAndPlaying;

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));

        ui::update_progress(
            &shared,
            recording_duration_samples,
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

    // Playback
    disconnect_input_sources(active_client.as_client(), &in_port_name);

    // Copy to playback buffer
    {
        let mut recorded = shared
            .record_buffer
            .lock()
            .unwrap();
        let mut playback = shared
            .playback_buffer
            .lock()
            .unwrap();

        playback.extend(recorded.drain(..));
    }

    progress_manager
        .create_bar(
            "playback",
            recording_duration_samples as u64,
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

        ui::update_progress(
            &shared,
            recording_duration_samples,
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

    tracing::info!("Exiting gracefully...");
    if let Err(err) = active_client.deactivate() {
        tracing::error!("Error deactivating client: {}", err);
    }
}
