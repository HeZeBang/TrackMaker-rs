use trackmaker_rs::amodem::{config::Configuration, dsp::Modem};
use num_complex::Complex64;

fn main() {
    let config = Configuration::bitrate_1();
    let modem = Modem::new(config.symbols.clone());
    
    println!("🔧 Rust MODEM configuration:");
    println!("   Symbols: {:?}", config.symbols);
    println!("   Bits per symbol: {}", modem.bits_per_symbol());
    
    // 测试编码映射
    println!("\n🔄 Testing encode mapping:");
    let test_bits = vec![
        vec![false],
        vec![true],
    ];
    
    for bits in &test_bits {
        let encoded = modem.encode(bits.iter().cloned());
        println!("   {:?} -> {:?}", bits, encoded);
    }
    
    // 测试解码映射
    println!("\n🔄 Testing decode mapping:");
    let test_symbols = vec![
        Complex64::new(0.0, -1.0),  // 0-1j
        Complex64::new(0.0, 1.0),   // 0+1j
    ];
    
    for symbol in &test_symbols {
        let decoded = modem.decode(vec![*symbol]);
        println!("   {:?} -> {:?}", symbol, decoded);
    }
    
    // 测试我们从接收器中看到的实际符号
    println!("\n🦀 Testing actual received symbols:");
    let received_symbols = vec![
        Complex64::new(-0.000, 1.000),  // -0.000 + 1.000i
        Complex64::new(0.000, -1.000),  // 0.000 + -1.000i
    ];
    
    for symbol in &received_symbols {
        let decoded = modem.decode(vec![*symbol]);
        println!("   {:.3} + {:.3}i -> {:?}", symbol.re, symbol.im, decoded);
    }
    
    // 模拟一个完整的比特序列
    println!("\n🧪 Testing bit sequence for '0123456789':");
    
    // ASCII '0' = 0x30 = 48 = 00110000 (LSB first: 00001100)
    // ASCII '1' = 0x31 = 49 = 00110001 (LSB first: 10001100)
    let ascii_0 = 48u8;
    let ascii_1 = 49u8;
    
    println!("ASCII '0' ({}): binary = {:08b}", ascii_0, ascii_0);
    println!("ASCII '1' ({}): binary = {:08b}", ascii_1, ascii_1);
    
    // 转换为LSB优先的比特
    let mut bits_0 = Vec::new();
    let mut bits_1 = Vec::new();
    
    for i in 0..8 {
        bits_0.push((ascii_0 & (1 << i)) != 0);
        bits_1.push((ascii_1 & (1 << i)) != 0);
    }
    
    println!("LSB-first bits for '0': {:?}", bits_0);
    println!("LSB-first bits for '1': {:?}", bits_1);
    
    // 编码为符号
    let symbols_0 = modem.encode(bits_0.iter().cloned());
    let symbols_1 = modem.encode(bits_1.iter().cloned());
    
    println!("Symbols for '0': {:?}", symbols_0);
    println!("Symbols for '1': {:?}", symbols_1);
    
    // 解码回比特
    let decoded_0: Vec<bool> = modem.decode(symbols_0).into_iter().flatten().collect();
    let decoded_1: Vec<bool> = modem.decode(symbols_1).into_iter().flatten().collect();
    
    println!("Decoded bits for '0': {:?}", decoded_0);
    println!("Decoded bits for '1': {:?}", decoded_1);
    
    // 转换回字节
    let mut byte_0 = 0u8;
    let mut byte_1 = 0u8;
    
    for (i, &bit) in decoded_0.iter().enumerate() {
        if bit {
            byte_0 |= 1 << i;
        }
    }
    
    for (i, &bit) in decoded_1.iter().enumerate() {
        if bit {
            byte_1 |= 1 << i;
        }
    }
    
    println!("Reconstructed byte for '0': {} ('{}')", byte_0, byte_0 as char);
    println!("Reconstructed byte for '1': {} ('{}')", byte_1, byte_1 as char);
}
