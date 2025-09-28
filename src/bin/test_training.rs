use trackmaker_rs::amodem::{config::Configuration, equalizer::Equalizer};

fn main() {
    let config = Configuration::bitrate_1();
    let equalizer = Equalizer::new(&config);
    
    let symbols = equalizer.train_symbols(200, &config);
    let signal = equalizer.modulator(&symbols);
    
    println!("Rust training signal length: {}", signal.len());
    println!("Rust training signal first 20 samples: {:?}", &signal[..20]);
    
    let max_val = signal.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    let min_val = signal.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let rms = (signal.iter().map(|&x| x * x).sum::<f64>() / signal.len() as f64).sqrt();
    
    println!("Rust training signal max/min: {} {}", max_val, min_val);
    println!("Rust training signal RMS: {}", rms);
}
