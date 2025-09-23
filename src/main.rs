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
    let payload_bits: Vec<u8> = file_content
        .bytes()
        .flat_map(|byte| (0..8).map(move |i| ((byte >> (7 - i)) & 1) as u8))
        .collect();

    // Build OFDM stream
    let params = audio::ofdm::OfdmParams::new(sample_rate as usize);
    let ofdm_stream = audio::ofdm::build_ofdm_frame_stream(&params, &payload_bits, 1u8);

    let output_track_len = ofdm_stream.len();

    {
        let mut playback = shared.playback_buffer.lock().unwrap();
        playback.extend(ofdm_stream);
        info!("Output track length: {} samples", playback.len());
    }

    progress_manager
        .create_bar("playback", output_track_len as u64, templates::PLAYBACK, "sender")
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
    progress_manager
        .create_bar(
            "recording",
            max_recording_duration_samples as u64,
            templates::RECORDING,
            "receiver",
        )
        .unwrap();

    *shared.app_state.lock().unwrap() = recorder::AppState::Recording;

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));

        ui::update_progress(
            &shared,
            max_recording_duration_samples as usize,
            &progress_manager,
        );

        let state = {
            shared.app_state.lock().unwrap().clone()
        };
        if let recorder::AppState::Idle = state {
            progress_manager.finish_all();
            break;
        }
    }

    let rx_fifo: Vec<f32> = {
        let record = shared.record_buffer.lock().unwrap();
        record.iter().copied().collect()
    };

    // Decode using OFDM decoder
    let params = audio::ofdm::OfdmParams::new(sample_rate as usize);
    let decoded_bits = audio::ofdm::decode_ofdm_stream(&params, &rx_fifo);

    // Convert bits to text and print progressively
    let mut decoded_content = decoded_bits;
    let mut output_text = String::new();
    let mut i = 0;
    while i + 8 <= decoded_content.len() {
        let mut byte = 0u8;
        for j in 0..8 {
            if decoded_content[i + j] != 0 {
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

    println!("\n接收完成！解码位数: {}", decoded_content.len());
}

fn test_sender_receiver() {
    println!("开始测试发送和接收功能...");

    // Read content from think-different.txt file
    let file_content = std::fs::read_to_string("assets/think-different.txt")
        .expect("Failed to read think-different.txt");

    println!("原始文件内容:\n{}", file_content);
    println!("原始文件长度: {} bytes", file_content.len());

    // Convert text content to bits (ASCII encoding)
    let payload_bits: Vec<u8> = file_content
        .bytes()
        .flat_map(|byte| (0..8).map(move |i| ((byte >> (7 - i)) & 1) as u8))
        .collect();

    let sample_rate = 48000u32;
    let params = audio::ofdm::OfdmParams::new(sample_rate as usize);

    // Run an in-memory encode->decode self-test to dump intermediate arrays for debugging.
    audio::ofdm::encode_decode_self_test(&params, &payload_bits);

    // Build OFDM stream
    let ofdm_stream = audio::ofdm::build_ofdm_frame_stream(&params, &payload_bits, 1u8);
    debug!("Total length: {} samples", ofdm_stream.len());

    // ensure output directory exists, then dump json
    std::fs::create_dir_all("./tmp").expect("Failed to create ./tmp directory");
    // dump json
    utils::dump::dump_to_json("./tmp/output.json", &utils::dump::AudioData {
        sample_rate,
        audio_data: ofdm_stream.clone(),
        duration: ofdm_stream.len() as f32 / sample_rate as f32,
        channels: 1,
    })
        .expect("Failed to dump output track to JSON");
    info!("Dumped output track to ./tmp/output.json");

    utils::dump::dump_to_wav("./tmp/output.wav", &utils::dump::AudioData {
        sample_rate,
        audio_data: ofdm_stream.clone(),
        duration: ofdm_stream.len() as f32 / sample_rate as f32,
        channels: 1,
    })
        .expect("Failed to dump output track to WAV");
    info!("Dumped output track to ./tmp/output.wav");

    // Now decode the output_track
    let rx_fifo: Vec<f32> = ofdm_stream;

    // Decode using OFDM decoder
    let decoded_bits = audio::ofdm::decode_ofdm_stream(&params, &rx_fifo);

    // Convert decoded bits to text
    let mut decoded_text = String::new();
    let mut i = 0usize;
    while i + 8 <= decoded_bits.len() {
        let mut byte = 0u8;
        for j in 0..8 {
            if decoded_bits[i + j] != 0 {
                byte |= 1 << (7 - j);
            }
        }
        decoded_text.push(byte as char);
        i += 8;
    }

    println!("\n解码完成！");
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
