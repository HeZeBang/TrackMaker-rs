use clap::{Parser, Subcommand};
use dialoguer::{Input, Select, theme::ColorfulTheme};
use jack;
use std::fs;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use tracing::{debug, error, info, warn};

mod audio;
mod device;
mod mac;
mod net;
mod phy;
mod ui;
mod utils;

use audio::recorder;
use device::jack::{connect_system_ports, print_jack_info};
use mac::csma::CsmaNode;
use phy::{Frame, LineCodingKind, PhyDecoder, PhyEncoder};
use rand::Rng;
use ui::print_banner;
use ui::progress::{ProgressManager, templates};
use utils::consts::*;
use utils::logging::init_logging;

use crate::phy::FrameType;

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

        /// Transmit Timeout in seconds
        #[arg(short = 'd', long, default_value_t = DEFAULT_TIMEOUT as u64)]
        duration: u64,
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
        #[arg(short = 'd', long, default_value_t = DEFAULT_TIMEOUT as u64)]
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

    /// Run as a Router (forward packets between acoustic and WiFi interfaces)
    Router {
        /// Local IP on acoustic side (connected to NODE1)
        #[arg(long, default_value = "192.168.1.2")]
        acoustic_ip: String,

        /// Local MAC on acoustic side
        #[arg(long, default_value = "2")]
        acoustic_mac: u8,

        /// Local IP on WiFi side (connected to NODE3)
        #[arg(long, default_value = "192.168.2.1")]
        wifi_ip: String,

        /// WiFi interface name (e.g., wlan0, wlp2s0)
        #[arg(long, default_value = "wlan0")]
        wifi_interface: String,

        /// NODE3 IP address (for static ARP entry)
        #[arg(long, default_value = "192.168.2.2")]
        node3_ip: String,

        /// NODE3 MAC address (for static ARP entry, format: aa:bb:cc:dd:ee:ff)
        #[arg(long)]
        node3_mac: Option<String>,

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
    let (selection, line_coding, tx_addr, rx_addr, timeout) =
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
                    duration,
                } => {
                    let line_coding = parse_line_coding(&encoding);
                    info!("Using line coding: {}", line_coding.name());
                    (0, line_coding, local, remote, duration)
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
                Commands::Router {
                    acoustic_ip,
                    acoustic_mac,
                    wifi_ip,
                    wifi_interface,
                    node3_ip,
                    node3_mac,
                    encoding,
                } => {
                    // Router Mode
                    let line_coding = parse_line_coding(&encoding);
                    run_router(
                        acoustic_ip,
                        acoustic_mac,
                        wifi_ip,
                        wifi_interface,
                        node3_ip,
                        node3_mac,
                        line_coding,
                    );
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

    let max_duration_samples = sample_rate * timeout as usize;

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
            timeout,
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
            timeout,
        );
    } else {
        unreachable!();
    }

    info!("Exiting gracefully...");
    if let Err(err) = active_client.deactivate() {
        error!("Error deactivating client: {}", err);
    }
}

fn run_sender(
    shared: recorder::AppShared,
    progress_manager: ProgressManager,
    sample_rate: u32,
    line_coding: LineCodingKind,
    sender_mac: mac::types::MacAddr,
    receiver_mac: mac::types::MacAddr,
    tx_timeout: u64,
) {
    info!("=== Sender Mode (with Stop-and-Wait) ===");
    info!("Using line coding: {}", line_coding.name());

    // Read input file
    let input_path = format!("INPUT{}to{}.bin", &sender_mac, &receiver_mac);
    let file_data = match fs::read(&input_path) {
        Ok(data) => {
            info!("Read {} bytes from {}", data.len(), input_path);
            data
        }
        Err(e) => {
            error!("Failed to read {}: {}", input_path, e);
            return;
        }
    };

    info!("=== Sender Mode (with Stop-and-Wait) ===");

    let progress_manager = Arc::new(Mutex::new(progress_manager));

    let _sender_progress = progress_manager
        .lock()
        .unwrap()
        .create_bar("sender", 0u64, templates::SENDER, "sender")
        .unwrap();

    let (tx, rx) = crossbeam_channel::unbounded::<Vec<u8>>();

    let sub_progress_manager = progress_manager.clone();
    let handle = thread::spawn(move || {
        let mut node = CsmaNode::new(
            shared,
            sub_progress_manager,
            sample_rate,
            line_coding,
            sender_mac,
            receiver_mac,
        );

        node.run_sender_loop(tx_timeout, rx);
    });

    // Split data into frames and push to queue
    for chunk in file_data.chunks(MAX_FRAME_DATA_SIZE) {
        progress_manager
            .lock()
            .unwrap()
            .increasae_length("sender", 1)
            .unwrap_or_else(|err| {
                debug!("Error while updating sender: {:?}", err)
            });
        tx.send(chunk.to_vec())
            .unwrap_or_else(|e| {
                error!("Failed to send data chunk to sender thread: {}", e);
            });
    }

    drop(tx); // Close the channel

    handle.join().unwrap();
}

fn run_receiver(
    shared: recorder::AppShared,
    progress_manager: ProgressManager,
    max_recording_duration_samples: u32,
    line_coding: LineCodingKind,
    receiver_addr: mac::types::MacAddr,
    sender_addr: mac::types::MacAddr,
    rx_duration: u64,
) {
    info!("=== Receiver Mode ===");
    info!("Using line coding: {}", line_coding.name());

    let (tx, rx) = crossbeam_channel::unbounded::<Vec<u8>>();

    let progress_manager = Arc::new(Mutex::new(progress_manager));

    let _progress_bar = progress_manager
        .lock()
        .unwrap()
        .create_bar(
            "recording",
            max_recording_duration_samples as u64,
            templates::RECEIVER,
            "receiver",
        )
        .unwrap();

    let sub_progress_manager = progress_manager.clone();
    let handle = thread::spawn(move || {
        let mut node = CsmaNode::new(
            shared,
            sub_progress_manager,
            SAMPLE_RATE,
            line_coding,
            receiver_addr,
            sender_addr,
        );

        node.run_receiver_loop(max_recording_duration_samples, rx_duration, tx);
    });

    let mut all_data = Vec::new();
    while let Ok(data) = rx.recv() {
        all_data.push(data);
    }

    handle.join().unwrap();

    let output_data: Vec<u8> = all_data
        .into_iter()
        .flatten()
        .collect();

    let output_path = format!("OUTPUT{}to{}.bin", &sender_addr, &receiver_addr);
    match fs::write(&output_path, &output_data) {
        Ok(_) => debug!("Written to {}", &output_path),
        Err(e) => error!("Failed to write {}: {}", output_path, e),
    }
}

fn run_ping(target: String, local_ip_str: String) {
    use crate::mac::ip_interface::IpInterface;
    use etherparse::{
        Icmpv4Header, Icmpv4Type, IpNumber, Ipv4Header as EtherIpv4Header,
    };
    use std::net::Ipv4Addr;
    use crate::net::arp::ArpTable;

    // Parse IP addresses
    let target_ip: Ipv4Addr = target
        .parse()
        .expect("Invalid target IP");
    let local_ip: Ipv4Addr = local_ip_str
        .parse()
        .expect("Invalid local IP");

    // Check static ARP table
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

    // A Modern taste to use a random identifier for ICMP
    let identifier = rand::random::<u16>();

    for seq in 0..PING_PACKET_COUNT {
        // Build ICMP Echo Request using etherparse
        // payload --> icmp header --> ip header
        let payload = vec![0u8; PING_PAYLOAD_SIZE];

        let icmp_header = Icmpv4Header::new(Icmpv4Type::EchoRequest(
            etherparse::IcmpEchoHeader {
                id: identifier,
                seq,
            },
        ));
        let icmp_bytes = {
            let mut buf = Vec::new();
            icmp_header
                .write(&mut buf)
                .expect("Failed to write ICMP header");
            buf.extend_from_slice(&payload);
            buf
        };

        let ip_header = EtherIpv4Header {
            dscp: Default::default(),
            ecn: Default::default(),
            total_len: (20 + icmp_bytes.len()) as u16,
            identification: seq,
            dont_fragment: false,
            more_fragments: false,
            fragment_offset: Default::default(),
            time_to_live: IP_TTL,
            protocol: IpNumber::ICMP,
            header_checksum: 0, // Will be calculated
            source: local_ip.octets(),
            destination: target_ip.octets(),
            options: Default::default(),
        };

        let ip_bytes = {
            let mut buf = Vec::new();
            ip_header
                .write(&mut buf)
                .expect("Failed to write IP header");
            buf.extend_from_slice(&icmp_bytes);
            buf
        };

        info!("Sending ICMP Echo Request seq={}...", seq);
        let start = std::time::Instant::now();

        // Send IP Packet
        if let Err(e) =
            interface.send_packet(&ip_bytes, dest_mac, FrameType::Data)
        {
            error!("Failed to send packet: {}", e);
            continue;
        }
        packets_sent += 1;
        // Wait for reply
        match interface.receive_packet(Some(std::time::Duration::from_millis(
            PING_TIMEOUT_MS,
        ))) {
            Ok(data) => {
                let rtt = start.elapsed();
                let rtt_ms = rtt.as_secs_f32() * 1000.0;

                // Parse IPv4
                let ip_slice =
                    match etherparse::Ipv4HeaderSlice::from_slice(&data) {
                        Ok(s) => s,
                        Err(e) => {
                            warn!("Failed to parse IP header: {:?}", e);
                            continue;
                        }
                    };

                // Ensure packet is large enough for claimed IP header length
                let ip_header_len = ip_slice.ihl() as usize * 4;
                if data.len() < ip_header_len {
                    warn!("Received packet too short for IP header length");
                    continue;
                }

                // Parse ICMP
                let icmp_data = &data[ip_header_len..];
                let icmp_slice =
                    match etherparse::Icmpv4Slice::from_slice(icmp_data) {
                        Ok(s) => s,
                        Err(e) => {
                            warn!("Failed to parse ICMP: {:?}", e);
                            continue;
                        }
                    };

                // Only handle Echo Replies that match our sequence
                if let Icmpv4Type::EchoReply(echo) =
                    icmp_slice.header().icmp_type
                {
                    if echo.seq == seq {
                        packets_received += 1;
                        rtt_times.push(rtt_ms);

                        info!(
                            "Reply from {}: bytes={} time={:.2}ms TTL={}",
                            target_ip,
                            data.len(),
                            rtt_ms,
                            ip_slice.ttl()
                        );
                    }
                }
            }
            Err(e) => {
                warn!("Request timed out: {}", e);
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(PING_INTERVAL_MS));
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
    use etherparse::{
        Icmpv4Header, Icmpv4Type, IpNumber, Ipv4Header as EtherIpv4Header,
    };
    use std::net::Ipv4Addr;
    use crate::net::arp::ArpTable;

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

    // Setup IP Interface
    let mut interface = IpInterface::new(
        shared.clone(),
        sample_rate,
        LineCodingKind::FourBFiveB,
        local_mac,
    );

    // Listen for packets
    loop {
        // Get a packet from interface
        let data = match interface.receive_packet(None) {
            Ok(data) => data,
            Err(e) => {
                warn!("Failed to receive packet: {:?}", e);
                continue;
            }
        };

        // Parse IPv4
        let ip_slice = match etherparse::Ipv4HeaderSlice::from_slice(&data) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to parse IP header: {:?}", e);
                continue;
            }
        };

        // Check if it's for us
        if ip_slice.destination() != local_ip.octets() {
            continue; // Continue if not
        }

        // Ensure packet is large enough for claimed IP header length
        let ip_header_len = ip_slice.ihl() as usize * 4;
        if data.len() < ip_header_len {
            warn!("Received packet too short for IP header length");
            continue;
        }

        // Parse ICMP
        let icmp_data = &data[ip_header_len..];
        let icmp_slice = match etherparse::Icmpv4Slice::from_slice(icmp_data) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to parse ICMP: {:?}", e);
                continue;
            }
        };

        // Only handle Echo Requests
        let echo = match icmp_slice.header().icmp_type {
            Icmpv4Type::EchoRequest(echo) => echo,
            _ => continue,
        };

        info!(
            "Received ICMP Echo Request from {:?}",
            ip_slice.source()
        );

        // Build Echo Reply using etherparse
        let payload = icmp_slice.payload().to_vec();
        let reply_icmp_header = Icmpv4Header::new(Icmpv4Type::EchoReply(
            etherparse::IcmpEchoHeader {
                id: echo.id,
                seq: echo.seq,
            },
        ));
        let reply_icmp_bytes = {
            let mut buf = Vec::new();
            reply_icmp_header
                .write(&mut buf)
                .expect("Failed to write ICMP header");
            buf.extend_from_slice(&payload);
            buf
        };

        // Build IPv4 reply header
        let reply_ip_header = EtherIpv4Header {
            dscp: Default::default(),
            ecn: Default::default(),
            total_len: (20 + reply_icmp_bytes.len()) as u16,
            identification: 0,
            dont_fragment: false,
            more_fragments: false,
            fragment_offset: Default::default(),
            time_to_live: IP_TTL,
            protocol: IpNumber::ICMP,
            header_checksum: 0,
            source: local_ip.octets(),
            destination: ip_slice.source(),
            options: Default::default(),
        };

        let reply_bytes = {
            let mut buf = Vec::new();
            reply_ip_header
                .write(&mut buf)
                .expect("Failed to write IP header");
            buf.extend_from_slice(&reply_icmp_bytes);
            buf
        };

        // Find dest MAC
        let src_ip = Ipv4Addr::from(ip_slice.source());
        let dest_mac = match arp.get_mac(&src_ip) {
            Some(m) => m,
            None => {
                // warn!("Unknown source IP {}, cannot reply", src_ip);
                // continue;
                1
            }
        };

        info!("Sending Echo Reply to {} ({})", src_ip, dest_mac);

        if let Err(e) =
            interface.send_packet(&reply_bytes, dest_mac, FrameType::Ack)
        {
            error!("Failed to send reply: {}", e);
        }
    }
}

fn run_router(
    acoustic_ip_str: String,
    acoustic_mac: u8,
    wifi_ip_str: String,
    wifi_interface: String,
    node3_ip_str: String,
    node3_mac_str: Option<String>,
    line_coding: LineCodingKind,
) {
    use crate::net::router::{Router, RouterConfig};
    use std::net::Ipv4Addr;

    // === Router Preparation ===

    info!("Starting Router Preparation...");

    // Parse IP addresses
    let acoustic_ip: Ipv4Addr = acoustic_ip_str
        .parse()
        .expect("Invalid acoustic IP");
    let wifi_ip: Ipv4Addr = wifi_ip_str
        .parse()
        .expect("Invalid WiFi IP");
    let node3_ip: Ipv4Addr = node3_ip_str
        .parse()
        .expect("Invalid NODE3 IP");

    // Parse NODE3 MAC if provided
    let node3_mac: Option<[u8; 6]> = node3_mac_str.map(|s| {
        let parts: Vec<u8> = s
            .split(':')
            .map(|p| u8::from_str_radix(p, 16).expect("Invalid MAC format"))
            .collect();
        if parts.len() != 6 {
            panic!("MAC address must have 6 octets");
        }
        let mut mac = [0u8; 6];
        mac.copy_from_slice(&parts);
        mac
    });

    info!("Starting Router Mode...");
    info!("Acoustic interface: {} (MAC {})", acoustic_ip, acoustic_mac);
    info!("WiFi interface: {} on {}", wifi_ip, wifi_interface);
    info!("NODE3: {}", node3_ip);

    // Determine acoustic and WiFi network from IPs
    // Assume /24 networks
    let acoustic_network: Ipv4Addr = {
        let octets = acoustic_ip.octets();
        Ipv4Addr::new(octets[0], octets[1], octets[2], 0)
    };
    let wifi_network: Ipv4Addr = {
        let octets = wifi_ip.octets();
        Ipv4Addr::new(octets[0], octets[1], octets[2], 0)
    };
    let netmask: Ipv4Addr = "255.255.255.0".parse().unwrap();

    // Setup JACK
    let (client, _status) = jack::Client::new(
        &format!("{}_router_{}", JACK_CLIENT_NAME, rand::random::<u16>()),
        jack::ClientOptions::NO_START_SERVER,
    )
    .unwrap();

    let sample_rate = client.sample_rate() as u32;
    let shared = recorder::AppShared::new(sample_rate as usize * 60); // 60s buffer
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
            sample_rate as usize * 60,
        ),
    );
    let active_client = client
        .activate_async((), process)
        .unwrap();
    connect_system_ports(active_client.as_client(), &in_name, &out_name);

    // Get WiFi MAC address (we'll use a dummy one for now, could be detected)
    let wifi_mac = [0x00, 0x00, 0x00, 0x00, 0x00, acoustic_mac];

    // Create router config
    let config = RouterConfig {
        acoustic_ip,
        acoustic_mac,
        wifi_ip,
        wifi_mac,
        wifi_interface,
        acoustic_network,
        acoustic_netmask: netmask,
        wifi_network,
        wifi_netmask: netmask,
    };

    let mut router = Router::new(config);

    // Add NODE3 ARP entry if MAC provided
    if let Some(mac) = node3_mac {
        router.add_wifi_arp_entry(node3_ip, mac);
        info!(
            "Added static ARP entry: {} -> {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            node3_ip, mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );
    } else {
        warn!("No NODE3 MAC provided. Router will need to learn it or packets to NODE3 will fail.");
        warn!("Use --node3-mac aa:bb:cc:dd:ee:ff to specify NODE3's MAC address");
    }

    // Run router
    if let Err(e) = router.run(shared, sample_rate, line_coding) {
        error!("Router error: {}", e);
    }

    // Cleanup
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
