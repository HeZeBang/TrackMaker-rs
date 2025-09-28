use trackmaker_rs::amodem::{config::Configuration, dsp::Modem};
use num_complex::Complex64;

fn main() {
    let config = Configuration::bitrate_1();
    let modem = Modem::new(config.symbols.clone());
    
    println!("BITRATE=1 symbols: {:?}", config.symbols);
    println!("Bits per symbol: {}", modem.bits_per_symbol());
    
    // Test encoding
    println!("\nEncoding test:");
    let test_bits = vec![false, true, false, true];
    let encoded = modem.encode(test_bits.into_iter());
    println!("Bits [false, true, false, true] -> symbols: {:?}", encoded);
    
    // Test decoding
    println!("\nDecoding test:");
    let test_symbols = vec![
        Complex64::new(0.0, -1.0), // 0-1j
        Complex64::new(0.0, 1.0),  // 0+1j
        Complex64::new(0.5, 0.0),  // What we're getting
        Complex64::new(0.0, 0.0),  // Zero
    ];
    
    for symbol in test_symbols {
        let decoded = modem.decode(vec![symbol]);
        println!("Symbol {:?} -> bits: {:?}", symbol, decoded[0]);
    }
    
    // Show the encoding map
    println!("\nEncoding map:");
    // We can't directly access the private encode_map, but we can test all possible bit combinations
    for i in 0..2_usize.pow(modem.bits_per_symbol() as u32) {
        let bits: Vec<bool> = (0..modem.bits_per_symbol())
            .map(|j| (i & (1 << j)) != 0)
            .collect();
        let encoded = modem.encode(bits.clone().into_iter());
        println!("  {:?} -> {:?}", bits, encoded[0]);
    }
}
