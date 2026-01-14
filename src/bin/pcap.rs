use std::net::Ipv4Addr;

use etherparse::{
    ArpHardwareId, ArpOperation, ArpPacket, EtherType, NetSlice, PacketBuilder,
    SlicedPacket,
};
use pcap::{Capture, Device, Linktype};
use tracing::{debug, error, info};
use trackmaker_rs::utils::logging::init_logging;

// Note: Run this with SUDO!

fn main() {
    init_logging();
    info!("Starting packet capture example...");
    let main_device =
        trackmaker_rs::net::pcap_utils::get_device_by_name("wlan0").unwrap();
    info!("Using device: {}", main_device.name);
    let mut cap = Capture::from_device(main_device)
        .unwrap()
        .promisc(true)
        .snaplen(5000)
        .immediate_mode(true)
        .open()
        .unwrap();
    cap.filter("icmp", true)
        .unwrap();
    info!("Capture opened, listening for ICMP packets...");
    {
        // 2. 构造 ARP 请求帧
        let source_mac = [0x9c, 0x29, 0x76, 0x0c, 0x49, 0x00]; // 改成你的接口 MAC
        let target_mac = [0xff; 6]; // broadcast

        let sender_ip = Ipv4Addr::new(10, 42, 0, 1); // 改成你的 IP
        let target_ip = Ipv4Addr::new(10, 42, 0, 2); // 想查询的 IP

        // 使用 PacketBuilder 构造 Ethernet + ARP 帧
        let builder = PacketBuilder::ethernet2(source_mac, target_mac).arp(
            ArpPacket::new(
                ArpHardwareId::ETHERNET,
                EtherType::IPV4,
                ArpOperation::REQUEST,
                &source_mac,         // sender_hw_addr
                &sender_ip.octets(), // sender_protocol_addr
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

        println!("Built ARP request, len = {}", result.len());

        // 3. 通过 pcap 发送 (inject) raw packet
        cap.sendpacket(result)
            .unwrap();

        println!("ARP request sent to {}", target_ip);
    }

    while let Ok(packet) = cap.next_packet() {
        debug!("received packet! {:?}", packet);
        match SlicedPacket::from_ethernet(packet.data) {
            Ok(sliced) => {
                info!(
                    "Link: {:?}\nLink_exts: {:?}\nNet: {:?}\nTransport: {:?}",
                    sliced.link, sliced.link_exts, sliced.net, sliced.transport
                );
            }
            Err(e) => {
                error!("Failed to parse packet: {:?}", e);
            }
        }
    }
}
