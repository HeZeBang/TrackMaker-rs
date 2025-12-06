//! Router module for forwarding IP packets between interfaces
//!
//! This module implements a simple static router that forwards IP packets
//! between an acoustic interface (to NODE1) and a WiFi interface (to NODE3).

use etherparse::{Ipv4HeaderSlice, PacketBuilder};
use pcap::{Active, Capture, Device, Linktype};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc::Sender};
use std::thread;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::audio::recorder::AppShared;
use crate::mac::acoustic_interface::AcousticInterface;
use crate::net::icmp::{self, IcmpPacket, IcmpType};
use crate::net::nat::NatTable;
use crate::phy::{FrameType, LineCodingKind};

/// Network interface type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterfaceType {
    /// Acoustic interface (to NODE1)
    Acoustic,
    /// WiFi interface (to NODE3)
    WiFi,
    /// Other
    Ethernet,
}

/// A directly connected network
#[derive(Debug, Clone)]
pub struct DirectNetwork {
    /// Network address (e.g., 192.168.1.0)
    pub network: Ipv4Addr,
    /// Subnet mask (e.g., 255.255.255.0)
    pub mask: Ipv4Addr,
    /// Interface to use for this network
    pub interface: InterfaceType,
}

impl DirectNetwork {
    pub fn new(
        network: Ipv4Addr,
        mask: Ipv4Addr,
        interface: InterfaceType,
    ) -> Self {
        Self {
            network,
            mask,
            interface,
        }
    }

    /// Check if an IP address belongs to this network
    pub fn contains(&self, ip: &Ipv4Addr) -> bool {
        let net_octets = self.network.octets();
        let mask_octets = self.mask.octets();
        let ip_octets = ip.octets();

        for i in 0..4 {
            if (net_octets[i] & mask_octets[i])
                != (ip_octets[i] & mask_octets[i])
            {
                return false;
            }
        }
        true
    }
}

/// Routing table entry
#[derive(Debug, Clone)]
pub struct RouteEntry {
    /// Destination network
    pub network: DirectNetwork,
    /// Next hop (None for directly connected)
    pub next_hop: Option<Ipv4Addr>,
}

/// Static routing table
#[derive(Clone)]
pub struct RoutingTable {
    routes: Vec<RouteEntry>,
}

impl RoutingTable {
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    /// Add a directly connected network
    pub fn add_direct_network(
        &mut self,
        network: Ipv4Addr,
        mask: Ipv4Addr,
        interface: InterfaceType,
    ) {
        self.routes.push(RouteEntry {
            network: DirectNetwork::new(network, mask, interface),
            next_hop: None,
        });
    }

    pub fn add_network(
        &mut self,
        network: Ipv4Addr,
        mask: Ipv4Addr,
        interface: InterfaceType,
        next_hop: Ipv4Addr
    ) {
        self.routes.push(RouteEntry {
            network: DirectNetwork::new(network, mask, interface),
            next_hop: Some(next_hop),
        });
    }

    /// Lookup the interface for a destination IP
    pub fn lookup(&self, dest_ip: &Ipv4Addr) -> Option<(Option<Ipv4Addr>, InterfaceType)> {
        for route in &self.routes {
            if route
                .network
                .contains(dest_ip)
            {
                return Some((route.next_hop, route.network.interface));
            }
        }
        None
    }
}

/// ARP table for Network interface (maps IP to MAC address)
#[derive(Clone)]
pub struct ArpTable {
    table: HashMap<InterfaceType, HashMap<Ipv4Addr, [u8; 6]>>,
}

impl ArpTable {
    pub fn new() -> Self {
        let mut ac_table = HashMap::new();
        ac_table.insert("192.168.1.1".parse().unwrap(), [0, 0, 0, 0, 0, 1]);
        ac_table.insert("192.168.1.2".parse().unwrap(), [0, 0, 0, 0, 0, 2]);
        ac_table.insert("192.168.1.3".parse().unwrap(), [0, 0, 0, 0, 0, 3]);

        Self {
            table: HashMap::from([(InterfaceType::Acoustic, ac_table)]),
        }
    }

    /// Add a static ARP entry
    pub fn add_entry(&mut self, ip: Ipv4Addr, mac: [u8; 6], iface: InterfaceType) {
        self.table
            .entry(iface)
            .or_insert_with(HashMap::new)
            .insert(ip, mac);
    }

    /// Get MAC address for an IP
    pub fn get_mac(&self, ip: &Ipv4Addr, iface: InterfaceType) -> Option<[u8; 6]> {
        // Borrow the interface key for lookup, then copy the MAC out of the inner map
        self.table.get(&iface).and_then(|m| m.get(ip).copied())
    }

    /// Update or add an ARP entry (for learning)
    pub fn update(&mut self, ip: Ipv4Addr, mac: [u8; 6], interface: InterfaceType) {
        self.table
            .entry(interface)
            .or_insert_with(HashMap::new)
            .insert(ip, mac);
    }
}

/// Router configuration
#[derive(Clone)]
pub struct RouterConfig {
    /// Local IP on acoustic side (connected to NODE1)
    pub acoustic_ip: Ipv4Addr,
    /// Local MAC on acoustic side
    pub acoustic_mac: u8,
    /// Local IP on WiFi side (connected to NODE3)
    pub wifi_ip: Ipv4Addr,
    /// Local MAC on WiFi side (Ethernet MAC)
    pub wifi_mac: [u8; 6],
    /// WiFi interface name (e.g., "wlan0")
    pub wifi_interface: String,
    /// Acoustic network (e.g., 192.168.1.0/24)
    pub acoustic_network: Ipv4Addr,
    pub acoustic_netmask: Ipv4Addr,
    /// WiFi network (e.g., 192.168.2.0/24)
    pub wifi_network: Ipv4Addr,
    pub wifi_netmask: Ipv4Addr,
    /// Ethernet IP
    pub eth_ip: Ipv4Addr,
    /// Ethernet Mac
    pub eth_mac: [u8; 6],
    /// Ethernet Gateway IP (e.g., 192.168.2.254)
    pub gateway_ip: Ipv4Addr,
    /// Ethernet Gateway MAC
    pub gateway_mac: Option<[u8; 6]>,
    /// Ethernet Gateway Interface
    pub gateway_interface: String,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            acoustic_ip: "192.168.1.1".parse().unwrap(),
            acoustic_mac: 2,
            wifi_ip: "192.168.2.1".parse().unwrap(),
            wifi_mac: [0x00, 0x00, 0x00, 0x00, 0x00, 0x02],
            wifi_interface: "wlan0".to_string(),
            acoustic_network: "192.168.1.0".parse().unwrap(),
            acoustic_netmask: "255.255.255.0"
                .parse()
                .unwrap(),
            wifi_network: "192.168.2.0".parse().unwrap(),
            wifi_netmask: "255.255.255.0"
                .parse()
                .unwrap(),
            gateway_ip: "192.168.2.254"
                .parse()
                .unwrap(),
            gateway_mac: None,
            gateway_interface: "eth0".to_string(),
            eth_ip: "10.20.0.1".parse().unwrap(),
            eth_mac: [0x9c, 0x29, 0x76, 0x0c, 0x49, 0x00],
        }
    }
}

/// Simple IP Router
#[derive(Clone)]
pub struct Router {
    config: RouterConfig,
    routing_table: RoutingTable,
    arp_table: ArpTable,
    nat_table: NatTable,
    running: Arc<Mutex<AtomicBool>>,
}


pub enum PacketState {
    /// Read raw data from any interface
    Ingress {
        iface: InterfaceType,
        raw_data: Vec<u8>,
    },
    /// 1. Parsed to IP Packet, ready to search Routing Table
    Routing {
        // L3
        src_ip: Ipv4Addr,
        dst_ip: Ipv4Addr,
        packet: Vec<u8>,
    },
    /// 2. Local delivery to router
    LocalProcess {
        src_ip: Ipv4Addr,
        packet: Vec<u8>,
    },
    /// Pack a frame, ready to send
    Send {
        out_interface: InterfaceType,
        payload: Vec<u8>,
        src_mac: [u8; 6],
        dst_mac: [u8; 6],
    },
    /// Dropped packet
    Dropped {
        reason: String,
    },
    
}

impl Router {
    pub fn new(config: RouterConfig) -> Self {
        let mut routing_table = RoutingTable::new();

        // Add directly connected networks
        routing_table.add_direct_network(
            config.acoustic_network,
            config.acoustic_netmask,
            InterfaceType::Acoustic,
        );
        routing_table.add_direct_network(
            config.wifi_network,
            config.wifi_netmask,
            InterfaceType::WiFi,
        );

        Self {
            config,
            routing_table,
            arp_table: ArpTable::new(),
            nat_table: NatTable::new(),
            running: Arc::new(Mutex::new(AtomicBool::new(false))),
        }
    }

    /// Add a static ARP entry for Other(Gateway)
    pub fn add_arp_entry(&mut self, ip: Ipv4Addr, mac: [u8; 6], interface: InterfaceType) {
        self.arp_table
            .add_entry(ip, mac, interface);
    }

    /// Build an Ethernet frame for WiFi transmission
    fn build_ethernet_frame(
        &self,
        src_mac: [u8; 6],
        dest_mac: [u8; 6],
        ip_packet: &[u8],
    ) -> Vec<u8> {
        let mut frame = Vec::with_capacity(14 + ip_packet.len());

        // Ethernet header (14 bytes)
        frame.extend_from_slice(&dest_mac); // Destination MAC
        frame.extend_from_slice(&src_mac); // Source MAC
        frame.extend_from_slice(&[0x08, 0x00]); // EtherType: IPv4

        // IP packet payload
        frame.extend_from_slice(ip_packet);

        frame
    }

    /// Parse Ethernet frame and extract IP packet
    fn parse_ethernet_frame(frame: &[u8]) -> Option<(Vec<u8>, [u8; 6], [u8; 6])> {
        // TODO: use etherparse
        let eth = etherparse::Ethernet2HeaderSlice::from_slice(frame).ok()?;
        if frame.len() < 14 {
            return None;
        }

        let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
        if ethertype != 0x0800 {
            // Not IPv4
            return None;
        }

        Some((frame[14..].to_vec(), eth.source(), eth.destination()))
    }

    /// Decrement TTL and recalculate checksum
    fn decrement_ttl(ip_packet: &mut [u8]) -> Result<(), &'static str> {
        if ip_packet.len() < 20 {
            return Err("IP packet too short");
        }

        let ttl = ip_packet[8];
        if ttl <= 1 {
            return Err("TTL expired");
        }

        // Decrement TTL
        ip_packet[8] = ttl - 1;

        // Recalculate header checksum
        // First, zero out the checksum field
        ip_packet[10] = 0;
        ip_packet[11] = 0;

        // Calculate IHL (header length in 32-bit words)
        let ihl = (ip_packet[0] & 0x0F) as usize;
        let header_len = ihl * 4;

        // Calculate checksum
        let mut sum: u32 = 0;
        for i in (0..header_len).step_by(2) {
            let word = u16::from_be_bytes([ip_packet[i], ip_packet[i + 1]]);
            sum = sum.wrapping_add(word as u32);
        }

        // Fold 32-bit sum to 16 bits
        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }

        let checksum = !(sum as u16);
        ip_packet[10] = (checksum >> 8) as u8;
        ip_packet[11] = (checksum & 0xFF) as u8;

        Ok(())
    }

    /// Recalculate IP header checksum
    fn recalculate_ip_checksum(ip_packet: &mut [u8]) {
        // Zero out checksum
        ip_packet[10] = 0;
        ip_packet[11] = 0;

        let ihl = (ip_packet[0] & 0x0F) as usize;
        let header_len = ihl * 4;

        let mut sum: u32 = 0;
        for i in (0..header_len).step_by(2) {
            let word = u16::from_be_bytes([ip_packet[i], ip_packet[i + 1]]);
            sum = sum.wrapping_add(word as u32);
        }

        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }

        let checksum = !(sum as u16);
        ip_packet[10] = (checksum >> 8) as u8;
        ip_packet[11] = (checksum & 0xFF) as u8;
        debug!(
            "Modified checksum to {:0x} {:02x}",
            ip_packet[10], ip_packet[11]
        );
    }

    /// Prepare a packet for Acoustic interface
    /// Returns (ip_packet, dest_mac)
    fn prepare_acoustic_packet(
        &self,
        mut ip_packet: Vec<u8>,
        dest_ip: Ipv4Addr,
    ) -> Result<(Vec<u8>, u8), String> {
        // Decrement TTL
        Self::decrement_ttl(&mut ip_packet).map_err(|e| e.to_string())?;

        // Get destination MAC from acoustic ARP table
        let dest_mac = self.arp_table
            .get_mac(&dest_ip, InterfaceType::Acoustic)
            .ok_or_else(|| format!("No ARP entry for {}", dest_ip))?;

        info!(
            "Forwarding packet to acoustic interface: {} -> MAC {}",
            dest_ip, dest_mac[5]
        );

        Ok((ip_packet, dest_mac[5]))
    }

    /// Handle inbound NAT and forward to acoustic interface
    fn handle_inbound_nat(
        &self,
        to_acoustic: &crossbeam_channel::Sender<(Vec<u8>, u8)>,
        mut ip_packet: Vec<u8>,
    ) {
        // Check if it's ICMP
        let ip_header = match Ipv4HeaderSlice::from_slice(&ip_packet) {
            Ok(h) => h,
            Err(_) => return,
        };

        if ip_header.protocol() == etherparse::IpNumber::ICMP {
            // ICMP
            let ihl = ip_header.slice().len();
            if let Ok(icmp_packet) = IcmpPacket::from_bytes(&ip_packet[ihl..]) {
                if icmp_packet.icmp_type == IcmpType::EchoReply {
                    // Lookup in NAT table
                    if let Some(original_ip) = self
                        .nat_table
                        .translate_echo_reply(icmp_packet.identifier)
                    {
                        debug!(
                            "NAT: Translating Echo Reply ID {} to {}",
                            icmp_packet.identifier, original_ip
                        );

                        // Modify Destination IP
                        let original_ip_octets = original_ip.octets();
                        ip_packet[16] = original_ip_octets[0];
                        ip_packet[17] = original_ip_octets[1];
                        ip_packet[18] = original_ip_octets[2];
                        ip_packet[19] = original_ip_octets[3];

                        // Recalculate IP Checksum
                        Self::recalculate_ip_checksum(&mut ip_packet);

                        // Forward to Acoustic Interface
                        match self
                            .prepare_acoustic_packet(ip_packet, original_ip)
                        {
                            Ok(msg) => {
                                if let Err(e) = to_acoustic.send(msg) {
                                    warn!(
                                        "Failed to send NAT reply to Acoustic thread: {}",
                                        e
                                    );
                                }
                            }
                            Err(e) => {
                                warn!("Failed to prepare NAT reply: {}", e);
                                self.stop();
                            }
                        }
                        return;
                    }
                }
            }
        }

        // If not handled by NAT, ignore (since it was addressed to us but not NATed)
        debug!("Packet for router itself, ignoring (let host stack handle)");
    }

    /// Check if packet is for us (router itself)
    fn is_for_us(&self, dest_ip: &Ipv4Addr) -> bool {
        *dest_ip == self.config.acoustic_ip || *dest_ip == self.config.wifi_ip || *dest_ip == self.config.eth_ip
    }

    /// Run the router
    pub fn run(
        &mut self,
        shared: AppShared,
        sample_rate: u32,
        line_coding: LineCodingKind,
    ) -> Result<(), String> {
        self.running.lock().unwrap()
            .store(true, Ordering::SeqCst);

        info!("Starting router...");
        info!(
            "Acoustic interface: {} (MAC {})",
            self.config.acoustic_ip, self.config.acoustic_mac
        );
        info!(
            "WiFi interface: {} on {}",
            self.config.wifi_ip, self.config.wifi_interface
        );

        // Open WiFi device
        let wifi_device = crate::net::pcap_utils::get_device_by_name(
            &self.config.wifi_interface,
        )
        .map_err(|e| format!("Failed to get WiFi device: {}", e))?;

        // Create acoustic interface
        let mut acoustic_interface = AcousticInterface::new(
            shared.clone(),
            sample_rate,
            line_coding,
            self.config.acoustic_mac,
        );

        // Open Ethernet device
        let eth_device =  if self.config.gateway_interface != self.config.wifi_interface {
            Some(crate::net::pcap_utils::get_device_by_name(
                &self.config.gateway_interface,
            )
            .unwrap_or_else(|err| {
                error!("Failed to open Ethernet device: {}, using default device", err);
                crate::net::pcap_utils::get_default_device().unwrap()
            }))
         } else {
            None
        };

        info!("Router is running. Press Ctrl+C to stop.");

        // Channels for inter-thread communication
        let (to_acoustic_tx, to_acoustic_rx) =
            crossbeam_channel::unbounded::<(Vec<u8>, u8)>();
        let (to_wifi_tx, to_wifi_rx) = crossbeam_channel::unbounded::<Vec<u8>>();
        let (to_eth_tx, to_eth_rx) = crossbeam_channel::unbounded::<Vec<u8>>();
        let (to_router_tx, to_router_rx) =
            crossbeam_channel::unbounded::<(Vec<u8>, InterfaceType)>();

        // WiFi Hotspot RX
        
        // Spawn WiFi Thread
        let router_wifi = self.clone();
        let running = self.running.clone();
        let wifi_to_router = to_router_tx.clone();

        let mut wifi_capture = crate::net::pcap_utils::open_capture(wifi_device.clone())
            .map_err(|e| format!("Failed to open WiFi capture: {}", e))?;

        // Set filter to only capture IP packets
        wifi_capture
            .filter("icmp", true)
            .map_err(|e| format!("Failed to set filter: {}", e))?;

        let wifi_rx_handle = thread::spawn(move || {
            while running.lock().unwrap().load(Ordering::SeqCst) {
                // 1. Read from WiFi (with timeout)
                match wifi_capture.next_packet() {
                    Ok(packet) => {
                        if let Some((ip_packet, src_mac, dst_mac)) =
                            Self::parse_ethernet_frame(packet.data)
                        {
                            if dst_mac == router_wifi.config.wifi_mac {
                                info!(
                                    "WiFi RX Packet for us from {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                                    src_mac[0],
                                    src_mac[1],
                                    src_mac[2],
                                    src_mac[3],
                                    src_mac[4],
                                    src_mac[5]
                                );
                                wifi_to_router.send((ip_packet.clone(), InterfaceType::WiFi)).unwrap();
                            }
                        }
                    }
                    Err(pcap::Error::TimeoutExpired) => {
                        // Timeout, check for outgoing packets
                        info!("WIFI Capture Timeout");
                    }
                    Err(e) => {
                        warn!("WiFi capture error: {}", e);
                    }
                }
            }
        });

        let running = self.running.clone();
        let mut wifi_capture = crate::net::pcap_utils::open_capture(wifi_device)
            .map_err(|e| format!("Failed to open WiFi capture: {}", e))?;
        let wifi_tx_handle = thread::spawn(move || {
            while running.lock().unwrap().load(Ordering::SeqCst) {
                // 2. Send to WiFi
                while let Ok(frame) = to_wifi_rx.try_recv() {
                    info!("WiFi sent");
                    if let Err(e) = wifi_capture.sendpacket(frame) {
                        warn!("Failed to send packet to WiFi: {}", e);
                    }
                }
            }
        });

        // Spawn Acoustic Thread
        let running = self.running.clone();
        let acoustic_to_router = to_router_tx.clone();
        let acoustic_handle = thread::spawn(move || {
            while running.lock().unwrap().load(Ordering::SeqCst) {
                // 1. Read from Acoustic (non-blocking/timeout)
                match acoustic_interface
                    .receive_packet(Some(Duration::from_millis(10)))
                {
                    Ok(ip_packet) => {
                        acoustic_to_router
                            .send((ip_packet.clone(), InterfaceType::Acoustic))
                            .unwrap();
                        // router_acoustic
                        //     .handle_acoustic_packet(&to_wifi_tx, ip_packet);
                    }
                    Err(_) => {
                        // Timeout or error
                    }
                }

                // 2. Send to Acoustic
                while let Ok((ip_packet, dest_mac)) = to_acoustic_rx.try_recv() {
                    if let Err(e) = acoustic_interface.send_packet(
                        &ip_packet,
                        dest_mac,
                        FrameType::Data,
                    ) {
                        warn!("Failed to send packet to Acoustic: {}", e);
                    }
                }
            }
        });


        let mut gateway_tx_handle: Option<thread::JoinHandle<()>> = None;
        let mut gateway_rx_handle: Option<thread::JoinHandle<()>> = None;

        if let Some(main_device) = eth_device {
            if main_device.name != self.config.wifi_interface {
                // Gateway TX
                let mut gateway_send = crate::net::pcap_utils::open_capture(main_device.clone())
                    .map_err(|e| format!("Failed to open Ethernet capture: {}", e))?;
                let running = self.running.clone();
                gateway_tx_handle = Some(thread::spawn(move || {
                    while running.lock().unwrap().load(Ordering::SeqCst) {
                        while let Ok(frame) = to_eth_rx.try_recv() {
                            info!("Gateway sent");
                            if let Err(e) = gateway_send.sendpacket(frame) {
                                warn!("Failed to send packet to Ethernet: {}", e);
                            }
                        }
                    }
                }));
                
                // Gateway RX
                let network_router = self.clone();
                let eth_to_router = to_router_tx.clone();
                let mut gateway_recv = crate::net::pcap_utils::open_capture(main_device)
                    .map_err(|e| format!("Failed to open Ethernet capture: {}", e))?;
                gateway_recv.filter("icmp", true).unwrap();
                let running = self.running.clone();
                gateway_rx_handle = Some(thread::spawn(move || {
                    while running.lock().unwrap().load(Ordering::SeqCst) {
                        match gateway_recv.next_packet() {
                            Ok(packet) => {
                                if let Some((ip_packet, src_mac, dst_mac)) =
                                    Self::parse_ethernet_frame(packet.data)
                                {
                                    if dst_mac == network_router.config.eth_mac {
                                        info!(
                                            "Ethernet RX Packet for us from {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                                            src_mac[0],
                                            src_mac[1],
                                            src_mac[2],
                                            src_mac[3],
                                            src_mac[4],
                                            src_mac[5]
                                        );
                                        eth_to_router.send((ip_packet.clone(), InterfaceType::Ethernet)).unwrap();
                                    }
                                }
                            }
                            Err(pcap::Error::TimeoutExpired) => {
                                // Timeout, check for outgoing packets
                            }
                            Err(e) => {
                                warn!("Ethernet capture error: {}", e);
                            }
                        }
                    }
                }));
            }
        }

        // Main Router Loop
        let router_main = self.clone();
        let running = self.running.clone();
        let main_handle = thread::spawn(move || {
            while running.lock().unwrap().load(Ordering::SeqCst) {
                // Receive packets from interfaces
                match to_router_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok((ip_packet, src_interface)) => {
                        router_main.handle_packet(
                            &to_acoustic_tx,
                            &to_wifi_tx,
                            &to_eth_tx,
                            ip_packet,
                            src_interface,
                        );
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        // Timeout, continue
                    }
                    Err(e) => {
                        warn!("Router main loop error: {}", e);
                    }
                }
            }
        });

        // Wait for threads to finish
        if let Err(e) = wifi_rx_handle.join() {
            warn!("WiFi RX thread panicked: {:?}", e);
        }
        if let Err(e) = wifi_tx_handle.join() {
            warn!("WiFi RX thread panicked: {:?}", e);
        }
        if let Some(handle) = gateway_tx_handle {
            if let Err(e) = handle.join() {
                warn!("Ethernet TX thread panicked: {:?}", e);
            }
        }
        if let Some(handle) = gateway_rx_handle {
            if let Err(e) = handle.join() {
                warn!("Ethernet RX thread panicked: {:?}", e);
            }
        }
        if let Err(e) = acoustic_handle.join() {
            warn!("Acoustic thread panicked: {:?}", e);
        }
        if let Err(e) = main_handle.join() {
            warn!("Router main thread panicked: {:?}", e);
        }

        info!("Router stopped.");
        Ok(())
    }

    fn handle_packet(
        &self,
        to_acoustic: &crossbeam_channel::Sender<(Vec<u8>, u8)>,
        to_wifi: &crossbeam_channel::Sender<Vec<u8>>,
        to_eth: &crossbeam_channel::Sender<Vec<u8>>,
        ip_packet: Vec<u8>,
        src_interface: InterfaceType,
    ) {
        let mut state = PacketState::Ingress { iface: src_interface, raw_data: ip_packet };
        'router_loop: loop {
            match state {
                PacketState::Ingress { iface, raw_data } => {
                    let (src_ip, dest_ip, protocol) = {
                        let ip_header = match Ipv4HeaderSlice::from_slice(&raw_data) {
                            Ok(h) => h,
                            Err(e) => {
                                debug!("Failed to parse IP header: {}", e);
                                state = PacketState::Dropped { reason: format!("Invalid IP header: {}", e) };
                                continue 'router_loop;
                            }
                        };
                        (
                            Ipv4Addr::from(ip_header.source()),
                            Ipv4Addr::from(ip_header.destination()),
                            ip_header.protocol()
                        )
                    };

                    debug!(
                        "{:?} packet: {} -> {} (proto: {:?})",
                        iface,
                        src_ip,
                        dest_ip,
                        protocol
                    );

                    // Check if packet is for us (our IP / NAT response)
                    if self.is_for_us(&dest_ip) {
                        debug!("Packet is for router");
                        state = PacketState::LocalProcess {
                            src_ip,
                            packet: raw_data,
                        };
                        continue 'router_loop;
                    } else {
                        // Decrement TTL
                        let mut packet = raw_data;
                        if let Err(e) = Self::decrement_ttl(&mut packet) {
                            warn!("TTL expired: {}", e);
                            state = PacketState::Dropped { reason: format!("TTL expired: {}", e) };
                            continue 'router_loop;
                        }

                        state = PacketState::Routing {
                            src_ip,
                            dst_ip: dest_ip,
                            packet,
                        };
                    }
                }
                PacketState::LocalProcess { src_ip: _, packet } => {
                    self.handle_inbound_nat(to_acoustic, packet);
                    return;
                }
                PacketState::Routing { src_ip: _, dst_ip, packet } => {
                    // Re-parse IP header for Routing state logic
                    let ip_header = match Ipv4HeaderSlice::from_slice(&packet) {
                        Ok(h) => h,
                        Err(_) => {
                             state = PacketState::Dropped { reason: "Invalid IP header in Routing state".to_string() };
                             continue 'router_loop;
                        }
                    };

                    // TODO: search DNAT table/rule (Pre-Routing)

                    // Lookup routing table
                    let (new_dst_ip, new_iface) = match self.routing_table.lookup(&dst_ip) {
                        Some((next_hop, iface)) => {
                            if let Some(new_dst) = next_hop { // redirect to some gateway
                                (new_dst, iface)
                            } else {
                                (dst_ip, iface)
                            }
                        },
                        None => {
                            // Maybe for 0.0.0.0/0?
                            // Check for default gateway
                            (self.config.gateway_ip, InterfaceType::Ethernet) // TODO: change to other interface
                        }
                    };

                    // Post-Routing (SNAT)
                    if new_iface == InterfaceType::Ethernet && ip_header.protocol() == etherparse::IpNumber::ICMP {
                        // ICMP
                        // Parse ICMP
                        let ihl = ip_header.slice().len();
                        if let Ok(icmp_packet) = IcmpPacket::from_bytes(&packet[ihl..]) {
                            if icmp_packet.icmp_type == IcmpType::EchoRequest {
                                // Register in NAT table
                                let src_ip = Ipv4Addr::from(ip_header.source());
                                self.nat_table
                                    .register_echo_request(icmp_packet.identifier, src_ip);
                                debug!(
                                    "NAT: Registered Echo Request ID {} from {}",
                                    icmp_packet.identifier, src_ip
                                );

                                let new_src_ip = self.config.eth_ip;
                                let new_src_mac = self.config.eth_mac;
                                if let Some(gateway_mac) = self.config.gateway_mac {
                                    info!(
                                        "NAT Forwarding packet to Gateway: {} -> MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                                        new_dst_ip,
                                        gateway_mac[0],
                                        gateway_mac[1],
                                        gateway_mac[2],
                                        gateway_mac[3],
                                        gateway_mac[4],
                                        gateway_mac[5]
                                    );

                                    let builder =
                                        PacketBuilder::ipv4(new_src_ip.octets(), dst_ip.octets(), 60)
                                            .icmpv4_echo_request(
                                                icmp_packet.identifier,
                                                icmp_packet.sequence_number,
                                            );
                                    let new_payload = {
                                        let mut frame = Vec::<u8>::with_capacity(
                                            builder.size(icmp_packet.payload.len()),
                                        );
                                        builder.write(&mut frame, &icmp_packet.payload).unwrap();
                                        frame
                                    };

                                    state = PacketState::Send {
                                        out_interface: new_iface,
                                        payload: new_payload,
                                        src_mac: new_src_mac,
                                        dst_mac: gateway_mac,
                                    };
                                    continue 'router_loop;
                                } else {
                                    state = PacketState::Dropped {
                                        reason: "No gateway MAC configured, cannot perform NAT".to_string(),
                                    };
                                    continue 'router_loop;
                                }
                            }
                        }
                    }

                    state = PacketState::Send {
                        out_interface: new_iface,
                        payload: packet,
                        src_mac: match new_iface {
                            InterfaceType::WiFi => self.config.wifi_mac,
                            InterfaceType::Acoustic => {
                                let mut mac = [0u8; 6];
                                mac[5] = self.config.acoustic_mac;
                                mac
                            },
                            InterfaceType::Ethernet => self.config.eth_mac,
                        },
                        dst_mac: self.arp_table.get_mac(&new_dst_ip, new_iface).unwrap_or_else(|| {
                            // TODO: arp request
                            error!("No ARP entry for {}, using default MAC", new_dst_ip);
                            [0u8; 6]
                        }),
                    };
                    continue 'router_loop;
                }
                PacketState::Send {
                    out_interface,
                    payload,
                    src_mac,
                    dst_mac,
                } => {
                    match out_interface {
                        InterfaceType::Acoustic => {
                            if let Err(e) = to_acoustic.send((payload, dst_mac[5])) {
                                warn!("Failed to send packet to Acoustic thread: {}", e);
                            }
                        }
                        InterfaceType::WiFi => {
                            if let Err(e) = to_wifi.send(self.build_ethernet_frame(src_mac,  dst_mac, payload.as_slice())) {
                                warn!("Failed to send packet to WiFi thread: {}", e);
                            }
                        }
                        InterfaceType::Ethernet => {
                            if let Err(e) = to_eth.send(self.build_ethernet_frame(src_mac, dst_mac, payload.as_slice())) {
                                warn!("Failed to send packet to Ethernet thread: {}", e);
                            }
                        }
                    }
                    return;
                }
                PacketState::Dropped { reason } => {
                    warn!("Packet dropped: {}", reason);
                    return;
                }
            }
        }
    }
    /// Stop the router
    pub fn stop(&self) {
        self.running.lock().unwrap()
            .store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direct_network_contains() {
        let net = DirectNetwork::new(
            "192.168.1.0".parse().unwrap(),
            "255.255.255.0"
                .parse()
                .unwrap(),
            InterfaceType::Acoustic,
        );

        assert!(net.contains(&"192.168.1.1".parse().unwrap()));
        assert!(
            net.contains(
                &"192.168.1.254"
                    .parse()
                    .unwrap()
            )
        );
        assert!(!net.contains(&"192.168.2.1".parse().unwrap()));
        assert!(!net.contains(&"10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn test_routing_table_lookup() {
        let mut table = RoutingTable::new();
        table.add_direct_network(
            "192.168.1.0".parse().unwrap(),
            "255.255.255.0"
                .parse()
                .unwrap(),
            InterfaceType::Acoustic,
        );
        table.add_direct_network(
            "192.168.2.0".parse().unwrap(),
            "255.255.255.0"
                .parse()
                .unwrap(),
            InterfaceType::WiFi,
        );

        assert_eq!(
            table.lookup(&"192.168.1.5".parse().unwrap()),
            Some((None, InterfaceType::Acoustic))
        );
        assert_eq!(
            table.lookup(
                &"192.168.2.100"
                    .parse()
                    .unwrap()
            ),
            Some((None, InterfaceType::WiFi))
        );
        assert_eq!(table.lookup(&"10.0.0.1".parse().unwrap()), None);
    }

    #[test]
    fn test_decrement_ttl() {
        // Create a minimal valid IP header
        let mut packet = vec![
            0x45, 0x00, // Version/IHL, DSCP/ECN
            0x00, 0x14, // Total length (20)
            0x00, 0x00, // Identification
            0x00, 0x00, // Flags/Fragment offset
            0x40, 0x01, // TTL (64), Protocol (ICMP)
            0x00, 0x00, // Checksum (will be calculated)
            0xC0, 0xA8, 0x01, 0x01, // Source IP (192.168.1.1)
            0xC0, 0xA8, 0x02, 0x01, // Dest IP (192.168.2.1)
        ];

        // Calculate initial checksum
        let mut sum: u32 = 0;
        for i in (0..20).step_by(2) {
            let word = u16::from_be_bytes([packet[i], packet[i + 1]]);
            sum = sum.wrapping_add(word as u32);
        }
        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        let checksum = !(sum as u16);
        packet[10] = (checksum >> 8) as u8;
        packet[11] = (checksum & 0xFF) as u8;

        // Test TTL decrement
        assert!(Router::decrement_ttl(&mut packet).is_ok());
        assert_eq!(packet[8], 63); // TTL should be 63 now
    }
}
