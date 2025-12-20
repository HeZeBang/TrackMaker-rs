//! Router module for forwarding IP packets between interfaces
//!
//! This module implements a simple static router that forwards IP packets
//! between an acoustic interface (to NODE1) and a WiFi interface (to NODE3).

use etherparse::{
    ArpHardwareId, ArpOperation, ArpPacket, EtherType, Icmpv4Header, Icmpv4Type,
    IpNumber, Ipv4Header, Ipv4HeaderSlice, PacketBuilder, TcpHeaderSlice, UdpHeaderSlice,
};
use pcap::{Active, Capture, Device, Linktype};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock}; // Added RwLock for better read concurrency
use std::thread;
use std::time::Duration;
use tracing::{debug, error, info, trace, warn};

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
    /// Virtual TUN interface
    Tun,
}

/// Packet waiting for ARP resolution
#[derive(Debug, Clone)]
struct PendingPacket {
    interface: InterfaceType,
    packet: Vec<u8>,
    src_mac: [u8; 6],
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
        next_hop: Ipv4Addr,
    ) {
        self.routes.push(RouteEntry {
            network: DirectNetwork::new(network, mask, interface),
            next_hop: Some(next_hop),
        });
    }

    /// Lookup the interface for a destination IP
    pub fn lookup(
        &self,
        dest_ip: &Ipv4Addr,
    ) -> Option<(Option<Ipv4Addr>, InterfaceType)> {
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
    pub fn add_entry(
        &mut self,
        ip: Ipv4Addr,
        mac: [u8; 6],
        iface: InterfaceType,
    ) {
        self.table
            .entry(iface)
            .or_insert_with(HashMap::new)
            .insert(ip, mac);
    }

    /// Get MAC address for an IP
    pub fn get_mac(
        &self,
        ip: &Ipv4Addr,
        iface: InterfaceType,
    ) -> Option<[u8; 6]> {
        // Borrow the interface key for lookup, then copy the MAC out of the inner map
        self.table
            .get(&iface)
            .and_then(|m| m.get(ip).copied())
    }

    /// Update or add an ARP entry (for learning)
    pub fn update(
        &mut self,
        ip: Ipv4Addr,
        mac: [u8; 6],
        interface: InterfaceType,
    ) {
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
    /// Ethernet Netmask
    pub eth_netmask: Ipv4Addr,
    /// Ethernet Mac
    pub eth_mac: [u8; 6],
    /// Ethernet Gateway IP (e.g., 192.168.2.254)
    pub gateway_ip: Ipv4Addr,
    /// Ethernet Gateway MAC
    pub gateway_mac: Option<[u8; 6]>,
    /// Ethernet Gateway Interface
    pub gateway_interface: String,
    /// TUN interface name
    pub tun_name: String,
    /// TUN interface IP
    pub tun_ip: Ipv4Addr,
    /// TUN interface Netmask
    pub tun_netmask: Ipv4Addr,
    /// NODE3 IP (for Traversal)
    pub node3_ip: Ipv4Addr,
    /// NODE1 IP (for Traversal)
    pub node1_ip: Ipv4Addr,
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
            eth_netmask: "255.255.255.0"
                .parse()
                .unwrap(),
            eth_mac: [0x9c, 0x29, 0x76, 0x0c, 0x49, 0x00],
            tun_name: "tun0".to_string(),
            tun_ip: "10.0.0.1".parse().unwrap(),
            tun_netmask: "255.255.255.0"
                .parse()
                .unwrap(),
            node3_ip: "192.168.2.2".parse().unwrap(),
            node1_ip: "192.168.1.2".parse().unwrap(),
        }
    }
}

/// Simple IP Router
#[derive(Clone)]
pub struct Router {
    config: RouterConfig,
    // Use Arc<RwLock> to share state across threads.
    routing_table: Arc<RwLock<RoutingTable>>,
    arp_table: Arc<RwLock<ArpTable>>,
    nat_table: Arc<RwLock<NatTable>>,
    // Simple Session table for TCP/UDP NAT: Port -> Original IP
    // Assumes simple Cone NAT where external port maps to internal IP 1:1 (no port translation unless collision, but keeping simple)
    nat_sessions: Arc<RwLock<HashMap<u16, Ipv4Addr>>>, 
    // Buffer for packets awaiting ARP resolution
    pending_packets: Arc<RwLock<HashMap<Ipv4Addr, Vec<PendingPacket>>>>,
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
    LocalProcess { src_ip: Ipv4Addr, packet: Vec<u8> },
    /// Pack a frame, ready to send
    Send {
        out_interface: InterfaceType,
        payload: Vec<u8>,
        src_mac: [u8; 6],
        dst_mac: [u8; 6],
    },
    /// Dropped packet
    Dropped { reason: String },
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

        // Add Ethernet network
        let eth_net_octets = [
            config.eth_ip.octets()[0] & config.eth_netmask.octets()[0],
            config.eth_ip.octets()[1] & config.eth_netmask.octets()[1],
            config.eth_ip.octets()[2] & config.eth_netmask.octets()[2],
            config.eth_ip.octets()[3] & config.eth_netmask.octets()[3],
        ];
        routing_table.add_direct_network(
            Ipv4Addr::from(eth_net_octets),
            config.eth_netmask,
            InterfaceType::Ethernet,
        );

        // Calculate TUN network
        let tun_net_octets = [
            config.tun_ip.octets()[0] & config.tun_netmask.octets()[0],
            config.tun_ip.octets()[1] & config.tun_netmask.octets()[1],
            config.tun_ip.octets()[2] & config.tun_netmask.octets()[2],
            config.tun_ip.octets()[3] & config.tun_netmask.octets()[3],
        ];
        routing_table.add_direct_network(
            Ipv4Addr::from(tun_net_octets),
            config.tun_netmask,
            InterfaceType::Tun,
        );

        Self {
            config,
            routing_table: Arc::new(RwLock::new(routing_table)),
            arp_table: Arc::new(RwLock::new(ArpTable::new())),
            nat_table: Arc::new(RwLock::new(NatTable::new())),
            nat_sessions: Arc::new(RwLock::new(HashMap::new())),
            pending_packets: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(Mutex::new(AtomicBool::new(false))),
        }
    }

    /// Add a static ARP entry for Other(Gateway)
    pub fn add_arp_entry(
        &self,
        ip: Ipv4Addr,
        mac: [u8; 6],
        interface: InterfaceType,
    ) {
        if let Ok(mut table) = self.arp_table.write() {
            table.add_entry(ip, mac, interface);
        }
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
    fn parse_ethernet_frame(
        frame: &[u8],
    ) -> Option<(Vec<u8>, [u8; 6], [u8; 6], u16)> {
        // TODO: use etherparse
        let eth = etherparse::Ethernet2HeaderSlice::from_slice(frame).ok()?;
        if frame.len() < 14 {
            return None;
        }

        let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
        if ethertype != 0x0800 && ethertype != 0x0806 {
            // Not IPv4 or ARP
            return None;
        }

        Some((
            frame[14..].to_vec(),
            eth.source(),
            eth.destination(),
            ethertype,
        ))
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
    }

    /// Recalculate TCP or UDP Checksum (Pseudo-Header + L4 Header + Payload)
    /// This is required when IP addresses change (NAT).
    fn recalculate_l4_checksum(packet: &mut [u8], src_ip: Ipv4Addr, dst_ip: Ipv4Addr, protocol: IpNumber) {
        let ihl = (packet[0] & 0x0F) as usize * 4;
        if packet.len() < ihl { return; }
        
        let l4_len = packet.len() - ihl;
        let l4_bytes = &mut packet[ihl..];

        // Reset Checksum Field to 0 before calculation
        match protocol {
            IpNumber::TCP => {
                if l4_bytes.len() >= 18 {
                    l4_bytes[16] = 0;
                    l4_bytes[17] = 0;
                } else { return; }
            },
            IpNumber::UDP => {
                 if l4_bytes.len() >= 8 {
                    l4_bytes[6] = 0;
                    l4_bytes[7] = 0;
                } else { return; }
            },
            _ => return,
        }

        let mut sum: u32 = 0;
        
        // 1. Pseudo Header Sum
        let src_oct = src_ip.octets();
        let dst_oct = dst_ip.octets();
        
        sum += u16::from_be_bytes([src_oct[0], src_oct[1]]) as u32;
        sum += u16::from_be_bytes([src_oct[2], src_oct[3]]) as u32;
        sum += u16::from_be_bytes([dst_oct[0], dst_oct[1]]) as u32;
        sum += u16::from_be_bytes([dst_oct[2], dst_oct[3]]) as u32;
        
        sum += protocol.0 as u32;
        sum += l4_len as u32;

        // 2. L4 Header + Payload Sum
        for i in (0..l4_bytes.len()).step_by(2) {
            if i + 1 < l4_bytes.len() {
                 sum += u16::from_be_bytes([l4_bytes[i], l4_bytes[i+1]]) as u32;
            } else {
                 // Padding for odd length
                 sum += u16::from_be_bytes([l4_bytes[i], 0]) as u32;
            }
        }

        // 3. Fold to 16 bits
        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        let checksum = !(sum as u16);

        // 4. Write Checksum
         match protocol {
            IpNumber::TCP => {
                 l4_bytes[16] = (checksum >> 8) as u8;
                 l4_bytes[17] = (checksum & 0xFF) as u8;
            },
            IpNumber::UDP => {
                 l4_bytes[6] = (checksum >> 8) as u8;
                 l4_bytes[7] = (checksum & 0xFF) as u8;
            },
            _ => {},
        }
    }

    /// Process packet using etherparse (decrement TTL, rebuild checksums)
    fn process_packet_with_etherparse(
        packet_data: &[u8],
    ) -> Result<Vec<u8>, String> {
        let (mut ip_header, payload) = Ipv4Header::from_slice(packet_data)
            .map_err(|e| format!("Invalid IPv4 header: {}", e))?;

        if ip_header.time_to_live <= 1 {
            return Err("TTL expired".to_string());
        }
        ip_header.time_to_live -= 1;

        if ip_header.protocol == IpNumber::ICMP {
            // Try to parse as ICMP
            if let Ok((icmp_header, icmp_payload)) =
                Icmpv4Header::from_slice(payload)
            {
                // If it is Echo Reply or Request, we can use PacketBuilder to be safe and "create an icmp"
                if let Icmpv4Type::EchoReply(echo) = icmp_header.icmp_type {
                    let builder = PacketBuilder::ipv4(
                        ip_header.source,
                        ip_header.destination,
                        ip_header.time_to_live,
                    )
                    .icmpv4_echo_reply(echo.id, echo.seq);

                    let mut result =
                        Vec::with_capacity(builder.size(icmp_payload.len()));
                    builder
                        .write(&mut result, icmp_payload)
                        .map_err(|e| {
                            format!("Failed to build ICMP packet: {}", e)
                        })?;
                    return Ok(result);
                } else if let Icmpv4Type::EchoRequest(echo) =
                    icmp_header.icmp_type
                {
                    let builder = PacketBuilder::ipv4(
                        ip_header.source,
                        ip_header.destination,
                        ip_header.time_to_live,
                    )
                    .icmpv4_echo_request(echo.id, echo.seq);

                    let mut result =
                        Vec::with_capacity(builder.size(icmp_payload.len()));
                    builder
                        .write(&mut result, icmp_payload)
                        .map_err(|e| {
                            format!("Failed to build ICMP packet: {}", e)
                        })?;
                    return Ok(result);
                }
            }
        }

        // Fallback for non-ICMP or other ICMP types: just rewrite IP header
        let mut result = Vec::with_capacity(packet_data.len());
        ip_header
            .write(&mut result)
            .map_err(|e| format!("Failed to write IP header: {}", e))?;
        result.extend_from_slice(payload);
        Ok(result)
    }

    fn prepare_arp_request(
        &self,
        source_mac: [u8; 6],
        source_ip: Ipv4Addr,
        target_ip: Ipv4Addr,
    ) -> Vec<u8> {
        // 2. 构造 ARP 请求帧
        let target_mac = [0xff; 6]; // broadcast

        // 使用 PacketBuilder 构造 Ethernet + ARP 帧
        let builder = PacketBuilder::ethernet2(source_mac, target_mac).arp(
            ArpPacket::new(
                ArpHardwareId::ETHERNET,
                EtherType::IPV4,
                ArpOperation::REQUEST,
                &source_mac,         // sender_hw_addr
                &source_ip.octets(), // sender_protocol_addr
                &[0u8; 6],           // target_hw_addr
                &target_ip.octets(), // target_protocol_addr
            )
            .unwrap(),
        );

        // get some memory to store the result
        let mut result = Vec::<u8>::with_capacity(builder.size());

        // serialize
        builder
            .write(&mut result)
            .unwrap();

        debug!("Built ARP request, len = {}", result.len());

        result
    }

    /// Handle inbound NAT. If translated, modifies packet in-place and returns original destination IP.
    fn handle_inbound_nat(&self, ip_packet: &mut Vec<u8>) -> Option<Ipv4Addr> {
        let ip_header = match Ipv4HeaderSlice::from_slice(ip_packet) {
            Ok(h) => h,
            Err(_) => return None,
        };

        // If the packet is destined for the Router's Ethernet IP, it might need DNAT
        let dst_ip = ip_header.destination_addr();
        if dst_ip != self.config.eth_ip {
             // Not for our WAN IP, so standard routing or already correct
             return None;
        }

        let protocol = ip_header.protocol();
        let ihl = ip_header.slice().len();
        let src_ip = ip_header.source_addr();

        if protocol == etherparse::IpNumber::ICMP {
            // ICMP
            if let Ok(icmp_packet) = IcmpPacket::from_bytes(&ip_packet[ihl..]) {
                if icmp_packet.icmp_type == IcmpType::EchoReply {
                    // Lookup in NAT table (Thread-safe read)
                    let original_ip_opt =
                        if let Ok(table) = self.nat_table.read() {
                            table.translate_echo_reply(icmp_packet.identifier)
                        } else {
                            None
                        };

                    if let Some(original_ip) = original_ip_opt {
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
                        Self::recalculate_ip_checksum(ip_packet);

                        return Some(original_ip);
                    }
                }
            }
        } else if protocol == etherparse::IpNumber::TCP {
            if let Ok(tcp) = TcpHeaderSlice::from_slice(&ip_packet[ihl..]) {
                let dst_port = tcp.destination_port();
                // Lookup session
                let original_ip_opt = if let Ok(sessions) = self.nat_sessions.read() {
                    sessions.get(&dst_port).copied()
                } else { None };

                if let Some(original_ip) = original_ip_opt {
                    debug!("NAT: Translating TCP Port {} to {}", dst_port, original_ip);
                    
                    // Modify Destination IP in IP Header
                    let original_ip_octets = original_ip.octets();
                    ip_packet[16] = original_ip_octets[0];
                    ip_packet[17] = original_ip_octets[1];
                    ip_packet[18] = original_ip_octets[2];
                    ip_packet[19] = original_ip_octets[3];

                    // Recalculate IP Checksum
                    Self::recalculate_ip_checksum(ip_packet);

                    // Recalculate TCP Checksum (Critical!)
                    // Note: We use the *NEW* destination IP (original_ip) for checksum calculation
                    Self::recalculate_l4_checksum(ip_packet, src_ip, original_ip, protocol);

                    return Some(original_ip);
                }
            }
        } else if protocol == etherparse::IpNumber::UDP {
            if let Ok(udp) = UdpHeaderSlice::from_slice(&ip_packet[ihl..]) {
                 let dst_port = udp.destination_port();
                 // Lookup session
                let original_ip_opt = if let Ok(sessions) = self.nat_sessions.read() {
                    sessions.get(&dst_port).copied()
                } else { None };

                if let Some(original_ip) = original_ip_opt {
                    debug!("NAT: Translating UDP Port {} to {}", dst_port, original_ip);
                    
                     // Modify Destination IP in IP Header
                    let original_ip_octets = original_ip.octets();
                    ip_packet[16] = original_ip_octets[0];
                    ip_packet[17] = original_ip_octets[1];
                    ip_packet[18] = original_ip_octets[2];
                    ip_packet[19] = original_ip_octets[3];

                    // Recalculate IP Checksum
                    Self::recalculate_ip_checksum(ip_packet);

                    // Recalculate UDP Checksum
                    Self::recalculate_l4_checksum(ip_packet, src_ip, original_ip, protocol);

                    return Some(original_ip);
                }
            }
        }

        // If not handled by NAT, ignore (since it was addressed to us but not NATed)
        trace!("Packet for router itself, ignoring (let host stack handle)");
        None
    }

    /// Check if packet is for us (router itself)
    fn is_for_us(&self, dest_ip: &Ipv4Addr) -> bool {
        *dest_ip == self.config.acoustic_ip
            || *dest_ip == self.config.wifi_ip
            || *dest_ip == self.config.eth_ip
    }

    /// Run the router
    pub fn run(
        &mut self,
        shared: AppShared,
        sample_rate: u32,
        line_coding: LineCodingKind,
    ) -> Result<(), String> {
        self.running
            .lock()
            .unwrap()
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
        info!(
            "Traversal Targets: NODE3={}, NODE1={}",
            self.config.node3_ip, self.config.node1_ip
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
        let eth_device = if self.config.gateway_interface
            != self.config.wifi_interface
        {
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

        // Create TUN interface
        let mut tun_config = tun::Configuration::default();
        tun_config
            .address(self.config.tun_ip)
            .netmask(self.config.tun_netmask)
            .destination(self.config.tun_ip)
            .tun_name(&self.config.tun_name)
            .up();

        #[cfg(target_os = "linux")]
        tun_config.platform_config(|config| {
            config.packet_information(false);
        });

        let tun_device = tun::create(&tun_config)
            .map_err(|e| format!("Failed to create TUN device: {}", e))?;

        info!("Router is running. Press Ctrl+C to stop.");

        // Channels for inter-thread communication
        let (to_acoustic_tx, to_acoustic_rx) =
            crossbeam_channel::unbounded::<(Vec<u8>, u8)>();
        let (to_wifi_tx, to_wifi_rx) = crossbeam_channel::unbounded::<Vec<u8>>();
        let (to_eth_tx, to_eth_rx) = crossbeam_channel::unbounded::<Vec<u8>>();
        let (to_tun_tx, to_tun_rx) = crossbeam_channel::unbounded::<Vec<u8>>();
        let (to_router_tx, to_router_rx) =
            crossbeam_channel::unbounded::<(Vec<u8>, InterfaceType)>();

        // Spawn TUN Threads
        let tun_to_router = to_router_tx.clone();
        let running = self.running.clone();

        // Split TUN device
        let (mut tun_reader, mut tun_writer) = tun_device.split();

        // TUN RX (Read from TUN -> Send to Router)
        let tun_rx_handle = thread::spawn(move || {
            let mut buf = [0u8; 1504];
            while running
                .lock()
                .unwrap()
                .load(Ordering::SeqCst)
            {
                // Blocking read is fine here as long as tun_reader supports it properly
                match std::io::Read::read(&mut tun_reader, &mut buf) {
                    Ok(n) => {
                        if n > 0 {
                            let packet = buf[..n].to_vec();
                            tun_to_router
                                .send((packet, InterfaceType::Tun))
                                .unwrap();
                        }
                    }
                    Err(e) => {
                        // If it's a temp error or timeout, we might continue
                        // For a real error, we might log and break
                        match e.kind() {
                            std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut => {
                                // Short sleep to avoid tight loop on non-blocking error
                                thread::sleep(Duration::from_millis(10));
                            }
                            _ => {
                                warn!("TUN read error: {}", e);
                                thread::sleep(Duration::from_millis(100));
                            }
                        }
                    }
                }
            }
        });

        // TUN TX (Read from Channel -> Write to TUN)
        // Optimized: Use blocking iterator instead of busy waiting loop
        let tun_tx_handle = thread::spawn(move || {
            // This loop automatically terminates when to_tun_tx is dropped
            for packet in to_tun_rx {
                info!("Writing packet to TUN device (len={})", packet.len());
                if let Err(e) =
                    std::io::Write::write_all(&mut tun_writer, &packet)
                {
                    warn!("Failed to write to TUN: {}", e);
                }
            }
            debug!("TUN TX thread stopping");
        });

        // WiFi Hotspot RX

        // Spawn WiFi Thread
        let router_wifi = self.clone(); // Clone is now cheap and safe (shares Arc<RwLock>)
        let running = self.running.clone();
        let wifi_to_router = to_router_tx.clone();

        let mut wifi_capture =
            crate::net::pcap_utils::open_capture(wifi_device.clone())
                .map_err(|e| format!("Failed to open WiFi capture: {}", e))?;

        // Set filter to only capture IP packets (including TCP, UDP)
        wifi_capture
            .filter("icmp or arp or tcp or udp", true)
            .map_err(|e| format!("Failed to set filter: {}", e))?;

        let wifi_rx_handle = thread::spawn(move || {
            while running
                .lock()
                .unwrap()
                .load(Ordering::SeqCst)
            {
                // 1. Read from WiFi (with timeout)
                // pcap `next_packet` is blocking or has timeout set in open_capture
                match wifi_capture.next_packet() {
                    Ok(packet) => {
                        if let Some((ip_packet, src_mac, dst_mac, _ethertype)) =
                            Self::parse_ethernet_frame(packet.data)
                        {
                            if dst_mac == router_wifi.config.wifi_mac
                                || dst_mac == [0xff; 6]
                            {
                                debug!(
                                    "WiFi RX Packet for us from {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                                    src_mac[0],
                                    src_mac[1],
                                    src_mac[2],
                                    src_mac[3],
                                    src_mac[4],
                                    src_mac[5]
                                );
                                wifi_to_router
                                    .send((
                                        ip_packet.clone(),
                                        InterfaceType::WiFi,
                                    ))
                                    .unwrap();
                            }
                        }
                    }
                    Err(pcap::Error::TimeoutExpired) => {
                        // Timeout allows checking `running` flag
                    }
                    Err(e) => {
                        warn!("WiFi capture error: {}", e);
                        thread::sleep(Duration::from_millis(100)); // Prevent tight loop on error
                    }
                }
            }
        });

        // WiFi TX Thread
        // Optimized: Use blocking iterator
        let mut wifi_capture = crate::net::pcap_utils::open_capture(wifi_device)
            .map_err(|e| format!("Failed to open WiFi capture: {}", e))?;
        let wifi_tx_handle = thread::spawn(move || {
            // Loop until channel is closed
            for frame in to_wifi_rx {
                info!("WiFi sent");
                if let Err(e) = wifi_capture.sendpacket(frame) {
                    warn!("Failed to send packet to WiFi: {}", e);
                }
            }
            debug!("WiFi TX thread stopping");
        });

        // Spawn Acoustic Thread
        let running = self.running.clone();
        let acoustic_to_router = to_router_tx.clone();
        let acoustic_handle = thread::spawn(move || {
            while running
                .lock()
                .unwrap()
                .load(Ordering::SeqCst)
            {
                // 1. Read from Acoustic (non-blocking/timeout)
                // Assuming receive_packet has internal timeout logic
                match acoustic_interface
                    .receive_packet(Some(Duration::from_millis(10)))
                {
                    Ok(ip_packet) => {
                        acoustic_to_router
                            .send((ip_packet.clone(), InterfaceType::Acoustic))
                            .unwrap();
                    }
                    Err(_) => {
                        // Timeout or error, just continue
                    }
                }

                // 2. Send to Acoustic
                // Use try_recv here because we are in a loop handling both RX and TX in one thread.
                // This is a specific design for acoustic interface which might be half-duplex or single-threaded.
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
                // Optimized: Use blocking iterator
                let mut gateway_send =
                    crate::net::pcap_utils::open_capture(main_device.clone())
                        .map_err(|e| {
                            format!("Failed to open Ethernet capture: {}", e)
                        })?;

                gateway_tx_handle = Some(thread::spawn(move || {
                    for frame in to_eth_rx {
                        // info!("Gateway sent");
                        if let Err(e) = gateway_send.sendpacket(frame) {
                            warn!("Failed to send packet to Ethernet: {}", e);
                        }
                    }
                    debug!("Ethernet TX thread stopping");
                }));

                // Gateway RX
                let network_router = self.clone(); // Clone Safe Router
                let eth_to_router = to_router_tx.clone();
                let mut gateway_recv =
                    crate::net::pcap_utils::open_capture(main_device).map_err(
                        |e| format!("Failed to open Ethernet capture: {}", e),
                    )?;
                gateway_recv
                    .filter("icmp or arp or tcp or udp", true)
                    .unwrap();
                let running = self.running.clone();
                gateway_rx_handle = Some(thread::spawn(move || {
                    while running
                        .lock()
                        .unwrap()
                        .load(Ordering::SeqCst)
                    {
                        match gateway_recv.next_packet() {
                            Ok(packet) => {
                                if let Some((
                                    ip_packet,
                                    src_mac,
                                    dst_mac,
                                    _ethertype,
                                )) = Self::parse_ethernet_frame(packet.data)
                                {
                                    if src_mac == network_router.config.eth_mac {
                                        // Ignore packets sent by ourselves
                                        continue;
                                    }
                                    if dst_mac == network_router.config.eth_mac
                                        || dst_mac == [0xff; 6]
                                    {
                                        trace!(
                                            "Ethernet RX Packet for us from {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                                            src_mac[0],
                                            src_mac[1],
                                            src_mac[2],
                                            src_mac[3],
                                            src_mac[4],
                                            src_mac[5]
                                        );
                                        eth_to_router
                                            .send((
                                                ip_packet.clone(),
                                                InterfaceType::Ethernet,
                                            ))
                                            .unwrap();
                                    }
                                }
                            }
                            Err(pcap::Error::TimeoutExpired) => {
                                // Timeout, check for outgoing packets
                            }
                            Err(e) => {
                                warn!("Ethernet capture error: {}", e);
                                thread::sleep(Duration::from_millis(100));
                            }
                        }
                    }
                }));
            }
        }

        // Main Router Loop
        let mut router_main = self.clone();
        let running = self.running.clone();
        let main_handle = thread::spawn(move || {
            while running
                .lock()
                .unwrap()
                .load(Ordering::SeqCst)
            {
                // Receive packets from interfaces
                // Uses recv_timeout so we can periodically check `running` flag
                // Alternatively, we could just block on recv() if we dropping the sender was the only stop mechanism.
                match to_router_rx.recv_timeout(Duration::from_secs(1)) {
                    Ok((ip_packet, src_interface)) => {
                        router_main.handle_packet(
                            &to_acoustic_tx,
                            &to_wifi_tx,
                            &to_eth_tx,
                            &to_tun_tx,
                            ip_packet,
                            src_interface,
                        );
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        // Timeout, continue loop to check running flag
                    }
                    Err(e) => {
                        warn!("Router main loop error: {}", e);
                        break; // Channel disconnected
                    }
                }
            }
            debug!("Main router loop stopping");
        });

        // Wait for threads to finish
        // Note: RX threads typically need an external signal or loop check to stop.
        // TX threads will stop when the main_handle drops the senders (which happens when main_handle finishes).

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
        if let Err(e) = tun_rx_handle.join() {
            warn!("TUN RX thread panicked: {:?}", e);
        }
        if let Err(e) = tun_tx_handle.join() {
            warn!("TUN TX thread panicked: {:?}", e);
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
        &mut self,
        to_acoustic: &crossbeam_channel::Sender<(Vec<u8>, u8)>,
        to_wifi: &crossbeam_channel::Sender<Vec<u8>>,
        to_eth: &crossbeam_channel::Sender<Vec<u8>>,
        to_tun: &crossbeam_channel::Sender<Vec<u8>>,
        ip_packet: Vec<u8>,
        src_interface: InterfaceType,
    ) {
        let mut state = PacketState::Ingress {
            iface: src_interface,
            raw_data: ip_packet,
        };
        'router_loop: loop {
            match state {
                PacketState::Ingress { iface, raw_data } => {
                    if iface == InterfaceType::Acoustic {
                        to_tun
                            .send(raw_data.clone())
                            .unwrap();
                    }
                    // Check if it's ARP (starts with 0x0001 for Ethernet HW type)
                    if raw_data.len() >= 28
                        && raw_data[0] == 0x00
                        && raw_data[1] == 0x01
                    {
                        // Manual ARP parsing
                        let hw_type =
                            u16::from_be_bytes([raw_data[0], raw_data[1]]);
                        let proto_type =
                            u16::from_be_bytes([raw_data[2], raw_data[3]]);
                        let hw_len = raw_data[4];
                        let proto_len = raw_data[5];
                        let opcode =
                            u16::from_be_bytes([raw_data[6], raw_data[7]]);

                        if hw_type == 1
                            && proto_type == 0x0800
                            && hw_len == 6
                            && proto_len == 4
                        {
                            if opcode == 2 {
                                // Reply
                                let mut sender_mac = [0u8; 6];
                                sender_mac.copy_from_slice(&raw_data[8..14]);
                                let sender_ip = Ipv4Addr::new(
                                    raw_data[14],
                                    raw_data[15],
                                    raw_data[16],
                                    raw_data[17],
                                );

                                info!(
                                    "ARP Reply: {} is at {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                                    sender_ip,
                                    sender_mac[0],
                                    sender_mac[1],
                                    sender_mac[2],
                                    sender_mac[3],
                                    sender_mac[4],
                                    sender_mac[5]
                                );

                                // Update ARP Table Thread-Safely
                                if let Ok(mut table) = self.arp_table.write() {
                                    table.update(sender_ip, sender_mac, iface);
                                }

                                // Check for pending packets (packets that were waiting for this ARP reply)
                                let buffered = if let Ok(mut pending) =
                                    self.pending_packets.write()
                                {
                                    pending.remove(&sender_ip)
                                } else {
                                    None
                                };

                                if let Some(packets) = buffered {
                                    info!(
                                        "ARP Resolved for {}. Sending {} buffered packets.",
                                        sender_ip,
                                        packets.len()
                                    );
                                    for pkt in packets {
                                        match pkt.interface {
                                            InterfaceType::WiFi => {
                                                let frame = self
                                                    .build_ethernet_frame(
                                                        pkt.src_mac,
                                                        sender_mac,
                                                        &pkt.packet,
                                                    );
                                                if let Err(e) =
                                                    to_wifi.send(frame)
                                                {
                                                    warn!(
                                                        "Failed to send buffered WiFi packet: {}",
                                                        e
                                                    );
                                                }
                                            }
                                            InterfaceType::Ethernet => {
                                                let frame = self
                                                    .build_ethernet_frame(
                                                        pkt.src_mac,
                                                        sender_mac,
                                                        &pkt.packet,
                                                    );
                                                if let Err(e) =
                                                    to_eth.send(frame)
                                                {
                                                    warn!(
                                                        "Failed to send buffered Ethernet packet: {}",
                                                        e
                                                    );
                                                }
                                            }
                                            InterfaceType::Acoustic => {
                                                if let Err(e) = to_acoustic.send(
                                                    (pkt.packet, sender_mac[5]),
                                                ) {
                                                    warn!(
                                                        "Failed to send buffered Acoustic packet: {}",
                                                        e
                                                    );
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                        return;
                    }

                    let (src_ip, dest_ip, protocol) = {
                        let ip_header =
                            match Ipv4HeaderSlice::from_slice(&raw_data) {
                                Ok(h) => h,
                                Err(e) => {
                                    debug!("Failed to parse IP header: {}", e);
                                    state = PacketState::Dropped {
                                        reason: format!(
                                            "Invalid IP header: {}",
                                            e
                                        ),
                                    };
                                    continue 'router_loop;
                                }
                            };
                        (
                            Ipv4Addr::from(ip_header.source()),
                            Ipv4Addr::from(ip_header.destination()),
                            ip_header.protocol(),
                        )
                    };

                    debug!(
                        "{:?} packet: {} -> {} (proto: {:?})",
                        iface, src_ip, dest_ip, protocol
                    );

                    // Check if packet is for us (our IP / NAT response)
                    if self.is_for_us(&dest_ip) {
                        // Check for Traversal (DNAT)
                        if protocol == etherparse::IpNumber::ICMP {
                            // Parse ICMP
                            let ihl = (raw_data[0] & 0x0F) as usize * 4;
                            if let Ok(icmp_packet) =
                                IcmpPacket::from_bytes(&raw_data[ihl..])
                            {
                                if icmp_packet.icmp_type == IcmpType::EchoRequest
                                {
                                    // Check payload
                                    if icmp_packet.payload.len() > 16 {
                                        let first_byte = icmp_packet.payload[16]; // Data first byte
                                        info!("First byte: {:02x}", first_byte);
                                        let target_ip = if first_byte == 0xaa {
                                            Some(self.config.node3_ip)
                                        } else if first_byte == 0xbb {
                                            Some(self.config.node1_ip)
                                        } else {
                                            None
                                        };

                                        if let Some(new_dst) = target_ip {
                                            info!(
                                                "Traversal: Forwarding Echo Request (payload {:02x}) to {}",
                                                first_byte, new_dst
                                            );

                                            // Register DNAT session (Thread-safe write)
                                            if let Ok(mut table) =
                                                self.nat_table.write()
                                            {
                                                table.register_dnat_session(
                                                    icmp_packet.identifier,
                                                );
                                            }
                                            info!(
                                                "Traversal: Registered DNAT session for ID {}",
                                                icmp_packet.identifier
                                            );

                                            // Modify Destination IP
                                            let mut packet = raw_data.clone();
                                            let new_dst_octets =
                                                new_dst.octets();
                                            packet[16] = new_dst_octets[0];
                                            packet[17] = new_dst_octets[1];
                                            packet[18] = new_dst_octets[2];
                                            packet[19] = new_dst_octets[3];

                                            // Recalculate IP Checksum
                                            Self::recalculate_ip_checksum(
                                                &mut packet,
                                            );

                                            // Decrement TTL (since we are forwarding)
                                            match Self::decrement_ttl(
                                                &mut packet,
                                            ) {
                                                Ok(_) => {
                                                    state =
                                                        PacketState::Routing {
                                                            src_ip,
                                                            dst_ip: new_dst,
                                                            packet,
                                                        };
                                                    continue 'router_loop;
                                                }
                                                Err(e) => {
                                                    state =
                                                        PacketState::Dropped {
                                                            reason: e
                                                                .to_string(),
                                                        };
                                                    continue 'router_loop;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        trace!("Packet is for router");
                        state = PacketState::LocalProcess {
                            src_ip,
                            packet: raw_data,
                        };
                        continue 'router_loop;
                    } else {
                        // Decrement TTL and rebuild packet
                        let packet = match Self::process_packet_with_etherparse(
                            &raw_data,
                        ) {
                            Ok(p) => p,
                            Err(e) => {
                                warn!("Failed to process packet: {}", e);
                                state = PacketState::Dropped { reason: e };
                                continue 'router_loop;
                            }
                        };

                        state = PacketState::Routing {
                            src_ip,
                            dst_ip: dest_ip,
                            packet,
                        };
                    }
                }
                PacketState::LocalProcess { src_ip, mut packet } => {
                    if let Some(new_dest_ip) =
                        self.handle_inbound_nat(&mut packet)
                    {
                        state = PacketState::Routing {
                            src_ip,
                            dst_ip: new_dest_ip,
                            packet,
                        };
                        continue 'router_loop;
                    }

                    // If not NAT, check if it is for Acoustic IP (which means it should go to TUN)
                    let is_acoustic_dest =
                        if let Ok(h) = Ipv4HeaderSlice::from_slice(&packet) {
                            h.destination_addr() == self.config.acoustic_ip
                        } else {
                            false
                        };

                    if is_acoustic_dest {
                        // Forward to TUN
                        state = PacketState::Send {
                            out_interface: InterfaceType::Tun,
                            payload: packet,
                            src_mac: [0u8; 6],
                            dst_mac: [0u8; 6],
                        };
                        continue 'router_loop;
                    }

                    return;
                }
                PacketState::Routing {
                    src_ip: _,
                    dst_ip,
                    mut packet,
                } => {
                    // Re-parse IP header for Routing state logic
                    let (protocol, ihl, src_ip_from_header) =
                        match Ipv4HeaderSlice::from_slice(&packet) {
                            Ok(h) => (
                                h.protocol(),
                                h.slice().len(),
                                Ipv4Addr::from(h.source()),
                            ),
                            Err(_) => {
                                state = PacketState::Dropped {
                                    reason: "Invalid IP header in Routing state"
                                        .to_string(),
                                };
                                continue 'router_loop;
                            }
                        };

                    // TODO: search DNAT table/rule (Pre-Routing)

                    // Lookup routing table (Thread-safe read)
                    let (new_dst_ip, new_iface) = if let Ok(table) =
                        self.routing_table.read()
                    {
                        match table.lookup(&dst_ip) {
                            Some((next_hop, iface)) => {
                                if let Some(new_dst) = next_hop {
                                    // redirect to some gateway
                                    (new_dst, iface)
                                } else {
                                    (dst_ip, iface)
                                }
                            }
                            None => {
                                // Maybe for 0.0.0.0/0?
                                // Check for default gateway
                                (self.config.gateway_ip, InterfaceType::Ethernet) // TODO: change to other interface
                            }
                        }
                    } else {
                        (self.config.gateway_ip, InterfaceType::Ethernet)
                    };

                    // Post-Routing (SNAT/DNAT handling)
                    // Handle packets going to local acoustic/TUN interfaces (reverse NAT)
                    if (new_iface == InterfaceType::Acoustic || new_iface == InterfaceType::Tun)
                        && (src_ip_from_header == self.config.gateway_ip || 
                            (src_ip_from_header.octets()[0..3] == self.config.eth_ip.octets()[0..3]))
                    {
                        // This packet came from external (Ethernet/Gateway) and is returning to local TUN
                        // No DNAT needed here - just forward as-is since the app on TUN
                        // will recognize the response by its own port/IP
                        debug!("Reverse NAT: Packet from external {} -> local {:?}", src_ip_from_header, new_iface);
                    }
                    
                    if new_iface == InterfaceType::Ethernet
                    {
                        // SNAT Logic for Ethernet interface
                        let new_src_ip = self.config.eth_ip;
                        let new_src_mac = self.config.eth_mac;
                        let gateway_mac = self.config.gateway_mac.unwrap_or([0xff; 6]); // Fallback if no gateway MAC, usually handled by ARP later but needed for build
                        
                        let can_snat = self.config.gateway_mac.is_some();
                        if !can_snat {
                             // Wait for ARP for gateway logic handled later in loop
                        }

                        if protocol == etherparse::IpNumber::ICMP {
                             // ICMP
                            debug!("Post-Routing: Ethernet ICMP packet");
                            // Parse ICMP
                            // We need to check if it is EchoRequest or EchoReply
                            let (icmp_type, icmp_id, icmp_seq) =
                                if let Ok(icmp_packet) =
                                    IcmpPacket::from_bytes(&packet[ihl..])
                                {
                                    (
                                        icmp_packet.icmp_type,
                                        icmp_packet.identifier,
                                        icmp_packet.sequence_number,
                                    )
                                } else {
                                    (IcmpType::Unknown(0), 0, 0)
                                };

                            if icmp_type == IcmpType::EchoRequest {
                                // Register in NAT table (Thread-safe write)
                                if let Ok(mut table) = self.nat_table.write() {
                                    table.register_echo_request(
                                        icmp_id,
                                        src_ip_from_header,
                                    );
                                }
                                debug!(
                                    "NAT: Registered Echo Request ID {} from {}",
                                    icmp_id, src_ip_from_header
                                );

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

                                    // Extract payload
                                    let payload = &packet[ihl + 8..];

                                    let builder = PacketBuilder::ipv4(
                                        new_src_ip.octets(),
                                        dst_ip.octets(),
                                        60,
                                    )
                                    .icmpv4_echo_request(icmp_id, icmp_seq);
                                    let new_payload = {
                                        let mut frame = Vec::<u8>::with_capacity(
                                            builder.size(payload.len()),
                                        );
                                        builder
                                            .write(&mut frame, payload)
                                            .unwrap();
                                        frame
                                    };

                                    state = PacketState::Send {
                                        out_interface: new_iface,
                                        payload: new_payload,
                                        src_mac: new_src_mac,
                                        dst_mac: gateway_mac,
                                    };
                                    continue 'router_loop;
                                }
                            } else if icmp_type == IcmpType::EchoReply {
                                // ... existing DNAT logic for traversal ...
                                debug!(
                                    "Checking SNAT for Echo Reply ID {}",
                                    icmp_id
                                );
                                // Thread-safe read
                                let is_dnat =
                                    if let Ok(table) = self.nat_table.read() {
                                        table.is_dnat_session(icmp_id)
                                    } else {
                                        false
                                    };

                                if is_dnat {
                                    info!(
                                        "Traversal: Masquerading Echo Reply ID {} from {}",
                                        icmp_id, src_ip_from_header
                                    );

                                    // Change Source IP to Router's External IP (eth_ip)
                                    let new_src_ip = self.config.eth_ip;
                                    let octets = new_src_ip.octets();

                                    // Mutate packet
                                    packet[12] = octets[0];
                                    packet[13] = octets[1];
                                    packet[14] = octets[2];
                                    packet[15] = octets[3];

                                    Self::recalculate_ip_checksum(&mut packet);
                                }
                            }
                        } else if protocol == etherparse::IpNumber::TCP {
                            // TCP SNAT
                            if let Ok(tcp) = TcpHeaderSlice::from_slice(&packet[ihl..]) {
                                let src_port = tcp.source_port();
                                
                                // Record session: External Port (same as src_port) -> Internal IP (src_ip_from_header)
                                if let Ok(mut sessions) = self.nat_sessions.write() {
                                    sessions.insert(src_port, src_ip_from_header);
                                }
                                
                                // Perform Masquerade (SNAT)
                                let new_src_ip = self.config.eth_ip;
                                let octets = new_src_ip.octets();

                                // Update IP Header Source
                                packet[12] = octets[0];
                                packet[13] = octets[1];
                                packet[14] = octets[2];
                                packet[15] = octets[3];

                                // Recalculate IP Checksum
                                Self::recalculate_ip_checksum(&mut packet);

                                // Recalculate TCP Checksum using new Source IP
                                Self::recalculate_l4_checksum(&mut packet, new_src_ip, dst_ip, protocol);
                            }
                        } else if protocol == etherparse::IpNumber::UDP {
                            // UDP SNAT
                             if let Ok(udp) = UdpHeaderSlice::from_slice(&packet[ihl..]) {
                                let src_port = udp.source_port();
                                
                                // Record session
                                if let Ok(mut sessions) = self.nat_sessions.write() {
                                    sessions.insert(src_port, src_ip_from_header);
                                }
                                
                                // Perform Masquerade (SNAT)
                                let new_src_ip = self.config.eth_ip;
                                let octets = new_src_ip.octets();

                                // Update IP Header Source
                                packet[12] = octets[0];
                                packet[13] = octets[1];
                                packet[14] = octets[2];
                                packet[15] = octets[3];

                                // Recalculate IP Checksum
                                Self::recalculate_ip_checksum(&mut packet);

                                // Recalculate UDP Checksum
                                Self::recalculate_l4_checksum(&mut packet, new_src_ip, dst_ip, protocol);
                            }
                        }
                    }

                    // Thread-safe read for ARP
                    let dst_mac_opt = if new_iface == InterfaceType::Tun {
                        Some([0u8; 6])
                    } else if let Ok(table) = self.arp_table.read() {
                        table.get_mac(&new_dst_ip, new_iface)
                    } else {
                        None
                    };

                    let dst_mac = match dst_mac_opt {
                        Some(mac) => mac,
                        None => {
                            // Determine Source MAC/IP for ARP request
                            let (src_mac, src_ip) = match new_iface {
                                InterfaceType::WiFi => {
                                    (self.config.wifi_mac, self.config.wifi_ip)
                                }
                                InterfaceType::Ethernet => {
                                    (self.config.eth_mac, self.config.eth_ip)
                                }
                                InterfaceType::Acoustic => {
                                    let mut mac = [0u8; 6];
                                    mac[5] = self.config.acoustic_mac;
                                    (mac, self.config.acoustic_ip)
                                }
                                _ => ([0u8; 6], Ipv4Addr::new(0, 0, 0, 0)),
                            };

                            if src_mac != [0u8; 6] {
                                // 1. Buffer packet waiting for ARP
                                // We need to clone the packet because we are moving it into the buffer
                                // and `packet` variable is needed if we were to continue (but here we return).
                                // Actually, we can just move `packet` into the PendingPacket.
                                let pending_pkt = PendingPacket {
                                    interface: new_iface,
                                    packet: packet, // Move packet
                                    src_mac: src_mac,
                                };

                                let should_send_arp = if let Ok(mut pending) =
                                    self.pending_packets.write()
                                {
                                    let queue = pending
                                        .entry(new_dst_ip)
                                        .or_default();
                                    queue.push(pending_pkt);
                                    queue.len() == 1 // Only send ARP if this is the first packet in queue to avoid ARP storm
                                } else {
                                    false
                                };

                                // 2. Send ARP Request if needed
                                if should_send_arp {
                                    let arp_req = self.prepare_arp_request(
                                        src_mac, src_ip, new_dst_ip,
                                    );
                                    match new_iface {
                                        InterfaceType::WiFi => {
                                            if let Err(e) = to_wifi.send(arp_req)
                                            {
                                                warn!(
                                                    "Failed to send ARP request to WiFi: {}",
                                                    e
                                                );
                                            }
                                        }
                                        InterfaceType::Ethernet => {
                                            if let Err(e) = to_eth.send(arp_req)
                                            {
                                                warn!(
                                                    "Failed to send ARP request to Ethernet: {}",
                                                    e
                                                );
                                            }
                                        }
                                        _ => {}
                                    }
                                    info!(
                                        "Sent ARP Request for {} and buffered packet",
                                        new_dst_ip
                                    );
                                } else {
                                    debug!(
                                        "Buffered packet for {} (ARP already pending)",
                                        new_dst_ip
                                    );
                                }
                            } else {
                                error!(
                                    "Cannot send ARP request: unknown source MAC/IP for interface {:?}",
                                    new_iface
                                );
                            }

                            // Stop processing this packet, it is either buffered or dropped due to error
                            return;
                        }
                    };

                    state = PacketState::Send {
                        out_interface: new_iface,
                        payload: packet,
                        src_mac: match new_iface {
                            InterfaceType::WiFi => self.config.wifi_mac,
                            InterfaceType::Acoustic => {
                                let mut mac = [0u8; 6];
                                mac[5] = self.config.acoustic_mac;
                                mac
                            }
                            InterfaceType::Ethernet => self.config.eth_mac,
                            InterfaceType::Tun => [0u8; 6],
                        },
                        dst_mac,
                    };
                    continue 'router_loop;
                }
                PacketState::Send {
                    out_interface,
                    payload,
                    src_mac,
                    dst_mac,
                } => {
                    debug!(
                        "Sending to {:?}, len={}",
                        out_interface,
                        payload.len()
                    );
                    match out_interface {
                        InterfaceType::Acoustic => {
                            if let Err(e) =
                                to_acoustic.send((payload.clone(), dst_mac[5]))
                            {
                                warn!(
                                    "Failed to send packet to Acoustic thread: {}",
                                    e
                                );
                            }
                            if let Err(e) = to_tun.send(payload) {
                                warn!(
                                    "Failed to send packet to TUN thread: {}",
                                    e
                                );
                            }
                        }
                        InterfaceType::WiFi => {
                            if let Err(e) =
                                to_wifi.send(self.build_ethernet_frame(
                                    src_mac,
                                    dst_mac,
                                    payload.as_slice(),
                                ))
                            {
                                warn!(
                                    "Failed to send packet to WiFi thread: {}",
                                    e
                                );
                            }
                        }
                        InterfaceType::Ethernet => {
                            if let Err(e) =
                                to_eth.send(self.build_ethernet_frame(
                                    src_mac,
                                    dst_mac,
                                    payload.as_slice(),
                                ))
                            {
                                warn!(
                                    "Failed to send packet to Ethernet thread: {}",
                                    e
                                );
                            }
                        }
                        InterfaceType::Tun => {
                            info!(
                                "Routing packet to TUN (len={})",
                                payload.len()
                            );
                            if let Err(e) = to_tun.send(payload) {
                                warn!(
                                    "Failed to send packet to TUN thread: {}",
                                    e
                                );
                            }
                        }
                    }
                    return;
                }
                PacketState::Dropped { reason } => {
                    debug!("Packet dropped: {}", reason);
                    return;
                }
            }
        }
    }
    /// Stop the router
    pub fn stop(&self) {
        self.running
            .lock()
            .unwrap()
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