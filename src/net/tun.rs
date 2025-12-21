use std::io::{Read, Write};
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crossbeam_channel;
use etherparse::Ipv4HeaderSlice;
use tracing::{debug, error, info, trace, warn};
use tun::Configuration;

use crate::audio::recorder::{self};
use crate::device::jack::connect_system_ports;
use crate::mac::acoustic_interface::AcousticInterface;
use crate::phy::{FrameType, LineCodingKind};
use crate::utils::consts::*;

pub fn run_tun(
    ip_str: String,
    netmask_str: String,
    tun_name: String,
    gateway_str: Option<String>,
    line_coding: LineCodingKind,
) {
    // Parse IPs
    let ip: Ipv4Addr = ip_str
        .parse()
        .expect("Invalid IP address");
    let netmask: Ipv4Addr = netmask_str
        .parse()
        .expect("Invalid netmask");
    let gateway: Option<Ipv4Addr> = gateway_str.map(|s| {
        s.parse()
            .expect("Invalid gateway IP")
    });

    info!("Starting TUN Adapter...");
    info!("  IP: {}", ip);
    info!("  Netmask: {}", netmask);
    if let Some(gw) = gateway {
        info!("  Gateway: {}", gw);
    }
    info!("  Device: {}", tun_name);
    info!("  MTU: {}", MAX_FRAME_DATA_SIZE);

    // Setup TUN
    let mut config = Configuration::default();
    config
        .address(ip)
        .netmask(netmask)
        .mtu(MAX_FRAME_DATA_SIZE as u16)
        .up();

    #[cfg(target_os = "linux")]
    config.tun_name(&tun_name);

    let dev = tun::create(&config).expect("Failed to create TUN device");
    let (mut tun_reader, mut tun_writer) = dev.split();

    // Setup JACK
    let (client, _status) = jack::Client::new(
        &format!("{}_tun_{}", JACK_CLIENT_NAME, rand::random::<u16>()),
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

    // Setup Acoustic Interface
    let local_mac = ip.octets()[3];
    info!("  Local MAC derived from IP: {}", local_mac);

    let mut interface = AcousticInterface::new(
        shared.clone(),
        sample_rate,
        line_coding,
        local_mac,
    );

    // Channels for inter-thread communication
    let (to_acoustic_tx, to_acoustic_rx) =
        crossbeam_channel::unbounded::<(Vec<u8>, u8)>();
    let (to_tun_tx, to_tun_rx) = crossbeam_channel::unbounded::<Vec<u8>>();

    let running = Arc::new(AtomicBool::new(true));

    // Handle Ctrl+C
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .ok(); // Ignore error if handler already set

    // 1. TUN Reader Thread (TUN -> Acoustic Channel)
    let r_reader = running.clone();
    let local_ip = ip;
    let local_netmask = netmask;
    let local_gateway = gateway;

    thread::spawn(move || {
        let mut buf = [0u8; 1500];
        while r_reader.load(Ordering::SeqCst) {
            match tun_reader.read(&mut buf) {
                Ok(len) => {
                    if len > 0 {
                        let packet = &buf[..len];
                        if let Ok(ip_header) =
                            Ipv4HeaderSlice::from_slice(packet)
                        {
                            let dest_ip =
                                Ipv4Addr::from(ip_header.destination());

                            // Filter multicast and broadcast
                            if dest_ip.is_multicast()
                                || dest_ip == Ipv4Addr::new(255, 255, 255, 255)
                            {
                                trace!(
                                    "Ignoring multicast/broadcast packet to {}",
                                    dest_ip
                                );
                                continue;
                            }

                            // Routing logic:
                            // 1. If dest_ip is in local subnet, target_mac = dest_ip.last_octet
                            // 2. If dest_ip is NOT in local subnet and gateway exists, target_mac = gateway.last_octet
                            // 3. Otherwise, use dest_ip.last_octet (fallback)

                            let is_local = {
                                let ip_octets = dest_ip.octets();
                                let net_octets = local_ip.octets();
                                let mask_octets = local_netmask.octets();
                                (0..4).all(|i| {
                                    (ip_octets[i] & mask_octets[i])
                                        == (net_octets[i] & mask_octets[i])
                                })
                            };

                            let target_mac = if is_local {
                                dest_ip.octets()[3]
                            } else if let Some(gw) = local_gateway {
                                debug!(
                                    "Routing packet for {} via gateway {}",
                                    dest_ip, gw
                                );
                                gw.octets()[3]
                            } else {
                                dest_ip.octets()[3]
                            };

                            debug!(
                                "TUN -> Channel: {} bytes to {} (MAC {})",
                                len, dest_ip, target_mac
                            );
                            if let Err(e) = to_acoustic_tx
                                .send((packet.to_vec(), target_mac))
                            {
                                error!(
                                    "Failed to send to acoustic channel: {}",
                                    e
                                );
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::WouldBlock {
                        warn!("TUN read error: {}", e);
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        }
        debug!("TUN Reader thread stopping");
    });

    // 2. TUN Writer Thread (TUN Channel -> TUN Device)
    let r_writer = running.clone();
    thread::spawn(move || {
        while r_writer.load(Ordering::SeqCst) {
            match to_tun_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(packet) => {
                    debug!("Channel -> TUN: {} bytes", packet.len());
                    if let Err(e) = tun_writer.write_all(&packet) {
                        error!("Failed to write to TUN: {}", e);
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }
        debug!("TUN Writer thread stopping");
    });

    info!("TUN Adapter running. Press Ctrl+C to stop.");

    while running.load(Ordering::SeqCst) {
        // A. Try receive from air
        match interface.receive_packet(Some(Duration::from_millis(10))) {
            Ok(mut packet) => {
                // FIXME: Patch: Ensure IPv4 checksum is correct
                if let Ok((mut header, payload)) = etherparse::Ipv4Header::from_slice(&packet) {
                     header.header_checksum = header.calc_header_checksum();
                     
                     let mut new_packet = Vec::with_capacity(packet.len());
                     if header.write(&mut new_packet).is_ok() {
                         new_packet.extend_from_slice(payload);
                         packet = new_packet;
                     }
                }

                debug!("Acoustic -> Channel: {} bytes", packet.len());
                if let Err(e) = to_tun_tx.send(packet) {
                    error!("Failed to send to TUN channel: {}", e);
                    break;
                }
            }
            Err(e) => {
                if e != "Timeout" {
                    trace!("Acoustic receive error: {}", e);
                }
            }
        }

        // B. Try send to air
        if let Ok((packet, target_mac)) = to_acoustic_rx.try_recv() {
            info!(
                "Channel -> Acoustic: {} bytes to MAC {}",
                packet.len(),
                target_mac
            );
            if let Err(e) =
                interface.send_packet(&packet, target_mac, FrameType::Data)
            {
                error!("Failed to send acoustic packet: {}", e);
            }
        }
    }

    info!("Stopping TUN Adapter...");
    let _ = active_client.deactivate();
}
