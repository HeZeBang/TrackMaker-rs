//! Router module for forwarding IP packets between interfaces
//!
//! This module implements a simple static router that forwards IP packets
//! between an acoustic interface (to NODE1) and a WiFi interface (to NODE3).

use etherparse::Ipv4HeaderSlice;
use pcap::{Active, Capture};
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::audio::recorder::AppShared;
use crate::mac::ip_interface::IpInterface;
use crate::phy::{FrameType, LineCodingKind};

/// Network interface type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterfaceType {
    /// Acoustic interface (to NODE1)
    Acoustic,
    /// WiFi interface (to NODE3)
    WiFi,
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
    pub fn new(network: Ipv4Addr, mask: Ipv4Addr, interface: InterfaceType) -> Self {
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
            if (net_octets[i] & mask_octets[i]) != (ip_octets[i] & mask_octets[i]) {
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

    /// Lookup the interface for a destination IP
    pub fn lookup(&self, dest_ip: &Ipv4Addr) -> Option<InterfaceType> {
        for route in &self.routes {
            if route.network.contains(dest_ip) {
                return Some(route.network.interface);
            }
        }
        None
    }
}

/// ARP table for WiFi interface (maps IP to MAC address)
pub struct WiFiArpTable {
    table: HashMap<Ipv4Addr, [u8; 6]>,
}

impl WiFiArpTable {
    pub fn new() -> Self {
        Self {
            table: HashMap::new(),
        }
    }

    /// Add a static ARP entry
    pub fn add_entry(&mut self, ip: Ipv4Addr, mac: [u8; 6]) {
        self.table.insert(ip, mac);
    }

    /// Get MAC address for an IP
    pub fn get_mac(&self, ip: &Ipv4Addr) -> Option<[u8; 6]> {
        self.table.get(ip).copied()
    }

    /// Update or add an ARP entry (for learning)
    pub fn update(&mut self, ip: Ipv4Addr, mac: [u8; 6]) {
        self.table.insert(ip, mac);
    }
}

/// Router configuration
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
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            acoustic_ip: "192.168.1.2".parse().unwrap(),
            acoustic_mac: 2,
            wifi_ip: "192.168.2.1".parse().unwrap(),
            wifi_mac: [0x00, 0x00, 0x00, 0x00, 0x00, 0x02],
            wifi_interface: "wlan0".to_string(),
            acoustic_network: "192.168.1.0".parse().unwrap(),
            acoustic_netmask: "255.255.255.0".parse().unwrap(),
            wifi_network: "192.168.2.0".parse().unwrap(),
            wifi_netmask: "255.255.255.0".parse().unwrap(),
        }
    }
}

/// Simple IP Router
pub struct Router {
    config: RouterConfig,
    routing_table: RoutingTable,
    wifi_arp: WiFiArpTable,
    running: Arc<AtomicBool>,
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
            wifi_arp: WiFiArpTable::new(),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Add a static ARP entry for WiFi
    pub fn add_wifi_arp_entry(&mut self, ip: Ipv4Addr, mac: [u8; 6]) {
        self.wifi_arp.add_entry(ip, mac);
    }

    /// Build an Ethernet frame for WiFi transmission
    fn build_ethernet_frame(&self, dest_mac: [u8; 6], ip_packet: &[u8]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(14 + ip_packet.len());

        // Ethernet header (14 bytes)
        frame.extend_from_slice(&dest_mac); // Destination MAC
        frame.extend_from_slice(&self.config.wifi_mac); // Source MAC
        frame.extend_from_slice(&[0x08, 0x00]); // EtherType: IPv4

        // IP packet payload
        frame.extend_from_slice(ip_packet);

        frame
    }

    /// Parse Ethernet frame and extract IP packet
    fn parse_ethernet_frame(frame: &[u8]) -> Option<(Vec<u8>, [u8; 6])> {
        if frame.len() < 14 {
            return None;
        }

        let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
        if ethertype != 0x0800 {
            // Not IPv4
            return None;
        }

        let mut src_mac = [0u8; 6];
        src_mac.copy_from_slice(&frame[6..12]);

        Some((frame[14..].to_vec(), src_mac))
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

    /// Forward a packet from WiFi to Acoustic interface
    fn forward_to_acoustic(
        &self,
        acoustic_interface: &mut IpInterface,
        mut ip_packet: Vec<u8>,
        dest_ip: Ipv4Addr,
    ) -> Result<(), String> {
        // Decrement TTL
        Self::decrement_ttl(&mut ip_packet).map_err(|e| e.to_string())?;

        // Get destination MAC from acoustic ARP table
        use crate::net::arp::ArpTable;
        let arp = ArpTable::new();
        let dest_mac = arp
            .get_mac(&dest_ip)
            .ok_or_else(|| format!("No ARP entry for {}", dest_ip))?;

        info!(
            "Forwarding packet to acoustic interface: {} -> MAC {}",
            dest_ip, dest_mac
        );

        acoustic_interface.send_packet(&ip_packet, dest_mac, FrameType::Data)
    }

    /// Forward a packet from Acoustic to WiFi interface
    fn forward_to_wifi(
        &self,
        wifi_capture: &mut Capture<Active>,
        mut ip_packet: Vec<u8>,
        dest_ip: Ipv4Addr,
    ) -> Result<(), String> {
        // Decrement TTL
        Self::decrement_ttl(&mut ip_packet).map_err(|e| e.to_string())?;

        // Get destination MAC from WiFi ARP table
        let dest_mac = self
            .wifi_arp
            .get_mac(&dest_ip)
            .ok_or_else(|| format!("No WiFi ARP entry for {}", dest_ip))?;

        info!(
            "Forwarding packet to WiFi interface: {} -> MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            dest_ip, dest_mac[0], dest_mac[1], dest_mac[2], dest_mac[3], dest_mac[4], dest_mac[5]
        );

        // Build Ethernet frame
        let frame = self.build_ethernet_frame(dest_mac, &ip_packet);

        // Send via pcap
        wifi_capture
            .sendpacket(frame)
            .map_err(|e| format!("Failed to send WiFi packet: {}", e))
    }

    /// Check if packet is for us (router itself)
    fn is_for_us(&self, dest_ip: &Ipv4Addr) -> bool {
        *dest_ip == self.config.acoustic_ip || *dest_ip == self.config.wifi_ip
    }

    /// Run the router
    pub fn run(
        &mut self,
        shared: AppShared,
        sample_rate: u32,
        line_coding: LineCodingKind,
    ) -> Result<(), String> {
        self.running.store(true, Ordering::SeqCst);

        info!("Starting router...");
        info!(
            "Acoustic interface: {} (MAC {})",
            self.config.acoustic_ip, self.config.acoustic_mac
        );
        info!(
            "WiFi interface: {} on {}",
            self.config.wifi_ip, self.config.wifi_interface
        );

        // Open WiFi capture
        let wifi_device = crate::net::pcap_utils::get_device_by_name(&self.config.wifi_interface)
            .map_err(|e| format!("Failed to get WiFi device: {}", e))?;

        let mut wifi_capture = crate::net::pcap_utils::open_capture(wifi_device)
            .map_err(|e| format!("Failed to open WiFi capture: {}", e))?;

        // Set filter to only capture IP packets
        wifi_capture
            .filter("icmp", true)
            .map_err(|e| format!("Failed to set filter: {}", e))?;

        // Create acoustic interface
        let mut acoustic_interface = IpInterface::new(
            shared.clone(),
            sample_rate,
            line_coding,
            self.config.acoustic_mac,
        );

        info!("Router is running. Press Ctrl+C to stop.");

        // Main routing loop
        // We need to poll both interfaces
        let running = self.running.clone();

        // For simplicity, we'll use a single-threaded approach with non-blocking receives
        // In production, you'd want separate threads for each interface

        while running.load(Ordering::SeqCst) {
            // Check WiFi interface (non-blocking)
            match wifi_capture.next_packet() {
                Ok(packet) => {
                    if let Some((ip_packet, src_mac)) = Self::parse_ethernet_frame(packet.data) {
                        self.handle_wifi_packet(
                            &mut acoustic_interface,
                            ip_packet,
                            src_mac,
                        );
                    }
                }
                Err(pcap::Error::TimeoutExpired) => {
                    // No packet available, continue
                }
                Err(e) => {
                    warn!("WiFi capture error: {}", e);
                }
            }

            // Check acoustic interface (non-blocking with short timeout)
            match acoustic_interface.receive_packet(Some(Duration::from_millis(150))) {
                Ok(ip_packet) => {
                    self.handle_acoustic_packet(&mut wifi_capture, ip_packet);
                }
                Err(_) => {
                    // Timeout or error, continue
                }
            }
        }

        info!("Router stopped.");
        Ok(())
    }

    /// Handle a packet received from WiFi interface
    fn handle_wifi_packet(
        &self,
        acoustic_interface: &mut IpInterface,
        ip_packet: Vec<u8>,
        _src_mac: [u8; 6],
    ) {
        // Parse IP header
        let ip_header = match Ipv4HeaderSlice::from_slice(&ip_packet) {
            Ok(h) => h,
            Err(e) => {
                debug!("Failed to parse IP header from WiFi: {}", e);
                return;
            }
        };

        let src_ip = Ipv4Addr::from(ip_header.source());
        let dest_ip = Ipv4Addr::from(ip_header.destination());

        debug!(
            "WiFi packet: {} -> {} (proto: {:?})",
            src_ip,
            dest_ip,
            ip_header.protocol()
        );

        // Learn source MAC
        // (In a real router, this would be more sophisticated)

        // Check if packet is for us
        if self.is_for_us(&dest_ip) {
            debug!("Packet is for router itself, ignoring (let host stack handle)");
            return;
        }

        // Lookup routing table
        match self.routing_table.lookup(&dest_ip) {
            Some(InterfaceType::Acoustic) => {
                // Forward to acoustic interface
                if let Err(e) =
                    self.forward_to_acoustic(acoustic_interface, ip_packet, dest_ip)
                {
                    warn!("Failed to forward to acoustic: {}", e);
                }
            }
            Some(InterfaceType::WiFi) => {
                // Same interface, shouldn't happen in normal routing
                debug!("Packet destination is on same WiFi network, ignoring");
            }
            None => {
                debug!("No route to {}, dropping packet", dest_ip);
            }
        }
    }

    /// Handle a packet received from acoustic interface
    fn handle_acoustic_packet(&self, wifi_capture: &mut Capture<Active>, ip_packet: Vec<u8>) {
        // Parse IP header
        let ip_header = match Ipv4HeaderSlice::from_slice(&ip_packet) {
            Ok(h) => h,
            Err(e) => {
                debug!("Failed to parse IP header from acoustic: {}", e);
                return;
            }
        };

        let src_ip = Ipv4Addr::from(ip_header.source());
        let dest_ip = Ipv4Addr::from(ip_header.destination());

        debug!(
            "Acoustic packet: {} -> {} (proto: {:?})",
            src_ip,
            dest_ip,
            ip_header.protocol()
        );

        // Check if packet is for us
        if self.is_for_us(&dest_ip) {
            debug!("Packet is for router itself, ignoring");
            return;
        }

        // Lookup routing table
        match self.routing_table.lookup(&dest_ip) {
            Some(InterfaceType::WiFi) => {
                // Forward to WiFi interface
                if let Err(e) = self.forward_to_wifi(wifi_capture, ip_packet, dest_ip) {
                    warn!("Failed to forward to WiFi: {}", e);
                }
            }
            Some(InterfaceType::Acoustic) => {
                // Same interface, shouldn't happen in normal routing
                debug!("Packet destination is on same acoustic network, ignoring");
            }
            None => {
                debug!("No route to {}, dropping packet", dest_ip);
            }
        }
    }

    /// Stop the router
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direct_network_contains() {
        let net = DirectNetwork::new(
            "192.168.1.0".parse().unwrap(),
            "255.255.255.0".parse().unwrap(),
            InterfaceType::Acoustic,
        );

        assert!(net.contains(&"192.168.1.1".parse().unwrap()));
        assert!(net.contains(&"192.168.1.254".parse().unwrap()));
        assert!(!net.contains(&"192.168.2.1".parse().unwrap()));
        assert!(!net.contains(&"10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn test_routing_table_lookup() {
        let mut table = RoutingTable::new();
        table.add_direct_network(
            "192.168.1.0".parse().unwrap(),
            "255.255.255.0".parse().unwrap(),
            InterfaceType::Acoustic,
        );
        table.add_direct_network(
            "192.168.2.0".parse().unwrap(),
            "255.255.255.0".parse().unwrap(),
            InterfaceType::WiFi,
        );

        assert_eq!(
            table.lookup(&"192.168.1.5".parse().unwrap()),
            Some(InterfaceType::Acoustic)
        );
        assert_eq!(
            table.lookup(&"192.168.2.100".parse().unwrap()),
            Some(InterfaceType::WiFi)
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
