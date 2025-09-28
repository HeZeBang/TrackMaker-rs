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
    
    // Use detector
    let detector = Detector::new(&config);
    let (signal, amplitude, freq_error) = detector.run(samples.into_iter()).unwrap();
    
    println!("After detector: {} samples", signal.len());
    println!("Amplitude: {}, Freq error: {}", amplitude, freq_error);
    
    // The detector output should start from where the carrier was detected
    // Let's check what the signal looks like at different positions
    
    println!("\nTesting different skip amounts on detector output:");
    let test_skips = [0, 400, 800, 1200, 1600, 2000, 2400, 2800, 3200];
    
    for &skip in &test_skips {
        if skip >= signal.len() {
            println!("  Skip {}: Not enough samples", skip);
            continue;
        }
        
        let test_signal = &signal[skip..];
        if test_signal.len() < 80 {
            println!("  Skip {}: Too few samples remaining", skip);
            continue;
        }
        
        // Test first few symbol periods
        let nsym = 8;
        let omega = 2.0 * std::f64::consts::PI * 2000.0 / 8000.0;
        
        let mut symbols = Vec::new();
        for chunk in test_signal.chunks(nsym).take(10) {
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
        
        // Check for variation
        let unique_symbols: std::collections::HashSet<_> = symbols.iter()
            .map(|s| ((s.re * 1000.0) as i32, (s.im * 1000.0) as i32))
            .collect();
        
        println!("  Skip {}: {} unique patterns, first symbol: {:.3}+{:.3}i", 
                 skip, unique_symbols.len(), 
                 symbols.get(0).map_or(0.0, |s| s.re),
                 symbols.get(0).map_or(0.0, |s| s.im));
        
        if unique_symbols.len() > 1 {
            println!("    ✅ Found variation at skip {}", skip);
        }
    }
    
    Ok(())
}
