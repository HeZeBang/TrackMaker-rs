use std::fs::File;
use std::io::BufReader;
use trackmaker_rs::amodem::{config::Configuration, common, recv::Receiver, detect::Detector};

fn main() -> std::io::Result<()> {
    let config = Configuration::bitrate_1();
    
    // 读取Python生成的测试文件
    let file = File::open("tmp/fresh_digits.pcm")?;
    let mut reader = BufReader::new(file);
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut data)?;
    
    // 转换为样本
    let samples = common::loads(&data);
    println!("📁 Read {} samples from PCM file", samples.len());
    
    // 跳过开头静音
    let skip_start = (0.1 * 8000.0) as usize;
    let samples_after_skip = &samples[skip_start..];
    
    // 运行检测器
    let detector = Detector::new(&config);
    let (signal, amplitude, freq_error) = detector.run(samples_after_skip.iter().cloned()).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    
    println!("🎯 Detector: {} samples, amplitude={:.3}, freq_error={:.6}", signal.len(), amplitude, freq_error);
    
    // 创建接收器并解调
    let receiver = Receiver::new(&config);
    let symbols = receiver.debug_demodulate(&signal, 1.0 / amplitude).unwrap();
    
    println!("🔧 Extracted {} symbols", symbols.len());
    
    // 跳过训练序列
    let training_skip = 550;
    let data_symbols = if symbols.len() > training_skip {
        symbols[training_skip..].to_vec()
    } else {
        symbols
    };
    
    println!("📈 Data symbols after training skip: {}", data_symbols.len());
    
    // 显示前10个数据符号
    println!("\n🔍 First 10 data symbols:");
    for (i, sym) in data_symbols.iter().take(10).enumerate() {
        println!("  Data[{}]: {:.3} + {:.3}i", i, sym.re, sym.im);
    }
    
    // 解码符号到比特
    let bit_tuples = receiver.get_modem().decode(data_symbols);
    let bits: Vec<bool> = bit_tuples.into_iter()
        .flat_map(|tuple| tuple.into_iter())
        .collect();
    
    println!("\n🔢 Decoded {} bits from symbols", bits.len());
    
    // 显示前80个比特
    println!("First 80 bits (in groups of 8):");
    for (i, chunk) in bits.chunks(8).take(10).enumerate() {
        let bit_str: String = chunk.iter().map(|&b| if b { '1' } else { '0' }).collect();
        print!("  Byte {}: {} ", i, bit_str);
        
        // 转换为字节值（LSB优先）
        if chunk.len() == 8 {
            let mut byte = 0u8;
            for (j, &bit) in chunk.iter().enumerate() {
                if bit {
                    byte |= 1 << j;
                }
            }
            print!("= 0x{:02x} ({})", byte, byte);
            if byte >= 32 && byte <= 126 {
                print!(" '{}'", byte as char);
            }
        }
        println!();
    }
    
    // 转换比特到字节
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
    
    println!("\n📊 Converted to {} bytes", bytes.len());
    println!("First 20 bytes: {:02x?}", &bytes[..20.min(bytes.len())]);
    
    // 尝试直接作为ASCII解码
    let ascii_attempt = String::from_utf8_lossy(&bytes[..20.min(bytes.len())]);
    println!("Direct ASCII interpretation: {:?}", ascii_attempt);
    
    // 查找可能的数据模式
    println!("\n🔍 Looking for '0123456789' pattern:");
    let target_bytes = b"0123456789";
    println!("Target bytes: {:02x?}", target_bytes);
    
    // 在解码的字节中搜索
    for start in 0..bytes.len().saturating_sub(10) {
        let slice = &bytes[start..start+10];
        let text = String::from_utf8_lossy(slice);
        if text.contains("012") || text.contains("123") {
            println!("  Found potential match at offset {}: {:02x?} = {:?}", start, slice, text);
        }
    }
    
    // 检查是否字节序有问题
    println!("\n🔄 Trying different bit interpretations:");
    
    // MSB优先解释
    let mut msb_bytes = Vec::new();
    for chunk in bits.chunks(8).take(10) {
        if chunk.len() == 8 {
            let mut byte = 0u8;
            for (i, &bit) in chunk.iter().enumerate() {
                if bit {
                    byte |= 1 << (7 - i); // MSB优先
                }
            }
            msb_bytes.push(byte);
        }
    }
    println!("MSB-first interpretation: {:02x?}", msb_bytes);
    println!("MSB as ASCII: {:?}", String::from_utf8_lossy(&msb_bytes));
    
    Ok(())
}
