use crate::audio::recorder;
use crate::net::router::InterfaceType;
use crate::phy::{FrameType, LineCodingKind};
use crate::utils::consts::*;
use tracing::{debug, error, info, warn};

use crate::device::jack::connect_system_ports;

pub fn run_ping(
    target: String,
    local_ip_str: String,
    gateway: Option<String>,
    payload_size: usize,
) {
    use crate::mac::acoustic_interface::AcousticInterface;
    use crate::net::arp::ArpTable;
    use etherparse::{
        Icmpv4Header, Icmpv4Type, IpNumber, Ipv4Header as EtherIpv4Header,
    };
    use std::net::Ipv4Addr;

    // Parse IP addresses
    let target_ip: Ipv4Addr = target
        .parse()
        .expect("Invalid target IP");
    let local_ip: Ipv4Addr = local_ip_str
        .parse()
        .expect("Invalid local IP");

    // Check static ARP table
    let arp = ArpTable::new();
    let dest_mac = if let Some(mac) = arp.get_mac(&target_ip) {
        mac
    } else if let Some(gateway_str) = gateway {
        let gateway_ip: Ipv4Addr = gateway_str
            .parse()
            .expect("Invalid gateway IP");
        arp.get_mac(&gateway_ip)
            .expect("Gateway IP not in ARP table")
    } else {
        panic!("Target IP not in ARP table and no gateway specified");
    };

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

    let mut interface = AcousticInterface::new(
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
        let payload = vec![0u8; payload_size];

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

pub fn run_ip_host(local_ip_str: String) {
    use crate::mac::acoustic_interface::AcousticInterface;
    use crate::net::arp::ArpTable;
    use etherparse::{
        Icmpv4Header, Icmpv4Type, IpNumber, Ipv4Header as EtherIpv4Header,
    };
    use std::net::Ipv4Addr;

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
    let mut interface = AcousticInterface::new(
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

        info!("Received ICMP Echo Request from {:?}", ip_slice.source());

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

pub fn run_router(
    acoustic_ip_str: String,
    acoustic_mac: u8,
    wifi_ip_str: String,
    wifi_interface: String,
    wifi_mac_str: Option<String>,
    node3_ip_str: String,
    node3_mac_str: Option<String>,
    eth_ip: String,
    eth_netmask_str: String,
    eth_mac_str: Option<String>,
    gateway_ip_str: String,
    gateway_mac_str: Option<String>,
    gateway_interface: String,
    tun_name: String,
    tun_ip_str: String,
    tun_netmask_str: String,
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
    let gateway_ip: Ipv4Addr = gateway_ip_str
        .parse()
        .expect("Invalid Default Gateway IP");
    let eth_ip: Ipv4Addr = eth_ip
        .parse()
        .expect("Invalid Ethernet IP");
    let eth_netmask: Ipv4Addr = eth_netmask_str
        .parse()
        .expect("Invalid Ethernet Netmask");
    let tun_ip: Ipv4Addr = tun_ip_str
        .parse()
        .expect("Invalid TUN IP");
    let tun_netmask: Ipv4Addr = tun_netmask_str
        .parse()
        .expect("Invalid TUN Netmask");

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

    // Parse Gateway MAC if provided
    let gateway_mac: Option<[u8; 6]> = gateway_mac_str.map(|s| {
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

    // Parse Ethernet MAC
    let eth_mac: Option<[u8; 6]> = eth_mac_str.map(|s| {
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

    // Parse WiFi MAC
    let wifi_mac: Option<[u8; 6]> = wifi_mac_str.map(|s| {
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

    if let Some(mac) = gateway_mac {
        info!(
            "Gateway: {} on {} (MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x})",
            gateway_ip,
            gateway_interface,
            mac[0],
            mac[1],
            mac[2],
            mac[3],
            mac[4],
            mac[5]
        );
    } else {
        info!(
            "Gateway: {} on {}(MAC not provided)",
            gateway_ip, gateway_interface
        );
    }

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
    let netmask: Ipv4Addr = "255.255.255.0"
        .parse()
        .unwrap();

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

    // Create router config
    let config = RouterConfig {
        acoustic_ip,
        acoustic_mac,
        wifi_ip,
        wifi_mac: wifi_mac.unwrap_or_else(|| {
            warn!("No MAC for WiFI Provided!");
            [0u8; 6]
        }),
        wifi_interface,
        acoustic_network,
        acoustic_netmask: netmask,
        wifi_network,
        wifi_netmask: netmask,
        gateway_ip,
        gateway_mac,
        gateway_interface,
        eth_ip,
        eth_netmask,
        eth_mac: eth_mac.unwrap_or_else(|| {
            warn!("No MAC for Ethernet Provided!");
            [0u8; 6]
        }),
        tun_name,
        tun_ip,
        tun_netmask,
        node3_ip,
        node1_ip: "192.168.1.2".parse().unwrap(),
    };

    let mut router = Router::new(config);

    // Add NODE3 ARP entry if MAC provided
    if let Some(mac) = node3_mac {
        router.add_arp_entry(node3_ip, mac, InterfaceType::WiFi);
        info!(
            "Added static ARP entry for Node3, WiFi: {} -> {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            node3_ip, mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );
    } else {
        warn!(
            "No NODE3 MAC provided. Router will need to learn it or packets to NODE3 will fail."
        );
        warn!(
            "Use --node3-mac aa:bb:cc:dd:ee:ff to specify NODE3's MAC address"
        );
    }

    if let Some(mac) = gateway_mac {
        router.add_arp_entry(gateway_ip, mac, InterfaceType::Ethernet);
        info!(
            "Added static ARP entry for Gateway: {} -> {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            gateway_ip, mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );
    } else {
        warn!("No Gateway Provided. NAT will be failed.");
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
