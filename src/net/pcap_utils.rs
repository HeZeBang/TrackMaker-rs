use pcap::{Active, Capture, Device};
use tracing::info;
use std::error::Error;

pub fn list_devices() -> Result<Vec<Device>, Box<dyn Error>> {
    let devices = Device::list()?;
    Ok(devices)
}

pub fn get_device_by_name(name: &str) -> Result<Device, Box<dyn Error>> {
    let devices = Device::list()?;
    for device in devices {
        if device.name == name {
            info!("Using device: {:?}", device);
            return Ok(device);
        }
    }
    Err(format!("Device {} not found", name).into())
}

// Open a capture on a device
pub fn open_capture(device: Device) -> Result<Capture<Active>, Box<dyn Error>> {
    let mut cap = Capture::from_device(device)?
        .promisc(true) // Promiscuous mode: capture all packets on the network
        .snaplen(65535) // Maximum packet size to capture
        .immediate_mode(true)
        .open()?; // Open the capture
    // cap.filter("icmp", true).unwrap();
    info!("Capture opened successfully");
    Ok(cap)
}

// Send a packet
pub fn send_packet(
    cap: &mut Capture<Active>,
    packet: &[u8],
) -> Result<(), Box<dyn Error>> {
    cap.sendpacket(packet)?;
    Ok(())
}

// Get the next packet from the capture
pub fn next_packet<'a>(
    cap: &'a mut Capture<Active>,
) -> Result<pcap::Packet<'a>, Box<dyn Error>> {
    match cap.next_packet() {
        Ok(packet) => {
            info!("Packet received: {:?}", packet);
            Ok(packet)
        },
        Err(e) => Err(Box::new(e)),
    }
}
