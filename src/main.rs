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

    /// Ping a remote host
    Ping {
        /// Target IP address
        target: String,

        /// Local IP address
        #[arg(long, default_value = "192.168.1.1")]
        local_ip: String,
    },

    /// Run as an IP Host (respond to pings)
    IpHost {
        /// Local IP address
        #[arg(long, default_value = "192.168.1.2")]
        local_ip: String,
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
                Commands::Ping { target, local_ip } => {
                    // Ping Mode
                    run_ping(target, local_ip);
                    return;
                }
                Commands::IpHost { local_ip } => {
                    // IP Host Mode
                    run_ip_host(local_ip);
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

fn run_ping(target: String, local_ip_str: String) {
    use crate::mac::ip_interface::IpInterface;
    use std::net::Ipv4Addr;
    use trackmaker_rs::net::arp::ArpTable;
    use trackmaker_rs::net::icmp::{IcmpPacket, IcmpType};
    use trackmaker_rs::net::ip::Ipv4Header;

    let target_ip: Ipv4Addr = target
        .parse()
        .expect("Invalid target IP");
    let local_ip: Ipv4Addr = local_ip_str
        .parse()
        .expect("Invalid local IP");

    let arp = ArpTable::new();
    let dest_mac = arp
        .get_mac(&target_ip)
        .expect("Target IP not in ARP table");
    let local_mac = arp
        .get_mac(&local_ip)
        .expect("Local IP not in ARP table");

    info!(
        "PING {} ({}) from {} ({})",
        target_ip, dest_mac, local_ip, local_mac
    );

    // Setup JACK
    let (client, _status) = jack::Client::new(
        &format!("{}_ping_{}", JACK_CLIENT_NAME, rand::random::<u16>()),
        jack::ClientOptions::NO_START_SERVER,
    )
    .unwrap();

    let sample_rate = client.sample_rate() as u32;
    let shared = recorder::AppShared::new(sample_rate as usize * 10); // 10s buffer
    let shared_cb = shared.clone();

    let in_port = client
        .register_port(INPUT_PORT_NAME, jack::AudioIn::default())
        .unwrap();
    let out_port = client
        .register_port(OUTPUT_PORT_NAME, jack::AudioOut::default())
        .unwrap();
    let in_name = in_port.name().unwrap();
    let out_name = out_port.name().unwrap();

    let process = jack::contrib::ClosureProcessHandler::new(
        recorder::build_process_closure(
            in_port,
            out_port,
            shared_cb,
            sample_rate as usize * 10,
        ),
    );
    let active_client = client
        .activate_async((), process)
        .unwrap();
    connect_system_ports(active_client.as_client(), &in_name, &out_name);

    let mut interface = IpInterface::new(
        shared.clone(),
        sample_rate,
        LineCodingKind::FourBFiveB,
        local_mac,
    );

    // Statistics
    let mut packets_sent = 0u32;
    let mut packets_received = 0u32;
    let mut rtt_times: Vec<f32> = Vec::new();
    let ping_start = std::time::Instant::now();

    for seq in 0..4 {
        let payload = vec![0u8; 32]; // 32 bytes payload
        let icmp = IcmpPacket::new(
            IcmpType::EchoRequest,
            0,
            1234,
            seq,
            payload.clone(),
        );
        let icmp_bytes = icmp.to_bytes().unwrap();

        let ip = Ipv4Header::new(
            (20 + icmp_bytes.len()) as u16,
            seq,
            64,
            1, // ICMP
            local_ip.octets(),
            target_ip.octets(),
        );
        let mut ip_bytes = ip.to_bytes().unwrap();
        ip_bytes.extend(icmp_bytes);

        info!("Sending ICMP Echo Request seq={}...", seq);
        let start = std::time::Instant::now();

        if let Err(e) = interface.send_packet(&ip_bytes, dest_mac) {
            error!("Failed to send packet: {}", e);
            continue;
        }
        packets_sent += 1;

        // Wait for reply
        match interface
            .receive_packet(Some(std::time::Duration::from_millis(2000)))
        {
            Ok(data) => {
                let rtt = start.elapsed();
                let rtt_ms = rtt.as_secs_f32() * 1000.0;

                // Parse IP
                if let Ok(ip_header) = Ipv4Header::from_bytes(&data) {
                    // Parse ICMP
                    let icmp_data = &data[20..]; // Assuming no options
                    if let Ok(icmp_header) = IcmpPacket::from_bytes(icmp_data) {
                        if icmp_header.icmp_type == IcmpType::EchoReply
                            && icmp_header.sequence_number == seq
                        {
                            packets_received += 1;
                            rtt_times.push(rtt_ms);

                            info!(
                                "Reply from {}: bytes={} time={:.2}ms TTL={}",
                                target_ip,
                                data.len(),
                                rtt_ms,
                                ip_header.ttl
                            );
                        }
                    }
                }
            }
            Err(e) => {
                warn!("Request timed out: {}", e);
            }
        }

        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    // Print statistics
    let total_time = ping_start.elapsed();
    info!("\n--- {} ping statistics ---", target_ip);
    info!(
        "{} packets transmitted, {} received, {:.1}% packet loss, time {:.2}s",
        packets_sent,
        packets_received,
        if packets_sent > 0 {
            ((packets_sent - packets_received) as f32 / packets_sent as f32)
                * 100.0
        } else {
            0.0
        },
        total_time.as_secs_f32()
    );

    if !rtt_times.is_empty() {
        let min_rtt = rtt_times
            .iter()
            .cloned()
            .fold(f32::INFINITY, f32::min);
        let max_rtt = rtt_times
            .iter()
            .cloned()
            .fold(f32::NEG_INFINITY, f32::max);
        let avg_rtt = rtt_times.iter().sum::<f32>() / rtt_times.len() as f32;

        info!(
            "rtt min/avg/max = {:.2}/{:.2}/{:.2} ms",
            min_rtt, avg_rtt, max_rtt
        );
    }
}

fn run_ip_host(local_ip_str: String) {
    use crate::mac::ip_interface::IpInterface;
    use std::net::Ipv4Addr;
    use trackmaker_rs::net::arp::ArpTable;
    use trackmaker_rs::net::icmp::{IcmpPacket, IcmpType};
    use trackmaker_rs::net::ip::Ipv4Header;

    let local_ip: Ipv4Addr = local_ip_str
        .parse()
        .expect("Invalid local IP");
    let arp = ArpTable::new();
    let local_mac = arp
        .get_mac(&local_ip)
        .expect("Local IP not in ARP table");

    info!("Starting IP Host on {} ({})", local_ip, local_mac);

    // Setup JACK
    let (client, _status) = jack::Client::new(
        &format!("{}_host_{}", JACK_CLIENT_NAME, rand::random::<u16>()),
        jack::ClientOptions::NO_START_SERVER,
    )
    .unwrap();

    let sample_rate = client.sample_rate() as u32;
    let shared = recorder::AppShared::new(sample_rate as usize * 10);
    let shared_cb = shared.clone();

    let in_port = client
        .register_port(INPUT_PORT_NAME, jack::AudioIn::default())
        .unwrap();
    let out_port = client
        .register_port(OUTPUT_PORT_NAME, jack::AudioOut::default())
        .unwrap();
    let in_name = in_port.name().unwrap();
    let out_name = out_port.name().unwrap();

    let process = jack::contrib::ClosureProcessHandler::new(
        recorder::build_process_closure(
            in_port,
            out_port,
            shared_cb,
            sample_rate as usize * 10,
        ),
    );
    let active_client = client
        .activate_async((), process)
        .unwrap();
    connect_system_ports(active_client.as_client(), &in_name, &out_name);

    let mut interface = IpInterface::new(
        shared.clone(),
        sample_rate,
        LineCodingKind::FourBFiveB,
        local_mac,
    );

    loop {
        if let Ok(data) = interface.receive_packet(None) {
            if let Ok(ip_header) = Ipv4Header::from_bytes(&data) {
                // Check if it's for us
                if ip_header.dest_ip == local_ip.octets() {
                    let icmp_data = &data[20..];
                    if let Ok(icmp_header) = IcmpPacket::from_bytes(icmp_data) {
                        if icmp_header.icmp_type == IcmpType::EchoRequest {
                            info!(
                                "Received ICMP Echo Request from {:?}",
                                ip_header.source_ip
                            );

                            // Send Reply
                            let reply_icmp = IcmpPacket::new(
                                IcmpType::EchoReply,
                                0,
                                icmp_header.identifier,
                                icmp_header.sequence_number,
                                icmp_header.payload,
                            );
                            let reply_icmp_bytes =
                                reply_icmp.to_bytes().unwrap();

                            let reply_ip = Ipv4Header::new(
                                (20 + reply_icmp_bytes.len()) as u16,
                                0,
                                64,
                                1,
                                local_ip.octets(),
                                ip_header.source_ip,
                            );
                            let mut reply_bytes = reply_ip.to_bytes().unwrap();
                            reply_bytes.extend(reply_icmp_bytes);

                            // Find dest MAC
                            let src_ip = Ipv4Addr::from(ip_header.source_ip);
                            if let Some(dest_mac) = arp.get_mac(&src_ip) {
                                info!(
                                    "Sending Echo Reply to {} ({})",
                                    src_ip, dest_mac
                                );
                                if let Err(e) =
                                    interface.send_packet(&reply_bytes, dest_mac)
                                {
                                    error!("Failed to send reply: {}", e);
                                }
                            } else {
                                warn!(
                                    "Unknown source IP {}, cannot reply",
                                    src_ip
                                );
                            }
                        }
                    }
                }
            }
        }
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

    (selection, line_coding, tx_addr, rx_addr, 60u64)
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
