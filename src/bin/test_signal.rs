use trackmaker_rs::amodem::{config::Configuration, send::Sender};
use std::io::Cursor;

fn main() {
    let config = Configuration::bitrate_1();
    let mut output = Vec::new();
    let cursor = Cursor::new(&mut output);
    let mut sender = Sender::new(cursor, &config, 1.0);
    
    // Generate start sequence
    sender.start().unwrap();
    
    // Convert first 100 samples (200 bytes) to i16
    let samples: Vec<i16> = output[..200].chunks(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    
    println!("Rust first 20 samples: {:?}", &samples[..20]);
    println!("Rust samples shape: {}", samples.len());
    let min_val = samples.iter().min().unwrap();
    let max_val = samples.iter().max().unwrap();
    println!("Rust samples range: {} to {}", min_val, max_val);
}
