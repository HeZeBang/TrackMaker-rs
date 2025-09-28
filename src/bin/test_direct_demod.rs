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
    println!("Total samples: {}", samples.len());
    
    // Try different skip amounts to find the data
    let skip_amounts = [
        (400 * 8, "400ms"),
        (500 * 8, "500ms"), 
        (600 * 8, "600ms"),
        (700 * 8, "700ms"),
        (800 * 8, "800ms"),
        (900 * 8, "900ms"),
        (1000 * 8, "1000ms"),
        (1100 * 8, "1100ms"),
    ];
    
    let receiver = Receiver::new(&config);
    
    for &(skip_samples, desc) in &skip_amounts {
        println!("\n=== Testing skip {} ({}) ===", skip_samples, desc);
        
        if samples.len() <= skip_samples {
            println!("Not enough samples!");
            continue;
        }
        
        let data_signal = &samples[skip_samples..];
        
        // Test just the first few symbols
        let test_symbols = receiver.debug_demodulate(&data_signal[..80.min(data_signal.len())], 1.0).unwrap();
        
        println!("First 5 symbols:");
        for (i, symbol) in test_symbols.iter().take(5).enumerate() {
            println!("  Symbol {}: {:.3} + {:.3}i (magnitude: {:.3})", 
                     i, symbol.re, symbol.im, symbol.norm());
        }
        
        // Check if we have variation
        let unique_symbols: std::collections::HashSet<_> = test_symbols.iter()
            .map(|s| ((s.re * 1000.0) as i32, (s.im * 1000.0) as i32))
            .collect();
        
        println!("Unique symbol patterns: {}", unique_symbols.len());
        
        if unique_symbols.len() > 1 {
            println!("🎉 Found variation at skip {} ({})", skip_samples, desc);
            
            // Full decode test
            let symbols = receiver.debug_demodulate(data_signal, 1.0).unwrap();
            let bit_tuples = receiver.get_modem().decode(symbols);
            let bits: Vec<bool> = bit_tuples.into_iter()
                .flat_map(|tuple| tuple.into_iter())
                .take(80)
                .collect();
            
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
            
            println!("Decoded bytes: {:?}", bytes);
            println!("As ASCII: {:?}", String::from_utf8_lossy(&bytes));
            break;
        }
    }
    
    Ok(())
}
