use tun;

fn main() {
    let mut config = tun::Configuration::default();
    #[cfg(target_os = "linux")]
    config.platform(|config| {
        config.packet_information(false);
    });
}
