use jack;
mod audio;
mod device;
mod ui;
mod utils;
use audio::AppShared;
use device::jack::{connect_system_ports, print_jack_info};
use ui::print_banner;
use utils::consts::*;
use utils::logging::init_logging;
use utils::progress::{ProgressManager, templates};

fn main() {
    init_logging();
    print_banner();
    let (client, status) =
        jack::Client::new(JACK_CLIENT_NAME, jack::ClientOptions::NO_START_SERVER).unwrap();
    tracing::info!("JACK client status: {:?}", status);
    let (sample_rate, _buffer_size) = print_jack_info(&client);

    let recording_duration_samples = sample_rate * DEFAULT_RECORD_SECONDS;
    tracing::info!(
        "Recording duration: {} samples ({} seconds)",
        recording_duration_samples,
        DEFAULT_RECORD_SECONDS
    );

    // 共享状态
    let shared = AppShared::new(recording_duration_samples);
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
    let process_cb =
        audio::build_process_closure(in_port, out_port, shared_cb, recording_duration_samples);
    let process = jack::contrib::ClosureProcessHandler::new(process_cb);

    let active_client = client.activate_async((), process).unwrap();

    connect_system_ports(active_client.as_client(), &in_port_name, &out_port_name);

    let progress_manager = ProgressManager::new();
    progress_manager
        .create_bar(
            "recording",
            recording_duration_samples as u64,
            templates::RECORDING,
            format!("Record for {} secs", DEFAULT_RECORD_SECONDS).as_str(),
        )
        .unwrap();

    ui::run_progress_loop(&shared, recording_duration_samples, &progress_manager);

    tracing::info!("Exiting gracefully...");
    if let Err(err) = active_client.deactivate() { // FIXME: callback problem//
        tracing::error!("Error deactivating client: {}", err);
    }
}
