use clap::{Parser, Subcommand};
use jack;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tracing::{debug, error, info, warn};

mod amodem;
mod audio;
mod device;
mod error_correction;
mod ui;
mod utils;

use amodem::{
    common, config::Configuration, detect::Detector, recv::Receiver, send,
};
use device::jack::{
    connect_input_from_first_system_output,
    connect_output_to_first_system_input, disconnect_input_sources,
    disconnect_output_sinks, print_jack_info,
};
use ui::{print_banner, progress::ProgressManager};
use utils::logging::init_logging;

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};

#[derive(Parser)]
#[command(name = "trackmaker-amodem")]
#[command(about = "Audio Modem with JACK support")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Receive mode: decode audio from JACK input or file to output file
    Receive {
        /// Output file for decoded data
        #[arg(short, long, default_value = "tmp/decoded_output.txt")]
        output: PathBuf,

        /// Duration to record in seconds (ignored when reading from file)
        #[arg(short, long, default_value = "100")]
        duration: f32,

        /// Bitrate configuration (1 or 2)
        #[arg(short, long, default_value = "1")]
        bitrate: u32,

        /// Enable Reed-Solomon error correction
        #[arg(long)]
        reed_solomon: bool,

        /// Reed-Solomon error correction code length (default: 16)
        #[arg(long, default_value = "16")]
        ecc_len: usize,

        /// Input WAV file to read audio from (if empty, read from JACK)
        #[arg(short, long)]
        input: Option<PathBuf>,
    },

    /// Send mode: encode file content and play through JACK
    Send {
        /// Input file to encode and transmit
        #[arg(short, long)]
        input: PathBuf,

        /// Max duration to transmit in seconds
        #[arg(short, long, default_value = "60")]
        duration: f32,

        /// Bitrate configuration (1 or 2)
        #[arg(short, long, default_value = "1")]
        bitrate: u32,

        /// Enable Reed-Solomon error correction
        #[arg(long)]
        reed_solomon: bool,

        /// Reed-Solomon error correction code length (default: 16)
        #[arg(long, default_value = "16")]
        ecc_len: usize,
    },

    /// Test mode: send and receive in sequence
    Test {
        /// Test message to send
        #[arg(short, long, default_value = "Hello, amodem!")]
        message: String,

        /// Duration for each phase in seconds
        #[arg(short, long, default_value = "5.0")]
        duration: f32,

        /// Bitrate configuration (1 or 2)
        #[arg(short, long, default_value = "1")]
        bitrate: u32,

        /// Enable Reed-Solomon error correction
        #[arg(long)]
        reed_solomon: bool,

        /// Reed-Solomon error correction code length (default: 16)
        #[arg(long, default_value = "16")]
        ecc_len: usize,
    },
}

fn main() {
    let cli = Cli::parse();
    init_logging();

    match cli.command {
        Commands::Receive {
            output,
            duration,
            bitrate,
            reed_solomon,
            ecc_len,
        } => {
            receive_mode(output, duration, bitrate, reed_solomon, ecc_len);
        }
        Commands::Send {
            input,
            duration,
            bitrate,
            reed_solomon,
            ecc_len,
        } => {
            send_mode(input, duration, bitrate, reed_solomon, ecc_len);
        }
        Commands::Test {
            message,
            duration,
            bitrate,
            reed_solomon,
            ecc_len,
        } => {
            test_mode(message, duration, bitrate, reed_solomon, ecc_len);
        }
    }
}

fn receive_mode(
    output: PathBuf,
    duration: f32,
    bitrate: u32,
    reed_solomon: bool,
    ecc_len: usize,
) {
    print_banner();

    info!("Amodem Receive Mode");
    info!("Output file: {}", output.display());
    info!("Duration: {:.1} seconds", duration);
    info!("Bitrate: {} kb/s", bitrate);
    if reed_solomon {
        info!(
            "Reed-Solomon error correction enabled (ECC length: {})",
            ecc_len
        );
    }

    // Get configuration
    let config = match bitrate {
        1 => Configuration::bitrate_1(),
        2 => Configuration::bitrate_2(),
        _ => {
            error!("Invalid bitrate: {}. Supported values: 1, 2", bitrate);
            return;
        }
    };

    // Setup JACK
    let (client, status) = jack::Client::new(
        "amodem-receive",
        jack::ClientOptions::NO_START_SERVER,
    )
    .unwrap();

    tracing::info!("JACK client status: {:?}", status);
    let (sample_rate, _buffer_size) = print_jack_info(&client);

    let recording_duration_samples = (sample_rate as f32 * duration) as usize;

    // Shared audio buffer for recording
    let audio_buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
    let buffer_clone = audio_buffer.clone();
    let finished = Arc::new(Mutex::new(false));
    let finished_clone = finished.clone();

    // Register JACK ports
    let in_port = client
        .register_port(utils::consts::INPUT_PORT_NAME, jack::AudioIn::default())
        .unwrap();

    let in_port_name = in_port.name().unwrap();

    // Process callback for recording
    let process_cb =
        move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
            let input_buffer = in_port.as_slice(ps);

            for &frame in input_buffer.iter() {
                let mut buffer = buffer_clone.lock().unwrap();
                buffer.push(frame);

                // Check if we have enough samples
                if buffer.len() >= recording_duration_samples {
                    *finished_clone.lock().unwrap() = true;
                    return jack::Control::Quit;
                }
            }

            jack::Control::Continue
        };

    let process = jack::contrib::ClosureProcessHandler::new(process_cb);

    let active_client = client
        .activate_async((), process)
        .unwrap();

    // Connect to system input
    connect_input_from_first_system_output(
        active_client.as_client(),
        &in_port_name,
    );

    info!("Waiting for audio input...");
    info!("Listening for amodem signal on JACK input");
    info!("Recording for {:.1} seconds...", duration);

    // Wait for recording to complete
    loop {
        thread::sleep(Duration::from_millis(100));
        if *finished.lock().unwrap() {
            break;
        }
    }

    info!("Recording complete");

    // Disconnect and deactivate
    disconnect_input_sources(active_client.as_client(), &in_port_name);
    active_client
        .deactivate()
        .unwrap();

    // Get recorded audio
    let recorded_samples = audio_buffer
        .lock()
        .unwrap()
        .clone();
    info!("Recorded {} samples", recorded_samples.len());

    // Convert to amodem format (f64, 8kHz)
    let amodem_samples_f32 =
        resample_audio(&recorded_samples, sample_rate as f32, config.fs as f32);
    let amodem_samples: Vec<f64> = amodem_samples_f32
        .iter()
        .map(|&x| x as f64)
        .collect();
    info!(
        "Resampled to {} samples at {:.1} kHz",
        amodem_samples.len(),
        config.fs / 1000.0
    );

    // Save raw audio for debugging with metadata
    let pcm_data =
        common::dumps_with_metadata(&amodem_samples, config.fs as u32, 1, 16);
    std::fs::write("tmp/recorded.pcm", &pcm_data).unwrap();
    info!(
        "Raw audio saved to tmp/recorded.pcm ({} Hz, 16-bit, mono)",
        config.fs as u32
    );

    // Decode using amodem (Reed-Solomon is now handled at frame level)
    info!("Decoding amodem signal...");
    if reed_solomon {
        info!(
            "Reed-Solomon error correction will be applied at frame level (ECC length: {})",
            ecc_len
        );
    }
    match decode_amodem_signal_with_reed_solomon(
        &amodem_samples,
        &config,
        reed_solomon,
        ecc_len,
    ) {
        Ok(decoded_data) => {
            // Write decoded data to output file
            std::fs::write(&output, &decoded_data).unwrap();
            info!(
                "Successfully decoded {} bytes to {}",
                decoded_data.len(),
                output.display()
            );

            // Display decoded content
            if let Ok(text) = String::from_utf8(decoded_data.clone()) {
                info!("Decoded content:");
                info!("{}", text);
            } else {
                info!("Decoded binary data ({} bytes)", decoded_data.len());
            }
        }
        Err(e) => {
            error!("Failed to decode amodem signal: {}", e);
            std::fs::write(&output, format!("Decode error: {}", e)).unwrap();
        }
    }
}

fn send_mode(
    input: PathBuf,
    duration: f32,
    bitrate: u32,
    reed_solomon: bool,
    ecc_len: usize,
) {
    print_banner();

    info!("Amodem Send Mode");
    info!("Input file: {}", input.display());
    info!("Max duration: {:.1} seconds", duration);
    info!("Bitrate: {} kb/s", bitrate);
    if reed_solomon {
        info!(
            "Reed-Solomon error correction enabled (ECC length: {})",
            ecc_len
        );
    }

    // Check if input file exists
    if !input.exists() {
        error!("Input file does not exist: {}", input.display());
        return;
    }

    // Get configuration
    let config = match bitrate {
        1 => Configuration::bitrate_1(),
        2 => Configuration::bitrate_2(),
        _ => {
            error!("Invalid bitrate: {}. Supported values: 1, 2", bitrate);
            return;
        }
    };

    // Read input file
    let input_data = match std::fs::read(&input) {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to read input file: {}", e);
            return;
        }
    };

    info!("Read {} bytes from input file", input_data.len());

    // Generate amodem audio (Reed-Solomon is now handled at frame level)
    info!("Generating amodem audio...");
    if reed_solomon {
        info!(
            "Reed-Solomon error correction will be applied at frame level (ECC length: {})",
            ecc_len
        );
    }
    let audio_samples = generate_amodem_audio_with_reed_solomon(
        &input_data,
        &config,
        reed_solomon,
        ecc_len,
    );
    info!("Generated {} audio samples", audio_samples.len());

    // Save generated audio for debugging with metadata
    let pcm_data =
        common::dumps_with_metadata(&audio_samples, config.fs as u32, 1, 16);
    let pcm_path = "tmp/generated.pcm";
    std::fs::write(pcm_path, &pcm_data).unwrap();
    info!(
        "Generated audio saved to {} ({} Hz, 16-bit, mono)",
        pcm_path, config.fs as u32
    );

    // Use PCM player to play the generated audio
    info!("Starting PCM playback...");
    play_pcm_file(pcm_path, duration);
}

fn test_mode(
    message: String,
    duration: f32,
    bitrate: u32,
    reed_solomon: bool,
    ecc_len: usize,
) {
    print_banner();

    info!("Amodem Test Mode (Direct Memory)");
    info!("Test message: \"{}\"", message);
    info!("Duration: {:.1} seconds per phase", duration);
    info!("Bitrate: {} kb/s", bitrate);
    if reed_solomon {
        info!(
            "Reed-Solomon error correction enabled (ECC length: {})",
            ecc_len
        );
    }

    // Get configuration
    let config = match bitrate {
        1 => Configuration::bitrate_1(),
        2 => Configuration::bitrate_2(),
        _ => {
            error!("Invalid bitrate: {}. Supported values: 1, 2", bitrate);
            return;
        }
    };

    // Phase 1: Generate amodem audio in memory
    info!("Phase 1: Generating amodem audio...");
    let message_bytes = message.as_bytes();

    // Generate amodem audio (Reed-Solomon is now handled at frame level)
    if reed_solomon {
        info!(
            "Reed-Solomon error correction will be applied at frame level (ECC length: {})",
            ecc_len
        );
    }

    let audio_samples = generate_amodem_audio_with_reed_solomon(
        message_bytes,
        &config,
        reed_solomon,
        ecc_len,
    );
    info!("Generated {} audio samples", audio_samples.len());

    // Save generated audio for debugging with metadata
    let pcm_data =
        common::dumps_with_metadata(&audio_samples, config.fs as u32, 1, 16);
    std::fs::write("tmp/test_generated.pcm", &pcm_data).unwrap();
    info!(
        "Generated audio saved to tmp/test_generated.pcm ({} Hz, 16-bit, mono)",
        config.fs as u32
    );

    // Phase 2: Decode the audio directly from memory
    info!("Phase 2: Decoding amodem signal...");
    match decode_amodem_signal_with_reed_solomon(
        &audio_samples,
        &config,
        reed_solomon,
        ecc_len,
    ) {
        Ok(decoded_data) => {
            // Compare results
            info!("Test Results:");
            info!("Sent: \"{}\"", message);

            // Display decoded content
            if let Ok(decoded_text) = String::from_utf8(decoded_data.clone()) {
                info!("Received: \"{}\"", decoded_text.trim());

                if message == decoded_text.trim() {
                    info!("Test PASSED! Message received correctly.");
                } else {
                    error!("Test FAILED! Message mismatch.");
                    error!("   Expected length: {} bytes", message.len());
                    error!("   Received length: {} bytes", decoded_text.len());
                }
            } else {
                info!("Received: {} bytes of binary data", decoded_data.len());
                info!(
                    "Raw bytes: {:02x?}",
                    &decoded_data[..decoded_data.len().min(50)]
                );

                // Check if it's the same binary data
                if message_bytes == &decoded_data[..] {
                    info!("Test PASSED! Binary data matches.");
                } else {
                    error!("Test FAILED! Binary data mismatch.");
                    error!("   Expected length: {} bytes", message_bytes.len());
                    error!("   Received length: {} bytes", decoded_data.len());
                }
            }

            // Save decoded result for inspection
            std::fs::write("tmp/test_result.txt", &decoded_data).unwrap();
            info!("Decoded result saved to tmp/test_result.txt");
        }
        Err(e) => {
            error!("Failed to decode amodem signal: {}", e);
            std::fs::write(
                "tmp/test_result.txt",
                format!("Decode error: {}", e),
            )
            .unwrap();
        }
    }
}

fn decode_amodem_signal_with_reed_solomon(
    samples: &[f64],
    config: &Configuration,
    use_reed_solomon: bool,
    ecc_len: usize,
) -> Result<Vec<u8>, String> {
    // Create detector and receiver
    let detector = Detector::new(config);
    let mut receiver =
        Receiver::with_reed_solomon(config, use_reed_solomon, ecc_len);

    // Detect carrier and get signal
    let (signal, amplitude, freq_error) =
        detector.run(samples.iter().cloned())?;
    let freq = 1.0 / (1.0 + freq_error);

    // Decode the signal directly using the receiver (it handles framing internally)
    let mut output = Vec::new();
    receiver.run(signal, 1.0 / amplitude, freq, &mut output)?;

    Ok(output)
}

fn generate_amodem_audio_with_reed_solomon(
    data: &[u8],
    config: &Configuration,
    use_reed_solomon: bool,
    ecc_len: usize,
) -> Vec<f64> {
    let mut output = Vec::new();

    send::send_with_reed_solomon(
        config,
        &data[..],
        &mut output,
        1.0,
        0.0,
        use_reed_solomon,
        ecc_len,
    )
    .unwrap();

    // Load the generated PCM data
    common::loads(&output)
}

fn resample_audio(input: &[f32], input_rate: f32, output_rate: f32) -> Vec<f32> {
    if input_rate == output_rate {
        return input.to_vec();
    }

    let ratio = output_rate / input_rate;
    let output_len = (input.len() as f32 * ratio) as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_index = i as f32 / ratio;
        let src_index_int = src_index.floor() as usize;
        let src_index_frac = src_index - src_index_int as f32;

        if src_index_int + 1 < input.len() {
            // Linear interpolation
            let sample1 = input[src_index_int];
            let sample2 = input[src_index_int + 1];
            let interpolated = sample1 + src_index_frac * (sample2 - sample1);
            output.push(interpolated);
        } else if src_index_int < input.len() {
            output.push(input[src_index_int]);
        } else {
            output.push(0.0);
        }
    }

    output
}

fn play_pcm_file(pcm_path: &str, duration: f32) {
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    // 读取 PCM 文件
    let (file_sample_rate, file_channels, file_bit_depth, samples) =
        match read_pcm_with_metadata(pcm_path) {
            Ok(result) => result,
            Err(e) => {
                error!("Failed to read PCM file: {}", e);
                return;
            }
        };

    if samples.is_empty() {
        error!("PCM file is empty");
        return;
    }

    info!(
        "Read PCM file: {} Hz, {} channels, {} bits, {} samples",
        file_sample_rate,
        file_channels,
        file_bit_depth,
        samples.len()
    );

    // 设置 JACK 客户端
    let (client, _status) = match jack::Client::new(
        "amodem-send-pcm",
        jack::ClientOptions::NO_START_SERVER,
    ) {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to create JACK client: {}", e);
            return;
        }
    };

    let jack_sample_rate = client.sample_rate();
    info!("JACK sample rate: {} Hz", jack_sample_rate);

    // 重采样到 JACK 采样率
    let resampled_samples = match high_quality_resample(
        &samples,
        file_sample_rate,
        jack_sample_rate as u32,
    ) {
        Ok(samples) => samples,
        Err(e) => {
            error!("Resampling failed: {}", e);
            return;
        }
    };

    // 创建进度管理器
    let progress_manager = ProgressManager::new();
    let total_samples = resampled_samples.len() as u64;

    // 创建播放进度条
    progress_manager.create_bar(
        "playback",
        total_samples,
        "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} samples ({percent}%) {msg}",
        "Playing..."
    ).unwrap();

    // 创建播放状态
    let state = PlaybackState {
        samples: Arc::new(Mutex::new(resampled_samples)),
        position: Arc::new(Mutex::new(0)),
        is_playing: Arc::new(Mutex::new(true)),
        should_loop: false,
        progress_manager: progress_manager.clone(),
    };

    // 注册输出端口
    let out_port = match client.register_port(
        utils::consts::OUTPUT_PORT_NAME,
        jack::AudioOut::default(),
    ) {
        Ok(port) => port,
        Err(e) => {
            error!("Failed to register port: {}", e);
            return;
        }
    };

    let out_port_name = match out_port.name() {
        Ok(name) => name,
        Err(e) => {
            error!("Failed to get port name: {}", e);
            return;
        }
    };

    // 创建处理回调
    let process_callback =
        create_pcm_process_callback(out_port, state.clone(), 1.0);
    let process = jack::contrib::ClosureProcessHandler::new(process_callback);

    // 激活客户端
    let active_client = match client.activate_async((), process) {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to activate client: {}", e);
            return;
        }
    };

    // 连接到系统输出端口
    let system_input_ports = active_client
        .as_client()
        .ports(
            None,
            None,
            jack::PortFlags::IS_INPUT | jack::PortFlags::IS_PHYSICAL,
        );

    if let Some(system_in) = system_input_ports.first() {
        match active_client
            .as_client()
            .connect_ports_by_name(&out_port_name, system_in)
        {
            Ok(_) => {
                info!("Connected output: {} -> {}", out_port_name, system_in)
            }
            Err(e) => warn!("Failed to connect output: {}", e),
        }
    } else {
        warn!("No system input ports found");
    }

    info!("Starting PCM file playback...");

    // 计算实际播放时长（基于音频长度）
    let actual_duration = total_samples as f32 / jack_sample_rate as f32;
    let play_duration = if duration > 0.0 && duration < actual_duration {
        duration // 如果指定了更短的时长，使用指定时长
    } else {
        actual_duration // 否则播放完整音频
    };

    info!(
        "Audio duration: {:.2} seconds, playback duration: {:.2} seconds",
        actual_duration, play_duration
    );

    // 启动进度更新线程
    let progress_manager_clone = progress_manager.clone();
    let state_clone = state.clone();
    let progress_handle = thread::spawn(move || {
        let start_time = Instant::now();
        while start_time
            .elapsed()
            .as_secs_f32()
            < play_duration
        {
            let current_pos = {
                let pos = state_clone
                    .position
                    .lock()
                    .unwrap();
                *pos
            };

            progress_manager_clone
                .set_position("playback", current_pos as u64)
                .unwrap();

            // 计算播放百分比
            let percent =
                (current_pos as f32 / total_samples as f32 * 100.0) as u8;
            let msg = format!("Playing... {}%", percent);
            progress_manager_clone
                .set_message("playback", &msg)
                .unwrap();

            thread::sleep(Duration::from_millis(100));
        }
    });

    // 等待播放完成
    thread::sleep(Duration::from_secs_f32(play_duration));

    // 停止播放
    {
        let mut playing = state
            .is_playing
            .lock()
            .unwrap();
        *playing = false;
    }

    // 等待进度更新线程结束
    let _ = progress_handle.join();

    // 完成进度条
    progress_manager
        .finish("playback", "Playback completed")
        .unwrap();

    // 断开连接并停用客户端
    if let Err(err) = active_client.deactivate() {
        warn!("Error deactivating client: {}", err);
    }
}

#[derive(Clone)]
struct PlaybackState {
    samples: Arc<Mutex<Vec<f32>>>,
    position: Arc<Mutex<usize>>,
    is_playing: Arc<Mutex<bool>>,
    should_loop: bool,
    progress_manager: ProgressManager,
}

fn read_pcm_with_metadata(
    path: &str,
) -> std::io::Result<(u32, u16, u16, Vec<f32>)> {
    use std::fs::File;
    use std::io::Read;

    let mut file = File::open(path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;

    // 尝试读取带元数据的 PCM 文件
    match common::loads_with_metadata(&data) {
        Ok((metadata, samples_f64)) => {
            info!("Reading PCM file with metadata: {}", path);
            info!(
                "Metadata: {} Hz, {} channels, {} bits, {} samples",
                metadata.sample_rate,
                metadata.channels,
                metadata.bit_depth,
                metadata.data_length
            );

            let samples_f32: Vec<f32> = samples_f64
                .iter()
                .map(|&x| x as f32)
                .collect();
            Ok((
                metadata.sample_rate,
                metadata.channels,
                metadata.bit_depth,
                samples_f32,
            ))
        }
        Err(_) => {
            // 如果不是带元数据的文件，回退到原始方法
            info!(
                "File is not in metadata PCM format, reading with default parameters"
            );
            let samples = read_pcm_file(path, 8000, 1, 16)?;
            Ok((8000, 1, 16, samples))
        }
    }
}

fn read_pcm_file(
    path: &str,
    sample_rate: u32,
    channels: u16,
    bit_depth: u16,
) -> std::io::Result<Vec<f32>> {
    use std::fs::File;
    use std::io::{BufReader, Read};

    let mut file = BufReader::new(File::open(path)?);
    let mut samples = Vec::new();

    info!("Reading PCM file: {}", path);
    info!(
        "Parameters: {}Hz, {} channels, {} bits",
        sample_rate, channels, bit_depth
    );

    match bit_depth {
        8 => {
            let mut buffer = [0u8; 1024];
            loop {
                let bytes_read = file.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }

                for &byte in &buffer[..bytes_read] {
                    // 8位无符号转有符号，然后归一化到 [-1.0, 1.0]
                    let sample = (byte as i8 as f32) / 128.0;
                    samples.push(sample);
                }
            }
        }
        16 => {
            let mut buffer = [0u8; 2048];
            loop {
                let bytes_read = file.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }

                for chunk in buffer[..bytes_read].chunks(2) {
                    if chunk.len() == 2 {
                        let sample = i16::from_le_bytes([chunk[0], chunk[1]])
                            as f32
                            / 32768.0;
                        samples.push(sample);
                    }
                }
            }
        }
        24 => {
            let mut buffer = [0u8; 3072];
            loop {
                let bytes_read = file.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }

                for chunk in buffer[..bytes_read].chunks(3) {
                    if chunk.len() == 3 {
                        // 24位转32位有符号整数
                        let sample = i32::from_le_bytes([
                            chunk[0], chunk[1], chunk[2], 0,
                        ]) as f32
                            / 8388608.0;
                        samples.push(sample);
                    }
                }
            }
        }
        32 => {
            let mut buffer = [0u8; 4096];
            loop {
                let bytes_read = file.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }

                for chunk in buffer[..bytes_read].chunks(4) {
                    if chunk.len() == 4 {
                        let sample = i32::from_le_bytes([
                            chunk[0], chunk[1], chunk[2], chunk[3],
                        ]) as f32
                            / 2147483648.0;
                        samples.push(sample);
                    }
                }
            }
        }
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("不支持的位深度: {}", bit_depth),
            ));
        }
    }

    // 如果是立体声，转换为单声道（取平均值）
    if channels == 2 {
        let mut mono_samples = Vec::new();
        for chunk in samples.chunks(2) {
            if chunk.len() == 2 {
                mono_samples.push((chunk[0] + chunk[1]) / 2.0);
            }
        }
        samples = mono_samples;
    }

    info!("Read {} samples", samples.len());
    Ok(samples)
}

fn high_quality_resample(
    input_samples: &[f32],
    input_rate: u32,
    output_rate: u32,
) -> std::io::Result<Vec<f32>> {
    if input_rate == output_rate {
        return Ok(input_samples.to_vec());
    }

    let ratio = output_rate as f64 / input_rate as f64;
    info!(
        "High-quality resampling: {} Hz -> {} Hz (ratio: {:.3})",
        input_rate, output_rate, ratio
    );

    // 配置高质量重采样参数
    let params = SincInterpolationParameters {
        sinc_len: 256,  // 更长的 sinc 滤波器
        f_cutoff: 0.95, // 截止频率
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256, // 高过采样因子
        window: WindowFunction::BlackmanHarris2, // 高质量窗函数
    };

    // 创建重采样器
    let mut resampler = SincFixedIn::<f32>::new(
        ratio,
        2.0, // 最大比例
        params,
        input_samples.len(),
        1, // 单声道
    )
    .map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("重采样器创建失败: {}", e),
        )
    })?;

    // 准备输入数据（Rubato 需要 Vec<Vec<f32>> 格式）
    let input = vec![input_samples.to_vec()];

    // 执行重采样
    let output = resampler
        .process(&input, None)
        .map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("重采样处理失败: {}", e),
            )
        })?;

    let result = output[0].clone();
    info!(
        "High-quality resampling completed: {} -> {} samples",
        input_samples.len(),
        result.len()
    );
    Ok(result)
}

fn create_pcm_process_callback(
    mut out_port: jack::Port<jack::AudioOut>,
    state: PlaybackState,
    gain: f32,
) -> impl FnMut(&jack::Client, &jack::ProcessScope) -> jack::Control + Send + 'static
{
    move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        let out_buffer = out_port.as_mut_slice(ps);

        // 清零输出缓冲区
        for sample in out_buffer.iter_mut() {
            *sample = 0.0;
        }

        // 检查是否正在播放
        let is_playing = {
            let playing = state
                .is_playing
                .lock()
                .unwrap();
            *playing
        };

        if !is_playing {
            return jack::Control::Continue;
        }

        // 获取当前播放位置
        let (current_pos, samples_len) = {
            let mut pos = state.position.lock().unwrap();
            let samples = state.samples.lock().unwrap();
            let current = *pos;
            let len = samples.len();
            *pos = current;
            (current, len)
        };

        if current_pos >= samples_len {
            if state.should_loop {
                // 循环播放：重置位置
                let mut pos = state.position.lock().unwrap();
                *pos = 0;
            } else {
                // 停止播放
                let mut playing = state
                    .is_playing
                    .lock()
                    .unwrap();
                *playing = false;
                return jack::Control::Continue;
            }
        }

        // 填充音频缓冲区
        let samples = state.samples.lock().unwrap();
        let mut pos = state.position.lock().unwrap();

        for out_sample in out_buffer.iter_mut() {
            if *pos < samples.len() {
                *out_sample = samples[*pos] * gain;
                *pos += 1;
            } else if state.should_loop {
                *pos = 0;
                if *pos < samples.len() {
                    *out_sample = samples[*pos] * gain;
                    *pos += 1;
                }
            } else {
                break;
            }
        }

        jack::Control::Continue
    }
}
