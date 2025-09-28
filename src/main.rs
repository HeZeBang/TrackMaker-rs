use jack;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use clap::{Parser, Subcommand};

mod audio;
mod device;
mod ui;
mod utils;
mod amodem;

use device::jack::{
    connect_input_from_first_system_output,
    connect_output_to_first_system_input, disconnect_input_sources,
    disconnect_output_sinks, print_jack_info,
};
use ui::print_banner;
use utils::logging::init_logging;
use amodem::{config::Configuration, detect::Detector, recv::Receiver, send, common};
use rubato::{Resampler, SincFixedIn, SincInterpolationType, SincInterpolationParameters, WindowFunction};

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
    /// Receive mode: decode audio from JACK input to file
    Receive {
        /// Output file for decoded data
        #[arg(short, long, default_value = "decoded_output.txt")]
        output: PathBuf,
        
        /// Duration to record in seconds
        #[arg(short, long, default_value = "10.0")]
        duration: f32,
        
        /// Bitrate configuration (1 or 2)
        #[arg(short, long, default_value = "1")]
        bitrate: u32,
    },
    
    /// Send mode: encode file content and play through JACK
    Send {
        /// Input file to encode and transmit
        #[arg(short, long)]
        input: PathBuf,
        
        /// Duration to transmit in seconds
        #[arg(short, long, default_value = "10.0")]
        duration: f32,
        
        /// Bitrate configuration (1 or 2)
        #[arg(short, long, default_value = "1")]
        bitrate: u32,
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
    },
}

fn main() {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Receive { output, duration, bitrate } => {
            receive_mode(output, duration, bitrate);
        }
        Commands::Send { input, duration, bitrate } => {
            send_mode(input, duration, bitrate);
        }
        Commands::Test { message, duration, bitrate } => {
            test_mode(message, duration, bitrate);
        }
    }
}

fn receive_mode(output: PathBuf, duration: f32, bitrate: u32) {
    print_banner();
    
    println!("🎧 Amodem Receive Mode");
    println!("📁 Output file: {}", output.display());
    println!("⏱️  Duration: {:.1} seconds", duration);
    println!("📡 Bitrate: {} kb/s", bitrate);
    println!();
    
    // Get configuration
    let config = match bitrate {
        1 => Configuration::bitrate_1(),
        2 => Configuration::bitrate_2(),
        _ => {
            eprintln!("❌ Invalid bitrate: {}. Supported values: 1, 2", bitrate);
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
        .register_port("input", jack::AudioIn::default())
        .unwrap();
    
    let in_port_name = in_port.name().unwrap();

    // Process callback for recording
    let process_cb = move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
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

    let active_client = client.activate_async((), process).unwrap();

    // Connect to system input
    connect_input_from_first_system_output(active_client.as_client(), &in_port_name);
    
    println!("🔍 Waiting for audio input...");
    println!("📡 Listening for amodem signal on JACK input");
    println!("⏳ Recording for {:.1} seconds...", duration);
    
    // Wait for recording to complete
    loop {
        thread::sleep(Duration::from_millis(100));
        if *finished.lock().unwrap() {
            break;
        }
    }
    
    println!("✅ Recording complete");
    
    // Disconnect and deactivate
    disconnect_input_sources(active_client.as_client(), &in_port_name);
    active_client.deactivate().unwrap();
    
    // Get recorded audio
    let recorded_samples = audio_buffer.lock().unwrap().clone();
    println!("📊 Recorded {} samples", recorded_samples.len());
    
    // Convert to amodem format (f64, 8kHz)
    let amodem_samples_f32 = resample_audio(&recorded_samples, sample_rate as f32, config.fs as f32);
    let amodem_samples: Vec<f64> = amodem_samples_f32.iter().map(|&x| x as f64).collect();
    println!("🔄 Resampled to {} samples at {:.1} kHz", amodem_samples.len(), config.fs / 1000.0);
    
    // Save raw audio for debugging with metadata
    let pcm_data = common::dumps_with_metadata(&amodem_samples, config.fs as u32, 1, 16);
    std::fs::write("tmp/recorded.pcm", &pcm_data).unwrap();
    println!("💾 Raw audio saved to tmp/recorded.pcm ({} Hz, 16-bit, mono)", config.fs as u32);
    
    // Decode using amodem
    println!("🔍 Decoding amodem signal...");
    match decode_amodem_signal(&amodem_samples, &config) {
        Ok(decoded_data) => {
            // Write decoded data to output file
            std::fs::write(&output, &decoded_data).unwrap();
            println!("✅ Successfully decoded {} bytes to {}", decoded_data.len(), output.display());
            
            // Display decoded content
            if let Ok(text) = String::from_utf8(decoded_data.clone()) {
                println!("📄 Decoded content:");
                println!("{}", text);
            } else {
                println!("📄 Decoded binary data ({} bytes)", decoded_data.len());
            }
        }
        Err(e) => {
            eprintln!("❌ Failed to decode amodem signal: {}", e);
            std::fs::write(&output, format!("Decode error: {}", e)).unwrap();
        }
    }
}

fn send_mode(input: PathBuf, duration: f32, bitrate: u32) {
    print_banner();
    
    println!("📡 Amodem Send Mode");
    println!("📁 Input file: {}", input.display());
    println!("⏱️  Duration: {:.1} seconds", duration);
    println!("📡 Bitrate: {} kb/s", bitrate);
    println!();
    
    // Check if input file exists
    if !input.exists() {
        eprintln!("❌ Input file does not exist: {}", input.display());
        return;
    }
    
    // Get configuration
    let config = match bitrate {
        1 => Configuration::bitrate_1(),
        2 => Configuration::bitrate_2(),
        _ => {
            eprintln!("❌ Invalid bitrate: {}. Supported values: 1, 2", bitrate);
            return;
        }
    };
    
    // Read input file
    let input_data = match std::fs::read(&input) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("❌ Failed to read input file: {}", e);
            return;
        }
    };
    
    println!("📄 Read {} bytes from input file", input_data.len());
    
    // Generate amodem audio
    println!("🎵 Generating amodem audio...");
    let audio_samples = generate_amodem_audio(&input_data, &config);
    println!("🎵 Generated {} audio samples", audio_samples.len());
    
    // Save generated audio for debugging with metadata
    let pcm_data = common::dumps_with_metadata(&audio_samples, config.fs as u32, 1, 16);
    let pcm_path = "tmp/generated.pcm";
    std::fs::write(pcm_path, &pcm_data).unwrap();
    println!("💾 Generated audio saved to {} ({} Hz, 16-bit, mono)", pcm_path, config.fs as u32);
    
    // Use PCM player to play the generated audio
    println!("🎵 Starting PCM playback...");
    play_pcm_file(pcm_path, duration);
}

fn test_mode(message: String, duration: f32, bitrate: u32) {
    print_banner();
    
    println!("🧪 Amodem Test Mode (Direct Memory)");
    println!("💬 Test message: \"{}\"", message);
    println!("⏱️  Duration: {:.1} seconds per phase", duration);
    println!("📡 Bitrate: {} kb/s", bitrate);
    println!();
    
    // Get configuration
    let config = match bitrate {
        1 => Configuration::bitrate_1(),
        2 => Configuration::bitrate_2(),
        _ => {
            eprintln!("❌ Invalid bitrate: {}. Supported values: 1, 2", bitrate);
            return;
        }
    };
    
    // Phase 1: Generate amodem audio in memory
    println!("📡 Phase 1: Generating amodem audio...");
    let message_bytes = message.as_bytes();
    let audio_samples = generate_amodem_audio(message_bytes, &config);
    println!("🎵 Generated {} audio samples", audio_samples.len());
    
    // Save generated audio for debugging with metadata
    let pcm_data = common::dumps_with_metadata(&audio_samples, config.fs as u32, 1, 16);
    std::fs::write("tmp/test_generated.pcm", &pcm_data).unwrap();
    println!("💾 Generated audio saved to tmp/test_generated.pcm ({} Hz, 16-bit, mono)", config.fs as u32);
    
    // Phase 2: Decode the audio directly from memory
    println!("🔍 Phase 2: Decoding amodem signal...");
    match decode_amodem_signal(&audio_samples, &config) {
        Ok(decoded_data) => {
            // Compare results
            println!();
            println!("📊 Test Results:");
            println!("📤 Sent: \"{}\"", message);
            
            // Display decoded content
            if let Ok(decoded_text) = String::from_utf8(decoded_data.clone()) {
                println!("📥 Received: \"{}\"", decoded_text.trim());
                
                if message == decoded_text.trim() {
                    println!("✅ Test PASSED! Message received correctly.");
                } else {
                    println!("❌ Test FAILED! Message mismatch.");
                    println!("   Expected length: {} bytes", message.len());
                    println!("   Received length: {} bytes", decoded_text.len());
                }
            } else {
                println!("📥 Received: {} bytes of binary data", decoded_data.len());
                println!("📄 Raw bytes: {:02x?}", &decoded_data[..decoded_data.len().min(50)]);
                
                // Check if it's the same binary data
                if message_bytes == &decoded_data[..] {
                    println!("✅ Test PASSED! Binary data matches.");
                } else {
                    println!("❌ Test FAILED! Binary data mismatch.");
                    println!("   Expected length: {} bytes", message_bytes.len());
                    println!("   Received length: {} bytes", decoded_data.len());
                }
            }
            
            // Save decoded result for inspection
            std::fs::write("tmp/test_result.txt", &decoded_data).unwrap();
            println!("💾 Decoded result saved to tmp/test_result.txt");
        }
        Err(e) => {
            eprintln!("❌ Failed to decode amodem signal: {}", e);
            std::fs::write("tmp/test_result.txt", format!("Decode error: {}", e)).unwrap();
        }
    }
}

fn decode_amodem_signal(samples: &[f64], config: &Configuration) -> Result<Vec<u8>, String> {
    // Create detector and receiver
    let detector = Detector::new(config);
    let mut receiver = Receiver::new(config);
    
    // Detect carrier and get signal
    let (signal, amplitude, _freq_error) = detector.run(samples.iter().cloned())?;
    
    // Decode the signal
    let mut output = Vec::new();
    receiver.run(signal, 1.0 / amplitude, &mut output)?;
    
    Ok(output)
}

fn generate_amodem_audio(data: &[u8], config: &Configuration) -> Vec<f64> {
    let mut output = Vec::new();
    
    send::send(config, &data[..], &mut output, 1.0, 0.0).unwrap();
    
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
    use std::fs::File;
    use std::io::Read;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;
    
    // 读取 PCM 文件
    let (file_sample_rate, file_channels, file_bit_depth, samples) = match read_pcm_with_metadata(pcm_path) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("❌ 读取 PCM 文件失败: {}", e);
            return;
        }
    };
    
    if samples.is_empty() {
        eprintln!("❌ PCM 文件为空");
        return;
    }
    
    println!("📁 读取 PCM 文件: {} Hz, {} 声道, {} 位, {} 样本", 
             file_sample_rate, file_channels, file_bit_depth, samples.len());
    
    // 设置 JACK 客户端
    let (client, _status) = match jack::Client::new(
        "amodem-send-pcm",
        jack::ClientOptions::NO_START_SERVER,
    ) {
        Ok(client) => client,
        Err(e) => {
            eprintln!("❌ JACK 客户端创建失败: {}", e);
            return;
        }
    };
    
    let jack_sample_rate = client.sample_rate();
    println!("🎵 JACK 采样率: {} Hz", jack_sample_rate);
    
    // 重采样到 JACK 采样率
    let resampled_samples = match high_quality_resample(&samples, file_sample_rate, jack_sample_rate as u32) {
        Ok(samples) => samples,
        Err(e) => {
            eprintln!("❌ 重采样失败: {}", e);
            return;
        }
    };
    
    // 创建播放状态
    let state = PlaybackState {
        samples: Arc::new(Mutex::new(resampled_samples)),
        position: Arc::new(Mutex::new(0)),
        is_playing: Arc::new(Mutex::new(true)),
        should_loop: false,
    };
    
    // 注册输出端口
    let out_port = match client.register_port("pcm_out", jack::AudioOut::default()) {
        Ok(port) => port,
        Err(e) => {
            eprintln!("❌ 端口注册失败: {}", e);
            return;
        }
    };
    
    let out_port_name = match out_port.name() {
        Ok(name) => name,
        Err(e) => {
            eprintln!("❌ 获取端口名称失败: {}", e);
            return;
        }
    };
    
    // 创建处理回调
    let process_callback = create_pcm_process_callback(out_port, state.clone(), 1.0);
    let process = jack::contrib::ClosureProcessHandler::new(process_callback);
    
    // 激活客户端
    let active_client = match client.activate_async((), process) {
        Ok(client) => client,
        Err(e) => {
            eprintln!("❌ 客户端激活失败: {}", e);
            return;
        }
    };
    
    // 连接到系统输出端口
    let system_input_ports = active_client.as_client().ports(
        None,
        None,
        jack::PortFlags::IS_INPUT | jack::PortFlags::IS_PHYSICAL,
    );
    
    if let Some(system_in) = system_input_ports.first() {
        match active_client.as_client().connect_ports_by_name(&out_port_name, system_in) {
            Ok(_) => println!("🔗 已连接输出: {} -> {}", out_port_name, system_in),
            Err(e) => eprintln!("⚠️  连接输出失败: {}", e),
        }
    } else {
        eprintln!("⚠️  未找到系统输入端口");
    }
    
    println!("🎵 开始播放 PCM 文件...");
    
    // 等待播放完成
    thread::sleep(Duration::from_secs_f32(duration));
    
    // 停止播放
    {
        let mut playing = state.is_playing.lock().unwrap();
        *playing = false;
    }
    
    println!("✅ 播放完成");
    
    // 断开连接并停用客户端
    if let Err(err) = active_client.deactivate() {
        eprintln!("⚠️  停用客户端时出错: {}", err);
    }
}

#[derive(Clone)]
struct PlaybackState {
    samples: Arc<Mutex<Vec<f32>>>,
    position: Arc<Mutex<usize>>,
    is_playing: Arc<Mutex<bool>>,
    should_loop: bool,
}

fn read_pcm_with_metadata(path: &str) -> std::io::Result<(u32, u16, u16, Vec<f32>)> {
    use std::fs::File;
    use std::io::Read;
    
    let mut file = File::open(path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    
    // 尝试读取带元数据的 PCM 文件
    match common::loads_with_metadata(&data) {
        Ok((metadata, samples_f64)) => {
            println!("📁 读取带元数据的 PCM 文件: {}", path);
            println!("📊 元数据: {} Hz, {} 声道, {} 位, {} 样本", 
                     metadata.sample_rate, metadata.channels, metadata.bit_depth, metadata.data_length);
            
            let samples_f32: Vec<f32> = samples_f64.iter().map(|&x| x as f32).collect();
            Ok((metadata.sample_rate, metadata.channels, metadata.bit_depth, samples_f32))
        }
        Err(_) => {
            // 如果不是带元数据的文件，回退到原始方法
            println!("📁 文件不是带元数据的 PCM 格式，使用默认参数读取");
            let samples = read_pcm_file(path, 8000, 1, 16)?;
            Ok((8000, 1, 16, samples))
        }
    }
}

fn read_pcm_file(path: &str, sample_rate: u32, channels: u16, bit_depth: u16) -> std::io::Result<Vec<f32>> {
    use std::fs::File;
    use std::io::{BufReader, Read};
    
    let mut file = BufReader::new(File::open(path)?);
    let mut samples = Vec::new();
    
    println!("📁 读取 PCM 文件: {}", path);
    println!("📊 参数: {}Hz, {}声道, {}位", sample_rate, channels, bit_depth);
    
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
                        let sample = i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0;
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
                        let sample = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]) as f32 / 8388608.0;
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
                        let sample = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f32 / 2147483648.0;
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
    
    println!("📊 读取了 {} 个样本", samples.len());
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
    println!("🔄 高质量重采样: {} Hz -> {} Hz (比例: {:.3})", input_rate, output_rate, ratio);
    
    // 配置高质量重采样参数
    let params = SincInterpolationParameters {
        sinc_len: 256,                    // 更长的 sinc 滤波器
        f_cutoff: 0.95,                   // 截止频率
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,         // 高过采样因子
        window: WindowFunction::BlackmanHarris2, // 高质量窗函数
    };
    
    // 创建重采样器
    let mut resampler = SincFixedIn::<f32>::new(
        ratio,
        2.0, // 最大比例
        params,
        input_samples.len(),
        1, // 单声道
    ).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("重采样器创建失败: {}", e)))?;
    
    // 准备输入数据（Rubato 需要 Vec<Vec<f32>> 格式）
    let input = vec![input_samples.to_vec()];
    
    // 执行重采样
    let output = resampler.process(&input, None)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("重采样处理失败: {}", e)))?;
    
    let result = output[0].clone();
    println!("✅ 高质量重采样完成: {} -> {} 样本", input_samples.len(), result.len());
    Ok(result)
}

fn create_pcm_process_callback(
    mut out_port: jack::Port<jack::AudioOut>,
    state: PlaybackState,
    gain: f32,
) -> impl FnMut(&jack::Client, &jack::ProcessScope) -> jack::Control + Send + 'static {
    move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        let out_buffer = out_port.as_mut_slice(ps);
        
        // 清零输出缓冲区
        for sample in out_buffer.iter_mut() {
            *sample = 0.0;
        }
        
        // 检查是否正在播放
        let is_playing = {
            let playing = state.is_playing.lock().unwrap();
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
                let mut playing = state.is_playing.lock().unwrap();
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
