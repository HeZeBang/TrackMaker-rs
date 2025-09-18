use jack;
mod audio;
mod device;
mod ui;
mod utils;
use audio::recorder;
use device::jack::{
    connect_input_from_first_system_output,
    connect_output_to_first_system_input, disconnect_input_sources,
    print_jack_info,
};
use ui::print_banner;
use utils::consts::*;
use utils::logging::init_logging;
use utils::progress::{ProgressManager, templates};

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

    // 共享状态
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

    // 音频处理回调
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
        
        ui::update_progress(&shared, recording_duration_samples, &progress_manager);
        
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

    // 切换到播放阶段：断开输入，连接输出
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

    progress_manager.create_bar(
        "playback",
        recording_duration_samples as u64,
        templates::PLAYBACK,
        out_port_name.as_str(),
    ).unwrap();

    *shared.app_state.lock().unwrap() = recorder::AppState::Playing;

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        
        ui::update_progress(&shared, recording_duration_samples, &progress_manager);
        
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
