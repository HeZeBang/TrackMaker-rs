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
    std::fs::write("tmp/generated.pcm", &pcm_data).unwrap();
    println!("💾 Generated audio saved to tmp/generated.pcm ({} Hz, 16-bit, mono)", config.fs as u32);
    
    // Setup JACK for playback
    let (client, _status) = jack::Client::new(
        "amodem-send",
        jack::ClientOptions::NO_START_SERVER,
    )
    .unwrap();
    
    let (sample_rate, _buffer_size) = print_jack_info(&client);
    
    // Resample to JACK sample rate
    let audio_samples_f32: Vec<f32> = audio_samples.iter().map(|&x| x as f32).collect();
    let jack_audio = resample_audio(&audio_samples_f32, config.fs as f32, sample_rate as f32);
    
    // Truncate or repeat to match duration
    let target_samples = (sample_rate as f32 * duration) as usize;
    let playback_audio = if jack_audio.len() > target_samples {
        jack_audio[..target_samples].to_vec()
    } else {
        let mut repeated = jack_audio.clone();
        while repeated.len() < target_samples {
            repeated.extend_from_slice(&jack_audio);
        }
        repeated[..target_samples].to_vec()
    };
    
    // Setup playback
    let audio_buffer = Arc::new(Mutex::new(playback_audio.clone()));
    let buffer_clone = audio_buffer.clone();
    let finished = Arc::new(Mutex::new(false));
    let finished_clone = finished.clone();
    
    let mut out_port = client
        .register_port("output", jack::AudioOut::default())
        .unwrap();
    
    let out_port_name = out_port.name().unwrap();
    
    // Process callback for playback
    let process_cb = move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        let output_buffer = out_port.as_mut_slice(ps);
        let mut buffer = buffer_clone.lock().unwrap();
        
        for frame in output_buffer.iter_mut() {
            if let Some(sample) = buffer.pop() {
                *frame = sample;
            } else {
                *finished_clone.lock().unwrap() = true;
                return jack::Control::Quit;
            }
        }
        
        jack::Control::Continue
    };
    
    let process = jack::contrib::ClosureProcessHandler::new(process_cb);
    let active_client = client.activate_async((), process).unwrap();
    
    // Connect to system output
    connect_output_to_first_system_input(active_client.as_client(), &out_port_name);
    
    println!("🔊 Playing amodem signal...");
    println!("📡 Transmitting through JACK output");
    println!("⏳ Playing for {:.1} seconds...", duration);
    
    // Wait for playback to complete
    loop {
        thread::sleep(Duration::from_millis(100));
        if *finished.lock().unwrap() {
            break;
        }
    }
    
    println!("✅ Transmission complete");
    
    // Disconnect and deactivate
    disconnect_output_sinks(active_client.as_client(), &out_port_name);
    active_client.deactivate().unwrap();
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
