use etherparse::{NetSlice, PacketBuilder, SlicedPacket};
use pcap::{Capture, Device, Linktype};
use tracing::{debug, error, info};
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
    cap.filter("icmp", true)
        .unwrap();
    info!("Capture opened, listening for ICMP packets...");

    {
        // 1. 准备构造包
        // Ethernet II, Src: Intel_0c:49:00 (9c:29:76:0c:49:00), Dst: IETF-VRRP-VRID_01 (00:00:5e:00:01:01)
        let src_mac = [0x9c, 0x29, 0x76, 0x0c, 0x49, 0x00];
        let dst_mac = [0x00, 0x00, 0x5e, 0x00, 0x01, 0x01];
        let src_ip = [10, 20, 239, 6];
        let dst_ip = [1, 1, 1, 1];

        // 使用 etherparse 构造 ICMPv4 Echo Request, 不带额外 payload（或你也可以加 payload）
        let builder = PacketBuilder::ethernet2(src_mac, dst_mac)
            .ipv4(src_ip, dst_ip, 64) // TTL = 64
            .icmpv4_echo_request(1234 /*id*/, 1 /*seq*/);

        // 如果你想要额外 payload（比如 ping data）：
        let payload: &[u8] = b"hello-icmp";

        // 构造 raw bytes
        let mut packet = Vec::<u8>::with_capacity(builder.size(payload.len()));
        builder.write(&mut packet, payload).unwrap();

        // 2. 用 pcap 打开网卡，并发送这个包
        // 注意：你得选对网卡 device 名称，且通常需要 root 权限
        let device = Device::lookup().unwrap().unwrap();
        let mut cap = Capture::from_device(device)
            .unwrap()
            .immediate_mode(true)
            .open()
            .unwrap();
        // 检查 linktype 是否为 Ethernet
        assert_eq!(cap.get_datalink(), Linktype(1) /* DLT_EN10MB */);

        info!("Sending {} bytes", packet.len());
        cap.sendpacket(packet).unwrap();
        info!("Sent");
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
