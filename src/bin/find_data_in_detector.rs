use std::fs::File;
use std::io::BufReader;
use trackmaker_rs::amodem::{config::Configuration, common, detect::Detector};

fn main() -> std::io::Result<()> {
    let config = Configuration::bitrate_1();
    
    // Read the Python-generated PCM file
    let file = File::open("tmp/python_digits.pcm")?;
    let mut reader = BufReader::new(file);
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut data)?;
    
    // Convert PCM data to float samples
    let samples = common::loads(&data);
    
    // Get detector output
    let detector = Detector::new(&config);
    let (detector_signal, _amplitude, _freq_error) = detector.run(samples.into_iter()).unwrap();
    
    println!("Detector output: {} samples", detector_signal.len());
    
    // Search for the data pattern in detector output
    // We know the data pattern starts with varying symbols
    let nsym = 8;
    let omega = 2.0 * std::f64::consts::PI * 2000.0 / 8000.0;
    
    println!("Searching for data pattern in detector output...");
    
    // Test every 400 samples (50 symbols) to find where variation starts
    let step = 400;
    for start_pos in (0..detector_signal.len()).step_by(step) {
        if start_pos + 80 > detector_signal.len() {
            break;
        }
        
        let test_signal = &detector_signal[start_pos..start_pos + 80];
        
        let mut symbols = Vec::new();
        for chunk in test_signal.chunks(nsym) {
            if chunk.len() == nsym {
                let carrier: Vec<num_complex::Complex64> = (0..nsym).map(|i| {
                    let phase = -omega * i as f64;
                    num_complex::Complex64::new(phase.cos(), phase.sin())
                }).collect();
                
                let mut correlation = num_complex::Complex64::new(0.0, 0.0);
                for (i, &sample) in chunk.iter().enumerate() {
                    correlation += sample * carrier[i];
                }
                correlation /= nsym as f64;
                correlation *= 2.0;
                
                symbols.push(correlation);
            }
        }
        
        if symbols.len() >= 5 {
            let unique_symbols: std::collections::HashSet<_> = symbols.iter()
                .map(|s| ((s.re * 1000.0) as i32, (s.im * 1000.0) as i32))
                .collect();
            
            if unique_symbols.len() > 1 {
                println!("🎯 Found variation at position {}", start_pos);
                println!("  First 5 symbols:");
                for (i, symbol) in symbols.iter().take(5).enumerate() {
                    println!("    Symbol {}: {:.3} + {:.3}i", i, symbol.re, symbol.im);
                }
                println!("  Unique patterns: {}", unique_symbols.len());
                
                // This is our data position!
                break;
            }
        }
    }
    
    // Also check the very end more carefully
    println!("\nChecking the end of detector signal more carefully:");
    let end_positions = [
        detector_signal.len().saturating_sub(1000),
        detector_signal.len().saturating_sub(800),
        detector_signal.len().saturating_sub(600),
        detector_signal.len().saturating_sub(400),
        detector_signal.len().saturating_sub(200),
    ];
    
    for &pos in &end_positions {
        if pos + 80 <= detector_signal.len() {
            let test_signal = &detector_signal[pos..pos + 80];
            
            let mut symbols = Vec::new();
            for chunk in test_signal.chunks(nsym) {
                if chunk.len() == nsym {
                    let carrier: Vec<num_complex::Complex64> = (0..nsym).map(|i| {
                        let phase = -omega * i as f64;
                        num_complex::Complex64::new(phase.cos(), phase.sin())
                    }).collect();
                    
                    let mut correlation = num_complex::Complex64::new(0.0, 0.0);
                    for (i, &sample) in chunk.iter().enumerate() {
                        correlation += sample * carrier[i];
                    }
                    correlation /= nsym as f64;
                    correlation *= 2.0;
                    
                    symbols.push(correlation);
                }
            }
            
            if symbols.len() >= 3 {
                let unique_symbols: std::collections::HashSet<_> = symbols.iter()
                    .map(|s| ((s.re * 1000.0) as i32, (s.im * 1000.0) as i32))
                    .collect();
                
                println!("  Position {} from end: {} unique patterns, first symbol: {:.3}+{:.3}i", 
                         detector_signal.len() - pos, unique_symbols.len(),
                         symbols.get(0).map_or(0.0, |s| s.re),
                         symbols.get(0).map_or(0.0, |s| s.im));
                
                if unique_symbols.len() > 1 {
                    println!("    🎯 Found variation at position {} (from end: {})", 
                             pos, detector_signal.len() - pos);
                }
            }
        }
    }
    
    Ok(())
}
