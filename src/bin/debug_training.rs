use trackmaker_rs::amodem::{config::Configuration, dsp::Prbs};
use num_complex::Complex64;

fn main() {
    let config = Configuration::bitrate_1();
    let length = 200;
    let constant_prefix = 16;
    let mut prbs = Prbs::new(1, 0x1100b, 2);
    let constellation = [
        Complex64::new(1.0, 0.0),    // 0: 1
        Complex64::new(0.0, 1.0),    // 1: 1j
        Complex64::new(-1.0, 0.0),   // 2: -1
        Complex64::new(0.0, -1.0),   // 3: -1j
    ];
    
    println!("Debug training symbols generation:");
    for i in 0..25 {
        if i < constant_prefix {
            println!("  {}: constant = 1+0j", i);
        } else {
            let prbs_val = prbs.next().unwrap();
            let idx = prbs_val as usize;
            let symbol = constellation[idx];
            println!("  {}: PRBS={} -> constellation[{}] = {:?}", i, prbs_val, idx, symbol);
        }
    }
}
