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
    
    println!("Original samples: {}", samples.len());
    
    // Test 1: Direct access to samples at position 7200 (our known good position)
    println!("\n=== Test 1: Direct access at position 7200 ===");
    let direct_signal = &samples[7200..];
    let nsym = 8;
    let omega = 2.0 * std::f64::consts::PI * 2000.0 / 8000.0;
    
    let mut direct_symbols = Vec::new();
    for chunk in direct_signal.chunks(nsym).take(10) {
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
            
            direct_symbols.push(correlation);
        }
    }
    
    println!("Direct symbols (first 5):");
    for (i, symbol) in direct_symbols.iter().take(5).enumerate() {
        println!("  Symbol {}: {:.3} + {:.3}i", i, symbol.re, symbol.im);
    }
    
    let unique_direct: std::collections::HashSet<_> = direct_symbols.iter()
        .map(|s| ((s.re * 1000.0) as i32, (s.im * 1000.0) as i32))
        .collect();
    println!("Unique patterns: {}", unique_direct.len());
    
    // Test 2: Through detector
    println!("\n=== Test 2: Through detector ===");
    let detector = Detector::new(&config);
    let (detector_signal, amplitude, freq_error) = detector.run(samples.into_iter()).unwrap();
    
    println!("Detector output: {} samples", detector_signal.len());
    println!("Amplitude: {}, Freq error: {}", amplitude, freq_error);
    
    // Check if the detector signal contains our expected data
    // If data was at 7200 in original and detector detected at ~4000, 
    // then data should be at 7200-4000=3200 in detector output
    let detector_data_pos = 3200;
    
    if detector_data_pos < detector_signal.len() {
        let detector_test_signal = &detector_signal[detector_data_pos..];
        
        let mut detector_symbols = Vec::new();
        for chunk in detector_test_signal.chunks(nsym).take(10) {
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
                
                detector_symbols.push(correlation);
            }
        }
        
        println!("Detector symbols at pos {} (first 5):", detector_data_pos);
        for (i, symbol) in detector_symbols.iter().take(5).enumerate() {
            println!("  Symbol {}: {:.3} + {:.3}i", i, symbol.re, symbol.im);
        }
        
        let unique_detector: std::collections::HashSet<_> = detector_symbols.iter()
            .map(|s| ((s.re * 1000.0) as i32, (s.im * 1000.0) as i32))
            .collect();
        println!("Unique patterns: {}", unique_detector.len());
    } else {
        println!("Detector signal too short for expected data position");
    }
    
    // Test 3: Check raw detector signal samples
    println!("\n=== Test 3: Raw detector signal analysis ===");
    println!("First 20 samples of detector output:");
    for (i, &sample) in detector_signal.iter().take(20).enumerate() {
        print!("{:.3} ", sample);
        if (i + 1) % 8 == 0 {
            println!();
        }
    }
    println!();
    
    println!("Last 20 samples of detector output:");
    let start = detector_signal.len().saturating_sub(20);
    for (i, &sample) in detector_signal[start..].iter().enumerate() {
        print!("{:.3} ", sample);
        if (i + 1) % 8 == 0 {
            println!();
        }
    }
    println!();
    
    Ok(())
}
