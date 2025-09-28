use trackmaker_rs::amodem::config::Configuration;
use num_complex::Complex64;

fn main() {
    let config = Configuration::bitrate_1();
    
    println!("Testing modulation process:");
    println!("Symbols: {:?}", config.symbols);
    
    // Get the carrier
    let carrier = &config.carriers[0];
    println!("Carrier (first 8): {:?}", &carrier[..8]);
    
    // Test modulation of both symbols
    for (i, &symbol) in config.symbols.iter().enumerate() {
        println!("\nTesting symbol {}: {:?}", i, symbol);
        
        // Modulate: signal[k] = symbol * carrier[k] / nfreq
        let modulated: Vec<Complex64> = carrier.iter()
            .map(|&c| symbol * c / 1.0) // nfreq = 1 for BITRATE=1
            .collect();
        
        println!("Modulated complex: {:?}", &modulated[..8]);
        
        // Take real part (as sender does)
        let real_signal: Vec<f64> = modulated.iter().map(|c| c.re).collect();
        println!("Real part: {:?}", &real_signal[..8]);
        
        // Take imaginary part (what if we used this instead?)
        let imag_signal: Vec<f64> = modulated.iter().map(|c| c.im).collect();
        println!("Imaginary part: {:?}", &imag_signal[..8]);
    }
    
    println!("\n🔍 Analysis:");
    println!("Our symbols are 0-1j and 0+1j (pure imaginary)");
    println!("When multiplied with carrier, the real part is always 0!");
    println!("We should be using the imaginary part for pure imaginary symbols.");
}
