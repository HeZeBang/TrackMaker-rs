use std::fs::File;
use std::io::BufReader;
use trackmaker_rs::amodem::{config::Configuration, common, detect::Detector, recv::Receiver};

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
    let (detector_signal, amplitude, freq_error) = detector.run(samples.into_iter()).unwrap();
    
    println!("Detector output: {} samples", detector_signal.len());
    println!("Amplitude: {}, Freq error: {}", amplitude, freq_error);
    
    // Take the last portion of the signal where we see the pattern
    let data_start = detector_signal.len().saturating_sub(1000);
    let data_signal = &detector_signal[data_start..];
    
    println!("Using last {} samples starting at position {}", data_signal.len(), data_start);
    
    // Extract symbols manually
    let nsym = 8;
    let omega = 2.0 * std::f64::consts::PI * 2000.0 / 8000.0;
    let gain = 1.0 / amplitude;
    
    let mut symbols = Vec::new();
    for chunk in data_signal.chunks(nsym) {
        if chunk.len() == nsym {
            let scaled_chunk: Vec<f64> = chunk.iter().map(|&x| x * gain).collect();
            
            let carrier: Vec<num_complex::Complex64> = (0..nsym).map(|i| {
                let phase = -omega * i as f64;
                num_complex::Complex64::new(phase.cos(), phase.sin())
            }).collect();
            
            let mut correlation = num_complex::Complex64::new(0.0, 0.0);
            for (i, &sample) in scaled_chunk.iter().enumerate() {
                correlation += sample * carrier[i];
            }
            correlation /= nsym as f64;
            correlation *= 2.0;
            
            symbols.push(correlation);
        }
    }
    
    println!("Extracted {} symbols", symbols.len());
    
    // Show symbols and their raw signal
    println!("\nFirst 10 symbols and their raw signals:");
    for (i, chunk) in data_signal.chunks(nsym).take(10).enumerate() {
        if chunk.len() == nsym {
            println!("  Symbol {}: {:?} -> {:.3} + {:.3}i", 
                     i, chunk, symbols[i].re, symbols[i].im);
        }
    }
    
    // Decode symbols
    let receiver = Receiver::new(&config);
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
    
    // Convert to bytes and try to decode
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
    println!("As ASCII: {:?}", String::from_utf8_lossy(&bytes[..10.min(bytes.len())]));
    
    // Check what symbol this raw pattern corresponds to
    println!("\nAnalyzing the repeating pattern [1.0, 0.0, -1.0, 0.0, 1.0, 0.0, -1.0, 0.0]:");
    let test_pattern = [1.0, 0.0, -1.0, 0.0, 1.0, 0.0, -1.0, 0.0];
    
    let carrier: Vec<num_complex::Complex64> = (0..nsym).map(|i| {
        let phase = -omega * i as f64;
        num_complex::Complex64::new(phase.cos(), phase.sin())
    }).collect();
    
    let mut test_correlation = num_complex::Complex64::new(0.0, 0.0);
    for (i, &sample) in test_pattern.iter().enumerate() {
        test_correlation += sample * carrier[i];
    }
    test_correlation /= nsym as f64;
    test_correlation *= 2.0;
    
    println!("  Correlation result: {:.3} + {:.3}i", test_correlation.re, test_correlation.im);
    
    // Test what this decodes to
    let test_decoded = receiver.get_modem().decode(vec![test_correlation]);
    println!("  Decodes to bit: {}", test_decoded[0][0]);
    
    Ok(())
}
