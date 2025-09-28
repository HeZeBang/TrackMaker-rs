use std::fs::File;
use std::io::BufReader;
use trackmaker_rs::amodem::{config::Configuration, common};

fn main() -> std::io::Result<()> {
    let config = Configuration::bitrate_1();
    
    // Read the Python-generated PCM file
    let file = File::open("tmp/python_digits.pcm")?;
    let mut reader = BufReader::new(file);
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut data)?;
    
    // Convert PCM data to float samples
    let samples = common::loads(&data);
    println!("Total samples: {}", samples.len());
    
    let nsym = config.nsym;
    
    // Look at raw signal values starting from where data should begin
    // Carrier detection skips 500 symbols, then we need to skip 550 more
    let data_start = 550 * nsym;
    
    println!("\nRaw signal values starting from data (first 20 symbols):");
    for i in 0..20 {
        let start = data_start + i * nsym;
        let end = start + nsym;
        if end <= samples.len() {
            let symbol_samples = &samples[start..end];
            println!("  Symbol {}: {:?}", i, symbol_samples);
        }
    }
    
    // Check if there are any variations
    println!("\nChecking for variations in the data section:");
    let mut unique_values = std::collections::HashSet::new();
    for i in 500..600 { // Check 100 symbols
        let start = i * nsym;
        let end = start + nsym;
        if end <= samples.len() {
            let symbol_samples = &samples[start..end];
            // Round to 3 decimal places to group similar values
            let rounded: Vec<i32> = symbol_samples.iter()
                .map(|&x| (x * 1000.0).round() as i32)
                .collect();
            unique_values.insert(rounded);
        }
    }
    
    println!("Found {} unique symbol patterns:", unique_values.len());
    for (i, pattern) in unique_values.iter().take(5).enumerate() {
        println!("  Pattern {}: {:?}", i, pattern.iter().take(8).collect::<Vec<_>>());
    }
    
    Ok(())
}
