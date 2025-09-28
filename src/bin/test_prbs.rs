use trackmaker_rs::amodem::dsp::Prbs;
use num_complex::Complex64;

fn main() {
    let mut prbs = Prbs::new(1, 0x1100b, 2);
    let constellation = [
        Complex64::new(1.0, 0.0),    // 0
        Complex64::new(0.0, 1.0),    // 1  
        Complex64::new(-1.0, 0.0),   // 2
        Complex64::new(0.0, -1.0),   // 3
    ];
    
    println!("PRBS first 20 values:");
    for i in 0..20 {
        let val = prbs.next().unwrap() as usize;
        println!("  {}: {} -> constellation[{}] = {:?}", i, val, val, constellation[val]);
    }
}
