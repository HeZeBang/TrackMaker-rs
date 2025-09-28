use std::fs::File;
use std::io::BufReader;
use trackmaker_rs::amodem::{config::Configuration, common, recv::Receiver};

fn main() -> std::io::Result<()> {
    let config = Configuration::bitrate_1();
    
    // Read the Python-generated PCM file
    let file = File::open("tmp/python_digits.pcm")?;
    let mut reader = BufReader::new(file);
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut data)?;
    
    // Convert PCM data to float samples
    let samples = common::loads(&data);
    
    // Skip to data position (we found this is at 7200 samples)
    let skip_samples = 7200;
    let data_signal = &samples[skip_samples..];
    
    // Create receiver and extract symbols
    let receiver = Receiver::new(&config);
    let symbols = receiver.debug_demodulate(data_signal, 1.0).unwrap();
    
    println!("Extracted {} symbols", symbols.len());
    
    // Decode symbols to bits
    let bit_tuples = receiver.get_modem().decode(symbols);
    let bits: Vec<bool> = bit_tuples.into_iter()
        .flat_map(|tuple| tuple.into_iter())
        .collect();
    
    println!("Total bits: {}", bits.len());
    
    // Show first 200 bits in groups of 8
    println!("\nFirst 200 bits (in bytes):");
    for (i, chunk) in bits.chunks(8).take(25).enumerate() {
        let bit_str: String = chunk.iter().map(|&b| if b { '1' } else { '0' }).collect();
        
        // Convert to byte
        let mut byte = 0u8;
        for (j, &bit) in chunk.iter().enumerate() {
            if bit {
                byte |= 1 << j;
            }
        }
        
        println!("  Byte {}: {} = {} ({})", i, bit_str, byte, byte as char);
    }
    
    // Try to decode as raw bytes
    let mut bytes = Vec::new();
    for chunk in bits.chunks(8) {
        if chunk.len() == 8 {
            let mut byte = 0u8;
            for (i, &bit) in chunk.iter().enumerate() {
                if bit {
                    byte |= 1 << i;
                }
            }
            bytes.push(byte);
        }
    }
    
    println!("\nFirst 50 bytes as ASCII:");
    let ascii_str = String::from_utf8_lossy(&bytes[..50.min(bytes.len())]);
    println!("{:?}", ascii_str);
    
    println!("\nFirst 50 bytes as hex:");
    for (i, &byte) in bytes.iter().take(50).enumerate() {
        if i % 16 == 0 {
            print!("\n{:04x}: ", i);
        }
        print!("{:02x} ", byte);
    }
    println!();
    
    Ok(())
}
