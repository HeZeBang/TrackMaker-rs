use std::fs::File;
use std::io::BufReader;
use trackmaker_rs::amodem::{config::Configuration, common, recv::Receiver, detect::Detector};

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
    
    // Use detector like in the real flow
    let detector = Detector::new(&config);
    let (signal, amplitude, freq_error) = detector.run(samples.into_iter()).unwrap();
    
    println!("After detector: {} samples", signal.len());
    println!("Amplitude: {}, Freq error: {}", amplitude, freq_error);
    
    let gain = 1.0 / amplitude;
    
    // Now use the receiver's debug demodulate
    let receiver = Receiver::new(&config);
    let symbols = receiver.debug_demodulate(&signal, gain).unwrap();
    
    println!("Extracted {} symbols from receiver flow", symbols.len());
    
    // Show first 20 symbols
    println!("\nFirst 20 symbols from receiver flow:");
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
        println!("✅ Found symbol variation in receiver flow!");
        
        // Decode and show bits
        let bit_tuples = receiver.get_modem().decode(symbols.clone());
        let bits: Vec<bool> = bit_tuples.into_iter()
            .flat_map(|tuple| tuple.into_iter())
            .take(80)
            .collect();
        
        println!("\nFirst 80 bits:");
        for (i, chunk) in bits.chunks(8).take(10).enumerate() {
            let bit_str: String = chunk.iter().map(|&b| if b { '1' } else { '0' }).collect();
            print!("Byte {}: {} ", i, bit_str);
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
        
        println!("\nFirst 10 bytes: {:02x?}", &bytes[..10.min(bytes.len())]);
        
    } else {
        println!("❌ No symbol variation found in receiver flow");
        
        // Check what symbols we're getting
        let first_few: Vec<_> = symbols.iter().take(5).collect();
        println!("First 5 symbols: {:?}", first_few);
    }
    
    Ok(())
}
