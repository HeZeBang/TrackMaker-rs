use std::fs::File;
use std::io::BufReader;
use trackmaker_rs::amodem::{
    config::Configuration,
    detect::Detector,
    recv::Receiver,
    common,
};

fn main() -> std::io::Result<()> {
    let config = Configuration::bitrate_1();
    
    // Read the Python-generated PCM file
    let file = File::open("tmp/python_test.pcm")?;
    let mut reader = BufReader::new(file);
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut data)?;
    
    // Convert PCM data to float samples
    let samples = common::loads(&data);
    println!("Total samples: {}", samples.len());
    println!("Duration: {:.3} seconds", samples.len() as f64 / config.fs);
    
    // Detect carrier
    let detector = Detector::new(&config);
    let (signal, amplitude, freq_error) = detector.run(samples.into_iter()).unwrap();
    
    println!("Signal after detection: {} samples", signal.len());
    println!("Amplitude: {:.3}, Freq error: {:.6}", amplitude, freq_error);
    
    // Create receiver and extract symbols manually
    let receiver = Receiver::new(&config);
    
    // Manually call demodulate_basic to see the symbols
    let symbols = receiver.debug_demodulate(&signal, 1.0).unwrap();
    
    println!("Extracted {} symbols:", symbols.len());
    for (i, symbol) in symbols.iter().take(20).enumerate() {
        println!("  Symbol {}: {:.3} + {:.3}i (magnitude: {:.3})", 
                 i, symbol.re, symbol.im, symbol.norm());
    }
    
    // Try to decode symbols
    let bit_tuples = receiver.get_modem().decode(symbols);
    println!("Decoded {} bit tuples:", bit_tuples.len());
    
    let bits: Vec<bool> = bit_tuples.into_iter()
        .flat_map(|tuple| tuple.into_iter())
        .take(40) // Show first 40 bits
        .collect();
    
    println!("First 40 bits: {:?}", bits);
    
    // Convert to bytes and show
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
    
    println!("First {} bytes: {:?}", bytes.len(), bytes);
    println!("As ASCII: {:?}", String::from_utf8_lossy(&bytes));
    
    Ok(())
}
