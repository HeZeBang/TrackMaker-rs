use trackmaker_rs::amodem::config::Configuration;
use num_complex::Complex64;

fn main() {
    let config = Configuration::bitrate_1();
    
    println!("Carrier analysis:");
    println!("  Frequency: {} Hz", config.frequencies[0]);
    println!("  Fs: {} Hz", config.fs);
    println!("  nsym: {} samples", config.nsym);
    
    // Generate carrier like in sender
    let omega_send = 2.0 * std::f64::consts::PI * config.frequencies[0] / config.fs;
    println!("  omega_send: {}", omega_send);
    
    let carrier_send: Vec<Complex64> = (0..config.nsym).map(|i| {
        let phase = omega_send * i as f64; // Positive for modulation
        Complex64::new(phase.cos(), phase.sin())
    }).collect();
    
    // Generate carrier like in receiver (should be conjugate)
    let omega_recv = 2.0 * std::f64::consts::PI * config.frequencies[0] / config.fs;
    let carrier_recv: Vec<Complex64> = (0..config.nsym).map(|i| {
        let phase = -omega_recv * i as f64; // Negative for demodulation
        Complex64::new(phase.cos(), phase.sin())
    }).collect();
    
    println!("\nSender carrier (first 8 samples):");
    for (i, c) in carrier_send.iter().enumerate() {
        println!("  {}: {:.3} + {:.3}i", i, c.re, c.im);
    }
    
    println!("\nReceiver carrier (first 8 samples):");
    for (i, c) in carrier_recv.iter().enumerate() {
        println!("  {}: {:.3} + {:.3}i", i, c.re, c.im);
    }
    
    // Test correlation with itself
    let mut self_corr = Complex64::new(0.0, 0.0);
    for i in 0..config.nsym {
        self_corr += carrier_send[i] * carrier_recv[i];
    }
    self_corr /= config.nsym as f64;
    
    println!("\nSelf correlation: {:.3} + {:.3}i (should be close to 1.0)", 
             self_corr.re, self_corr.im);
    
    // Test what happens when we multiply by a symbol
    let symbol_0_minus_1j = Complex64::new(0.0, -1.0); // false bit
    let symbol_0_plus_1j = Complex64::new(0.0, 1.0);   // true bit
    
    println!("\nTesting symbol modulation and demodulation:");
    
    // Modulate symbol_0_minus_1j
    let modulated_signal: Vec<f64> = carrier_send.iter()
        .map(|&c| (symbol_0_minus_1j * c).re)
        .collect();
    
    // Demodulate
    let mut demod_result = Complex64::new(0.0, 0.0);
    for i in 0..config.nsym {
        demod_result += modulated_signal[i] * carrier_recv[i];
    }
    demod_result /= config.nsym as f64;
    
    println!("  Symbol 0-1j -> modulated -> demodulated: {:.3} + {:.3}i", 
             demod_result.re, demod_result.im);
    
    // Modulate symbol_0_plus_1j
    let modulated_signal2: Vec<f64> = carrier_send.iter()
        .map(|&c| (symbol_0_plus_1j * c).re)
        .collect();
    
    // Demodulate
    let mut demod_result2 = Complex64::new(0.0, 0.0);
    for i in 0..config.nsym {
        demod_result2 += modulated_signal2[i] * carrier_recv[i];
    }
    demod_result2 /= config.nsym as f64;
    
    println!("  Symbol 0+1j -> modulated -> demodulated: {:.3} + {:.3}i", 
             demod_result2.re, demod_result2.im);
}
