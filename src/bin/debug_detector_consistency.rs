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
    
    // 跳过开头静音（模拟两种调用方式）
    let skip_start = (0.1 * 8000.0) as usize;
    let samples_after_skip = &samples[skip_start..];
    println!("⏭️  After skip_start: {} samples", samples_after_skip.len());
    
    // 测试检测器多次调用的一致性
    println!("\n🔍 Testing detector consistency:");
    
    for run in 1..=5 {
        let detector = Detector::new(&config);
        let (signal, amplitude, freq_error) = detector.run(samples_after_skip.iter().cloned()).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        
        println!("Run {}: {} samples, amp={:.3}, freq_err={:.6}", 
                 run, signal.len(), amplitude, freq_error);
        
        // 检查信号内容的一致性
        if run == 1 {
            println!("  First 10 samples: {:?}", &signal[..10.min(signal.len())]);
            println!("  Last 10 samples: {:?}", &signal[signal.len().saturating_sub(10)..]);
        }
    }
    
    Ok(())
}
