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
    
    // 解码符号到比特
    let bit_tuples = receiver.get_modem().decode(data_symbols);
    let bits: Vec<bool> = bit_tuples.into_iter()
        .flat_map(|tuple| tuple.into_iter())
        .collect();
    
    println!("\n🔢 Decoded {} bits from symbols", bits.len());
    
    // 分析比特流，寻找ASCII模式
    println!("\n🔍 Analyzing bit stream for ASCII patterns:");
    
    // 尝试不同的起始偏移量
    for start_offset in 0..std::cmp::min(bits.len(), 80) {
        if start_offset % 8 != 0 {
            continue; // 只检查字节对齐的偏移量
        }
        
        let mut found_readable = false;
        let mut text = String::new();
        
        // 尝试解码8个字节
        for byte_idx in 0..8 {
            let bit_start = start_offset + byte_idx * 8;
            if bit_start + 8 > bits.len() {
                break;
            }
            
            let byte_bits = &bits[bit_start..bit_start + 8];
            let mut byte = 0u8;
            
            for (i, &bit) in byte_bits.iter().enumerate() {
                if bit {
                    byte |= 1 << i;
                }
            }
            
            if byte >= 32 && byte <= 126 {
                text.push(byte as char);
                found_readable = true;
            } else if byte == 0 {
                text.push('.');
            } else {
                text.push('?');
                found_readable = false;
                break;
            }
        }
        
        if found_readable && text.len() >= 3 {
            println!("  Offset {}: {:?}", start_offset / 8, text);
            
            // 如果找到了可读文本，显示更多详细信息
            if text.contains("0123") || text.contains("Hello") {
                println!("    🎯 Found target pattern!");
                
                // 显示这个偏移量的比特和字节
                for byte_idx in 0..std::cmp::min(10, (bits.len() - start_offset) / 8) {
                    let bit_start = start_offset + byte_idx * 8;
                    let byte_bits = &bits[bit_start..bit_start + 8];
                    let mut byte = 0u8;
                    
                    for (i, &bit) in byte_bits.iter().enumerate() {
                        if bit {
                            byte |= 1 << i;
                        }
                    }
                    
                    let bit_str: String = byte_bits.iter().map(|&b| if b { '1' } else { '0' }).collect();
                    let char_repr = if byte >= 32 && byte <= 126 { format!("'{}'", byte as char) } else { "?".to_string() };
                    
                    println!("    Byte {}: {} = 0x{:02x} ({}) {}", byte_idx, bit_str, byte, byte, char_repr);
                }
                break;
            }
        }
    }
    
    // 如果没找到明显的模式，显示原始数据
    if bits.len() >= 80 {
        println!("\n📊 Raw bit analysis (first 80 bits):");
        for i in (0..80).step_by(8) {
            let byte_bits = &bits[i..i + 8];
            let mut byte = 0u8;
            
            for (j, &bit) in byte_bits.iter().enumerate() {
                if bit {
                    byte |= 1 << j;
                }
            }
            
            let bit_str: String = byte_bits.iter().map(|&b| if b { '1' } else { '0' }).collect();
            let char_repr = if byte >= 32 && byte <= 126 { format!("'{}'", byte as char) } else { "?".to_string() };
            
            println!("  Byte {}: {} = 0x{:02x} ({}) {}", i / 8, bit_str, byte, byte, char_repr);
        }
    }
    
    Ok(())
}
