use trackmaker_rs::amodem::config::Configuration;

fn main() {
    let config = Configuration::bitrate_1();
    
    println!("Rust BITRATE=1 config:");
    println!("  symbols: {:?}", config.symbols);
    println!("  carriers[0]: {:?}", config.carriers[0]);
    println!("  nsym: {}", config.nsym);
    println!("  baud: {}", config.baud);
    println!("  nfreq: {}", config.nfreq);
    println!("  fs: {}", config.fs);
    println!("  modem_bps: {}", config.modem_bps);
    println!("  npoints: {}", config.npoints);
}
