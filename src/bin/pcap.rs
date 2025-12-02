use etherparse::{NetSlice, SlicedPacket};
use pcap::{Capture, Device};
use tracing::{debug, info, error};
use trackmaker_rs::utils::logging::init_logging;

// Note: Run this with SUDO!

fn main() {
    init_logging();
    info!("Starting packet capture example...");
    let main_device = Device::lookup()
        .unwrap()
        .unwrap();
    info!("Using device: {}", main_device.name);
    let mut cap = Capture::from_device(main_device)
        .unwrap()
        .promisc(true)
        .snaplen(5000)
        .immediate_mode(true)
        .open()
        .unwrap();
    cap.filter("icmp", true).unwrap();
    info!("Capture opened, listening for ICMP packets...");

    while let Ok(packet) = cap.next_packet() {
        debug!("received packet! {:?}", packet);
        match SlicedPacket::from_ethernet(packet.data) {
            Ok(sliced) => {
                info!("Link: {:?}\nLink_exts: {:?}\nNet: {:?}\nTransport: {:?}", sliced.link, sliced.link_exts, sliced.net, sliced.transport);
            }
            Err(e) => {
                error!("Failed to parse packet: {:?}", e);
            }
        }
    }
}
