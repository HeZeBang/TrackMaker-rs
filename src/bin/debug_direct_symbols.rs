use std::fs::File;
use std::io::BufReader;
use trackmaker_rs::amodem::{config::Configuration, common, recv::Receiver};
use num_complex::Complex64;

fn main() -> std::io::Result<()> {
    let config = Configuration::bitrate_1();
    
    // Read the Python-generated PCM file
    let file = File::open("tmp/python_digits.pcm")?;
    let mut reader = BufReader::new(file);
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut data)?;
    
    // Convert PCM data to float samples
    let samples = common::loads(&data);
    
    // Skip to data position (we know this works from test_direct_demod)
    let skip_samples = 7200;
    let data_signal = &samples[skip_samples..];
    
    println!("Data signal length: {} samples", data_signal.len());
    
    // Create receiver
    let receiver = Receiver::new(&config);
    let nsym = 8; // config.nsym
    let omega = 2.0 * std::f64::consts::PI * 2000.0 / 8000.0; // 2kHz carrier
    
    // Manual demodulation (bypass the internal skip logic)
    let mut symbols = Vec::new();
    
    for chunk in data_signal.chunks(nsym) {
        if chunk.len() == nsym {
            let scaled_chunk: Vec<f64> = chunk.iter().map(|&x| x * 1.0).collect(); // gain = 1.0
            
            // Create reference carrier (conjugate for demodulation)
            let carrier: Vec<Complex64> = (0..nsym).map(|i| {
                let phase = -omega * i as f64;
                Complex64::new(phase.cos(), phase.sin())
            }).collect();
            
            // Correlate with carrier
            let mut correlation = Complex64::new(0.0, 0.0);
            for (i, &sample) in scaled_chunk.iter().enumerate() {
                correlation += sample * carrier[i];
            }
            
            // Normalize by symbol length
            correlation /= nsym as f64;
            
            // Scale to match expected symbol amplitude
            correlation *= 2.0;
            
            symbols.push(correlation);
        }
    }
    
    println!("Extracted {} symbols", symbols.len());
    
    // Show first 20 symbols
    println!("\nFirst 20 symbols:");
    for (i, symbol) in symbols.iter().take(20).enumerate() {
        println!("  Symbol {}: {:.3} + {:.3}i (magnitude: {:.3})", 
                 i, symbol.re, symbol.im, symbol.norm());
    }
    
    // Check for variation
    let unique_symbols: std::collections::HashSet<_> = symbols.iter().take(50)
        .map(|s| ((s.re * 1000.0) as i32, (s.im * 1000.0) as i32))
        .collect();
    
    println!("\nUnique symbol patterns in first 50: {}", unique_symbols.len());
    
    if unique_symbols.len() > 1 {
        println!("✅ Found symbol variation!");
        
        // Decode symbols
        let bit_tuples = receiver.get_modem().decode(symbols.clone());
        let bits: Vec<bool> = bit_tuples.into_iter()
            .flat_map(|tuple| tuple.into_iter())
            .take(80)
            .collect();
        
        println!("\nFirst 80 bits:");
        for chunk in bits.chunks(8) {
            let bit_str: String = chunk.iter().map(|&b| if b { '1' } else { '0' }).collect();
            print!("{} ", bit_str);
        }
        println!();
        
        // Convert to bytes
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
        
        println!("\nFirst 10 bytes: {:?}", &bytes[..10.min(bytes.len())]);
        println!("As ASCII: {:?}", String::from_utf8_lossy(&bytes[..10.min(bytes.len())]));
        
    } else {
        println!("❌ No symbol variation found");
    }
    
    Ok(())
}
