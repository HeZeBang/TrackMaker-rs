pub mod arp;
pub mod icmp;
pub mod ip;
pub mod pcap_utils;
pub mod router;

pub enum Protocol {
    Icmp = 1,
    Tcp = 6,
    Udp = 17,
    Unknown = 255,
}

impl From<u8> for Protocol {
    fn from(value: u8) -> Self {
        match value {
            1 => Protocol::Icmp,
            6 => Protocol::Tcp,
            17 => Protocol::Udp,
            _ => Protocol::Unknown,
        }
    }
}

impl Into<u8> for Protocol {
    fn into(self) -> u8 {
        self as u8
    }
}
