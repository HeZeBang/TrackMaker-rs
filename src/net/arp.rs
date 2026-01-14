use std::collections::HashMap;
use std::net::Ipv4Addr;

pub struct ArpTable {
    table: HashMap<Ipv4Addr, u8>,
}

// FIXME: deprecate this in favor of Router's ARP table
impl ArpTable {
    pub fn new() -> Self {
        let mut table = HashMap::new();
        // Static mapping the IP addresses to MAC addresses
        table.insert("192.168.1.1".parse().unwrap(), 1);
        table.insert("192.168.1.2".parse().unwrap(), 2);
        table.insert("192.168.1.3".parse().unwrap(), 3);
        Self { table }
    }

    pub fn get_mac(&self, ip: &Ipv4Addr) -> Option<u8> {
        self.table.get(ip).cloned()
    }

    pub fn get_ip(&self, mac: u8) -> Option<Ipv4Addr> {
        for (ip, m) in &self.table {
            if *m == mac {
                return Some(*ip);
            }
        }
        None
    }
}
