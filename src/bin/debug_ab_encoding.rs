use trackmaker_rs::amodem::{config::Configuration, dsp::Modem, framing};

fn main() {
    let config = Configuration::bitrate_1();
    let modem = Modem::new(config.symbols.clone());
    
    println!("Analyzing 'AB' encoding:");
    
    // Convert "AB" to bytes
    let data = b"AB";
    println!("Input data: {:?}", data);
    println!("As bytes: {:?}", data);
    
    // Convert to bits using framing
    let bits = framing::encode(data);
    println!("After framing: {} bits", bits.len());
    println!("First 20 bits: {:?}", &bits[..20.min(bits.len())]);
    
    // Group bits for symbols (1 bit per symbol for BITRATE=1)
    println!("\nBit to symbol mapping:");
    for (i, &bit) in bits.iter().take(20).enumerate() {
        let symbol = if bit {
            config.symbols[1] // true -> 0+1j
        } else {
            config.symbols[0] // false -> 0-1j
        };
        println!("  Bit {}: {} -> {:?}", i, bit, symbol);
    }
    
    // Check if all bits are the same
    let unique_bits: std::collections::HashSet<_> = bits.iter().collect();
    println!("\nUnique bit values: {:?}", unique_bits);
    
    if unique_bits.len() == 1 {
        println!("All bits are the same! This explains why all symbols are identical.");
        if bits[0] {
            println!("All bits are true -> all symbols are 0+1j");
        } else {
            println!("All bits are false -> all symbols are 0-1j");
        }
    }
}
