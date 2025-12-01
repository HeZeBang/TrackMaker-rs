use clap::{Parser, Subcommand};
use dialoguer::{Input, Select, theme::ColorfulTheme};
use jack;
use tracing::{debug, error, info, warn};

mod audio;
mod device;
mod mac;
mod phy;
mod ui;
mod utils;

use audio::recorder;
use device::jack::{connect_system_ports, print_jack_info};
use rand::Rng;
use ui::print_banner;
use ui::progress::ProgressManager;
use utils::consts::*;
use utils::logging::init_logging;

use phy::{Frame, LineCodingKind, PhyDecoder, PhyEncoder};

use crate::mac::csma::{run_receiver, run_sender};

#[derive(Parser)]
#[command(name = "trackmaker-rs")]
#[command(about = "Audio-based wireless transmission system", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Enable interactive mode (dialoguer) instead of CLI args
    #[arg(long)]
    interactive: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Transmit a file
    Tx {
        /// Local sender address
        #[arg(short = 'l', long, default_value = "1")]
        local: u8,

        /// Remote receiver address
        #[arg(short = 'r', long, default_value = "2")]
        remote: u8,

        /// Line coding scheme (4b5b or manchester)
        #[arg(long, default_value = "4b5b")]
        encoding: String,
    },

    /// Receive a file
    Rx {
        /// Local receiver address
        #[arg(short = 'l', long, default_value = "2")]
        local: u8,

        /// Remote sender address
        #[arg(short = 'r', long, default_value = "1")]
        remote: u8,

        /// Line coding scheme (4b5b or manchester)
        #[arg(long, default_value = "4b5b")]
        encoding: String,

        /// Recording duration in seconds
        #[arg(short = 'd', long, default_value_t = DEFAULT_RECORD_SECONDS as u64)]
        duration: u64,
    },

    /// Test mode (loopback without JACK)
    Test {
        /// Line coding scheme (4b5b or manchester)
        #[arg(long, default_value = "4b5b")]
        encoding: String,
    },
}

fn parse_line_coding(encoding: &str) -> LineCodingKind {
    match encoding
        .to_lowercase()
        .as_str()
    {
        "manchester" | "manchester-biphase" => LineCodingKind::Manchester,
        "4b5b" | "4b5b-nrz" => LineCodingKind::FourBFiveB,
        _ => {
            warn!("Unknown encoding '{}', defaulting to 4B5B", encoding);
            LineCodingKind::FourBFiveB
        }
    }
}

fn main() {
    init_logging();
    print_banner();

    let cli = Cli::parse();

    // Determine mode and parameters
    let (selection, line_coding, tx_addr, rx_addr, rx_duration) =
        if cli.interactive || cli.command.is_none() {
            // Interactive mode (original dialoguer behavior)
            interactive_mode()
        } else {
            // Command-line mode
            match cli.command.unwrap() {
                Commands::Tx {
                    local,
                    remote,
                    encoding,
                } => {
                    let line_coding = parse_line_coding(&encoding);
                    info!("Using line coding: {}", line_coding.name());
                    (0, line_coding, local, remote, 60u64)
                }
                Commands::Rx {
                    local,
                    remote,
                    encoding,
                    duration,
                } => {
                    let line_coding = parse_line_coding(&encoding);
                    info!("Using line coding: {}", line_coding.name());
                    (1, line_coding, local, remote, duration)
                }
                Commands::Test { encoding } => {
                    let line_coding = parse_line_coding(&encoding);
                    test_transmission(line_coding);
                    return;
                }
            }
        };

    let (client, status) = jack::Client::new(
        format!(
            "{}_{:04}",
            JACK_CLIENT_NAME,
            rand::rng().random_range(0..10000)
        )
        .as_str(),
        jack::ClientOptions::NO_START_SERVER,
    )
    .unwrap();
    tracing::info!("JACK client status: {:?}", status);
    let (sample_rate, _buffer_size) = print_jack_info(&client);

    if sample_rate as u32 != SAMPLE_RATE {
        warn!(
            "Sample rate mismatch! Expected {}, got {}",
            SAMPLE_RATE, sample_rate
        );
        warn!("Physical layer is designed for {} Hz", SAMPLE_RATE);
    }

    let max_duration_samples = sample_rate * rx_duration as usize;

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

    {
        shared
            .record_buffer
            .lock()
            .unwrap()
            .clear();
    }

    if selection == 0 {
        // Sender
        run_sender(
            shared,
            progress_manager,
            sample_rate as u32,
            line_coding,
            tx_addr,
            rx_addr,
        );
    } else if selection == 1 {
        // Receiver
        run_receiver(
            shared,
            progress_manager,
            max_duration_samples as u32,
            line_coding,
            tx_addr,
            rx_addr,
            rx_duration,
        );
    } else {
        unreachable!();
    }

    info!("Exiting gracefully...");
    if let Err(err) = active_client.deactivate() {
        error!("Error deactivating client: {}", err);
    }
}

fn interactive_mode() -> (usize, LineCodingKind, u8, u8, u64) {
    let selections = &["Send File", "Receive File", "Test (No JACK - Loopback)"];
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select mode")
        .default(0)
        .items(&selections[..])
        .interact()
        .unwrap();

    if selection == 2 {
        // Test mode - return dummy values that won't be used
        let line_coding_options =
            [LineCodingKind::FourBFiveB, LineCodingKind::Manchester];
        let line_coding_labels = ["4B5B (NRZ)", "Manchester (Bi-phase)"];
        let line_coding_idx = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Select line coding scheme")
            .default(0)
            .items(&line_coding_labels)
            .interact()
            .unwrap();
        let line_coding = line_coding_options[line_coding_idx];
        test_transmission(line_coding);
        std::process::exit(0);
    }

    let line_coding_options =
        [LineCodingKind::FourBFiveB, LineCodingKind::Manchester];
    let line_coding_labels = ["4B5B (NRZ)", "Manchester (Bi-phase)"];
    let line_coding_idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select line coding scheme")
        .default(0)
        .items(&line_coding_labels)
        .interact()
        .unwrap();
    let line_coding = line_coding_options[line_coding_idx];

    let tx_addr =
        Input::<mac::types::MacAddr>::with_theme(&ColorfulTheme::default())
            .with_prompt("Enter local sender addr")
            .default(1)
            .interact()
            .unwrap();
    let rx_addr =
        Input::<mac::types::MacAddr>::with_theme(&ColorfulTheme::default())
            .with_prompt("Enter remote receiver addr")
            .default(2)
            .interact()
            .unwrap();

    (
        selection,
        line_coding,
        tx_addr,
        rx_addr,
        DEFAULT_RECORD_SECONDS as u64,
    )
}

fn test_transmission(line_coding: LineCodingKind) {
    info!("=== Test Mode (Loopback without JACK) ===");
    info!("Using line coding: {}", line_coding.name());

    // Create test data
    let test_text = format!(
        "114514Hello, Project 2! This is a test of cable-based transmission using {} line coding.",
        line_coding.name()
    );
    let test_data = test_text.into_bytes();
    info!("Test data: {} bytes", test_data.len());
    info!("Content: {}", String::from_utf8_lossy(&test_data));

    // Create encoder and decoder
    let encoder =
        PhyEncoder::new(SAMPLES_PER_LEVEL, PREAMBLE_PATTERN_BYTES, line_coding);
    let mut decoder = PhyDecoder::new(
        SAMPLES_PER_LEVEL,
        PREAMBLE_PATTERN_BYTES,
        line_coding,
        2,
    );

    // Create frames
    let mut frames = Vec::new();
    let mut seq = 0u8;

    for chunk in test_data.chunks(MAX_FRAME_DATA_SIZE) {
        let frame = Frame::new_data(seq, 0, 1, chunk.to_vec());
        frames.push(frame);
        seq = seq.wrapping_add(1);
    }

    info!("Created {} frames", frames.len());

    // Encode
    let samples = encoder.encode_frames(&frames, INTER_FRAME_GAP_SAMPLES);
    info!(
        "Encoded to {} samples ({:.2} seconds at {} Hz)",
        samples.len(),
        samples.len() as f32 / SAMPLE_RATE as f32,
        SAMPLE_RATE
    );

    // Save to WAV for inspection
    if let Err(e) = utils::dump::dump_to_wav(
        "./tmp/project2_test.wav",
        &utils::dump::AudioData {
            sample_rate: SAMPLE_RATE,
            audio_data: samples.clone(),
            duration: samples.len() as f32 / SAMPLE_RATE as f32,
            channels: 1,
        },
    ) {
        warn!("Failed to save WAV: {}", e);
    } else {
        info!("Saved test signal to ./tmp/project2_test.wav");
    }

    // Decode
    let decoded_frames = decoder.process_samples(&samples);
    info!("Decoded {} frames", decoded_frames.len());

    // Reconstruct data
    let mut decoded_data = Vec::new();
    for frame in decoded_frames {
        decoded_data.extend_from_slice(&frame.data);
    }

    // Compare
    if decoded_data == test_data {
        info!("✅ Test PASSED - Data matches perfectly!");
    } else {
        error!("❌ Test FAILED - Data mismatch");
        info!("Original: {} bytes", test_data.len());
        info!("Decoded:  {} bytes", decoded_data.len());

        // Find first difference
        for i in 0..test_data
            .len()
            .min(decoded_data.len())
        {
            if test_data[i] != decoded_data[i] {
                info!(
                    "First difference at byte {}: expected {:#04x}, got {:#04x}",
                    i,
                    test_data[i],
                    decoded_data
                        .get(i)
                        .unwrap_or(&0)
                );
                break;
            }
        }
    }

    // Performance stats
    let total_bits = test_data.len() * 8;
    let duration_s = samples.len() as f32 / SAMPLE_RATE as f32;
    let effective_bitrate = total_bits as f32 / duration_s;

    info!("Performance:");
    info!("  - Total bits: {}", total_bits);
    info!("  - Duration: {:.3} seconds", duration_s);
    info!("  - Effective bit rate: {:.0} bps", effective_bitrate);
    info!(
        "  - Overhead: {:.1}%",
        (1.0 - effective_bitrate / BIT_RATE as f32) * 100.0
    );
}
