use trackmaker_rs::amodem::{config::Configuration, equalizer::Equalizer};

fn main() {
    let config = Configuration::bitrate_1();
    let equalizer = Equalizer::new(&config);
    
    let symbols = equalizer.train_symbols(200, &config);
    
    println!("Train symbols shape: ({}, {})", symbols.len(), symbols[0].len());
    println!("First 5 train symbols:");
    for i in 0..5 {
        println!("  {}: {:?}", i, symbols[i]);
    }
    println!("Train symbols 16-20:");
    for i in 16..21 {
        println!("  {}: {:?}", i, symbols[i]);
    }
}
