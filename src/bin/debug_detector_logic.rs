use std::fs::File;
use std::io::BufReader;
use trackmaker_rs::amodem::{config::Configuration, common, detect::Detector};

fn main() -> std::io::Result<()> {
    let config = Configuration::bitrate_1();
    
    // 读取测试文件
    let file = File::open("tmp/fresh_digits.pcm")?;
    let mut reader = BufReader::new(file);
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut data)?;
    
    // 转换为样本
    let samples = common::loads(&data);
    println!("📁 Original samples: {}", samples.len());
    
    // 跳过开头静音（模拟Python的skip_start）
    let skip_start = (0.1 * 8000.0) as usize; // config.skip_start * Fs
    let samples_after_skip = &samples[skip_start..];
    println!("⏭️  After skip_start: {} samples", samples_after_skip.len());
    
    // 模拟我们检测器的逻辑
    println!("\n🔍 Debugging detector logic step by step:");
    
    let detector = Detector::new(&config);
    
    // 手动调用wait_for_carrier来看看它消耗了多少样本
    println!("🎯 Calling wait_for_carrier...");
    
    // 我们需要创建一个简单的载波检测来了解消耗情况
    let nsym = config.nsym;
    let mut samples_consumed = 0;
    let mut found_carrier = false;
    
    // 简单的载波搜索（模拟）
    for (offset, chunk) in samples_after_skip.chunks(nsym).enumerate() {
        samples_consumed = (offset + 1) * nsym;
        
        // 简单检测：如果有非零样本就认为找到了载波
        let has_signal = chunk.iter().any(|&x| x.abs() > 0.1);
        if has_signal {
            println!("🎯 Found carrier at offset {} (sample {})", offset, samples_consumed);
            found_carrier = true;
            break;
        }
        
        if offset > 1000 { // 避免无限循环
            break;
        }
    }
    
    if !found_carrier {
        println!("❌ No carrier found");
        return Ok(());
    }
    
    println!("📊 Samples consumed by carrier detection: {}", samples_consumed);
    println!("📊 Remaining samples after carrier detection: {}", samples_after_skip.len() - samples_consumed);
    
    // 现在运行真正的检测器
    let (signal, amplitude, freq_error) = detector.run(samples_after_skip.iter().cloned()).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    
    println!("\n🎯 Actual detector results:");
    println!("   Signal length: {} samples", signal.len());
    println!("   Signal symbols: {} symbols", signal.len() / nsym);
    println!("   Amplitude: {:.3}", amplitude);
    println!("   Frequency error: {:.6}", freq_error);
    
    // 分析信号的不同部分
    println!("\n🔍 Signal content analysis:");
    
    // 前20个样本
    println!("First 20 samples: {:?}", &signal[..20.min(signal.len())]);
    
    // 中间20个样本
    if signal.len() > 40 {
        let mid = signal.len() / 2;
        println!("Middle 20 samples: {:?}", &signal[mid..mid+20.min(signal.len()-mid)]);
    }
    
    // 后20个样本
    if signal.len() > 20 {
        println!("Last 20 samples: {:?}", &signal[signal.len()-20..]);
    }
    
    // 检查唯一模式
    let mut unique_patterns = std::collections::HashSet::new();
    for chunk in signal.chunks(nsym) {
        if chunk.len() == nsym {
            let rounded: Vec<i32> = chunk.iter().map(|&x| (x * 1000.0).round() as i32).collect();
            unique_patterns.insert(rounded);
        }
    }
    
    println!("\n📈 Unique {}-sample patterns: {}", nsym, unique_patterns.len());
    
    // 比较期望值
    println!("\n📊 Comparison with Python:");
    println!("   Python: 9689 samples (1211 symbols)");
    println!("   Rust:   {} samples ({} symbols)", signal.len(), signal.len() / nsym);
    println!("   Ratio:  {:.2}x", signal.len() as f64 / 9689.0);
    
    Ok(())
}
