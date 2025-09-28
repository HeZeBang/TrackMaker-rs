use std::fs::File;
use std::io::BufReader;
use trackmaker_rs::amodem::{config::Configuration, common};
use num_complex::Complex64;

fn main() -> std::io::Result<()> {
    let config = Configuration::bitrate_1();
    
    // Read the Python-generated PCM file
    let file = File::open("tmp/simple_test.pcm")?;
    let mut reader = BufReader::new(file);
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut data)?;
    
    // Convert PCM data to float samples
    let samples = common::loads(&data);
    println!("Total samples: {}", samples.len());
    
    // Analyze different parts of the signal
    let nsym = config.nsym;
    println!("Symbol period (nsym): {} samples", nsym);
    
    // Check the first few symbols (should be training/prefix)
    println!("\nFirst 10 symbol periods:");
    for i in 0..10 {
        let start = i * nsym;
        let end = (i + 1) * nsym;
        if end <= samples.len() {
            let symbol_samples = &samples[start..end];
            let avg = symbol_samples.iter().sum::<f64>() / symbol_samples.len() as f64;
            let max = symbol_samples.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
            let min = symbol_samples.iter().fold(f64::INFINITY, |a, &b| a.min(b));
            println!("  Symbol {}: avg={:.3}, min={:.3}, max={:.3}", i, avg, min, max);
        }
    }
    
    // Test carrier correlation at different positions
    let omega = 2.0 * std::f64::consts::PI * 2000.0 / 8000.0; // 2kHz carrier
    let carrier: Vec<Complex64> = (0..nsym).map(|i| {
        let phase = -omega * i as f64;
        Complex64::new(phase.cos(), phase.sin())
    }).collect();
    
    println!("\nCarrier correlation test (first 20 symbols):");
    for i in 0..20 {
        let start = i * nsym;
        let end = (i + 1) * nsym;
        if end <= samples.len() {
            let symbol_samples = &samples[start..end];
            
            let mut correlation = Complex64::new(0.0, 0.0);
            for (j, &sample) in symbol_samples.iter().enumerate() {
                correlation += sample * carrier[j];
            }
            correlation /= nsym as f64;
            
            println!("  Symbol {}: {:.3} + {:.3}i (mag: {:.3})", 
                     i, correlation.re, correlation.im, correlation.norm());
        }
    }
    
    // Try different skip amounts
    println!("\nTesting different skip amounts:");
    for skip_symbols in [0, 100, 200, 250, 300, 400, 500] {
        let skip_samples = skip_symbols * nsym;
        if skip_samples + nsym <= samples.len() {
            let symbol_samples = &samples[skip_samples..skip_samples + nsym];
            
            let mut correlation = Complex64::new(0.0, 0.0);
            for (j, &sample) in symbol_samples.iter().enumerate() {
                correlation += sample * carrier[j];
            }
            correlation /= nsym as f64;
            
            println!("  Skip {} symbols: {:.3} + {:.3}i (mag: {:.3})", 
                     skip_symbols, correlation.re, correlation.im, correlation.norm());
        }
    }
    
    // Analyze symbols starting from 500
    println!("\nAnalyzing symbols starting from 500:");
    for i in 500..520 {
        let start = i * nsym;
        let end = (i + 1) * nsym;
        if end <= samples.len() {
            let symbol_samples = &samples[start..end];
            
            let mut correlation = Complex64::new(0.0, 0.0);
            for (j, &sample) in symbol_samples.iter().enumerate() {
                correlation += sample * carrier[j];
            }
            correlation /= nsym as f64;
            
            println!("  Symbol {}: {:.3} + {:.3}i (mag: {:.3})", 
                     i, correlation.re, correlation.im, correlation.norm());
        }
    }
    
    Ok(())
}
