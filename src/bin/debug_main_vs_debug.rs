use std::fs::File;
use std::io::BufReader;
use trackmaker_rs::amodem::{config::Configuration, common, recv::Receiver, detect::Detector};

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
    
    println!("\n=== 模拟主接收器流程 ===");
    
    // 主接收器流程（修复后）
    let skip_start = (config.skip_start * config.fs) as usize;
    let samples_after_skip = if samples.len() > skip_start {
        &samples[skip_start..]
    } else {
        &samples[..]
    };
    
    println!("Skip start: {} samples", skip_start);
    println!("Samples after skip: {}", samples_after_skip.len());
    
    let detector = Detector::new(&config);
    let (signal, amplitude, freq_error) = detector.run(samples_after_skip.iter().cloned()).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    
    println!("Detector signal: {} samples", signal.len());
    
    let receiver = Receiver::new(&config);
    let symbols = receiver.debug_demodulate(&signal, 1.0 / amplitude).unwrap();
    
    println!("Main receiver symbols: {}", symbols.len());
    
    println!("\n=== 模拟调试工具流程 ===");
    
    // 调试工具流程
    let skip_start_debug = (0.1 * 8000.0) as usize; // 硬编码的800
    let samples_after_skip_debug = &samples[skip_start_debug..];
    
    println!("Debug skip start: {} samples", skip_start_debug);
    println!("Debug samples after skip: {}", samples_after_skip_debug.len());
    
    let detector_debug = Detector::new(&config);
    let (signal_debug, amplitude_debug, freq_error_debug) = detector_debug.run(samples_after_skip_debug.iter().cloned()).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    
    println!("Debug detector signal: {} samples", signal_debug.len());
    
    let receiver_debug = Receiver::new(&config);
    let symbols_debug = receiver_debug.debug_demodulate(&signal_debug, 1.0 / amplitude_debug).unwrap();
    
    println!("Debug receiver symbols: {}", symbols_debug.len());
    
    println!("\n=== 比较 ===");
    println!("Skip start difference: {} vs {}", skip_start, skip_start_debug);
    println!("Signal length difference: {} vs {}", signal.len(), signal_debug.len());
    println!("Symbol count difference: {} vs {}", symbols.len(), symbols_debug.len());
    
    // 检查config.skip_start的值
    println!("\nConfig values:");
    println!("  skip_start: {}", config.skip_start);
    println!("  fs: {}", config.fs);
    println!("  calculated skip: {}", config.skip_start * config.fs);
    
    Ok(())
}
