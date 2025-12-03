use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};

/// NAT Table for tracking ICMP Echo requests
#[derive(Debug, Clone)]
pub struct NatTable {
    /// Map from ICMP Identifier to Source IP
    /// We use the identifier to map replies back to the original sender
    icmp_map: Arc<Mutex<HashMap<u16, Ipv4Addr>>>,
}

impl NatTable {
    pub fn new() -> Self {
        Self {
            icmp_map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register an outgoing ICMP Echo Request
    pub fn register_echo_request(&self, identifier: u16, source_ip: Ipv4Addr) {
        let mut map = self.icmp_map.lock().unwrap();
        map.insert(identifier, source_ip);
    }

    /// Look up the original source IP for an incoming ICMP Echo Reply
    /// Returns Some(original_ip) if found
    pub fn translate_echo_reply(&self, identifier: u16) -> Option<Ipv4Addr> {
        let map = self.icmp_map.lock().unwrap();
        map.get(&identifier).copied()
    }
}
