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
    
    // Create receiver and extract symbols
    // Note: debug_demodulate has its own skip logic, so we pass the full signal
    let receiver = Receiver::new(&config);
    let symbols = receiver.debug_demodulate(&samples, 1.0).unwrap();
    
    println!("Symbol constellation: {:?}", config.symbols);
    println!("Expected mapping:");
    println!("  false -> {:?}", config.symbols[0]);
    println!("  true  -> {:?}", config.symbols[1]);
    
    println!("\nFirst 20 extracted symbols and their decoding:");
    for (i, symbol) in symbols.iter().take(20).enumerate() {
        // Decode this single symbol
        let decoded_bits = receiver.get_modem().decode(vec![*symbol]);
        let bit = decoded_bits[0][0]; // Get the single bit
        
        println!("  Symbol {}: {:.3} + {:.3}i -> {} (closest to {:?})", 
                 i, symbol.re, symbol.im, bit, 
                 if bit { config.symbols[1] } else { config.symbols[0] });
    }
    
    // Let's also test the mapping directly
    println!("\nDirect constellation mapping test:");
    for (bit_val, &expected_symbol) in config.symbols.iter().enumerate() {
        let decoded = receiver.get_modem().decode(vec![expected_symbol]);
        let result_bit = decoded[0][0];
        println!("  Input {:?} -> decoded bit: {} (expected: {})", 
                 expected_symbol, result_bit, bit_val == 1);
    }
    
    // Test some variations
    println!("\nTesting symbol variations:");
    let test_symbols = [
        (0.0, 1.0),   // 0+1j
        (0.0, -1.0),  // 0-1j  
        (1.0, 0.0),   // 1+0j
        (-1.0, 0.0),  // -1+0j
    ];
    
    for &(re, im) in &test_symbols {
        let test_symbol = num_complex::Complex64::new(re, im);
        let decoded = receiver.get_modem().decode(vec![test_symbol]);
        let bit = decoded[0][0];
        println!("  {:+.1}{:+.1}j -> bit: {}", re, im, bit);
    }
    
    Ok(())
}
