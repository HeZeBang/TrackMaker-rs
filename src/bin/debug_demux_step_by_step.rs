use std::fs::File;
use std::io::BufReader;
use trackmaker_rs::amodem::{config::Configuration, common, detect::Detector};
use num_complex::Complex64;

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
    
    // 运行检测器
    let detector = Detector::new(&config);
    let (signal, amplitude, freq_error) = detector.run(samples_after_skip.iter().cloned()).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    
    println!("🎯 Detector result: amplitude={:.3}, freq_error={:.6}", amplitude, freq_error);
    println!("📊 Signal after detector: {} samples", signal.len());
    
    // 检查信号的前几个样本
    println!("\n🔍 First 20 samples from detector:");
    for (i, &sample) in signal.iter().take(20).enumerate() {
        print!("{:.3} ", sample);
        if (i + 1) % 8 == 0 {
            println!();
        }
    }
    println!();
    
    // 检查信号的中间部分
    let mid_start = signal.len() / 2;
    println!("\n🔍 Middle 20 samples from detector (starting at {}):", mid_start);
    for (i, &sample) in signal[mid_start..].iter().take(20).enumerate() {
        print!("{:.3} ", sample);
        if (i + 1) % 8 == 0 {
            println!();
        }
    }
    println!();
    
    // 手动实现Python风格的Demux
    let nsym = config.nsym;
    let omega = 2.0 * std::f64::consts::PI * config.frequencies[0] / config.fs;
    let gain = 1.0 / amplitude;
    
    println!("\n🧮 Demux parameters:");
    println!("   nsym: {}", nsym);
    println!("   omega: {:.6}", omega);
    println!("   gain: {:.3}", gain);
    
    // 创建Python风格的滤波器
    let filter: Vec<Complex64> = (0..nsym).map(|i| {
        let phase = -omega * i as f64;
        let exp_val = Complex64::new(phase.cos(), phase.sin());
        exp_val / (0.5 * nsym as f64)
    }).collect();
    
    println!("\n🔧 Demux filter (first 8):");
    for (i, &f) in filter.iter().take(8).enumerate() {
        println!("   [{}]: {:.6} + {:.6}i", i, f.re, f.im);
    }
    
    // 处理前几个符号周期
    println!("\n🎯 Processing first 10 symbol periods:");
    for (period, chunk) in signal.chunks(nsym).take(10).enumerate() {
        if chunk.len() == nsym {
            // 应用增益
            let scaled_chunk: Vec<f64> = chunk.iter().map(|&x| x * gain).collect();
            
            // 计算相关性
            let mut correlation = Complex64::new(0.0, 0.0);
            for (i, &sample) in scaled_chunk.iter().enumerate() {
                correlation += filter[i] * sample;
            }
            
            println!("  Period {}: {:?} -> {:.3} + {:.3}i (mag: {:.3})", 
                     period, 
                     &scaled_chunk,
                     correlation.re, correlation.im, correlation.norm());
        }
    }
    
    // 处理更多符号来找到变化
    println!("\n🔍 Looking for symbol variation in first 100 periods:");
    let mut unique_symbols = std::collections::HashSet::new();
    let mut all_symbols = Vec::new();
    
    for (period, chunk) in signal.chunks(nsym).take(100).enumerate() {
        if chunk.len() == nsym {
            let scaled_chunk: Vec<f64> = chunk.iter().map(|&x| x * gain).collect();
            
            let mut correlation = Complex64::new(0.0, 0.0);
            for (i, &sample) in scaled_chunk.iter().enumerate() {
                correlation += filter[i] * sample;
            }
            
            all_symbols.push(correlation);
            
            // 四舍五入到2位小数进行分组
            let rounded = ((correlation.re * 100.0).round() as i32, (correlation.im * 100.0).round() as i32);
            unique_symbols.insert(rounded);
            
            if period % 20 == 0 {
                println!("  Period {}: {:.3} + {:.3}i", period, correlation.re, correlation.im);
            }
        }
    }
    
    println!("\n📊 Symbol statistics (first 100 periods):");
    println!("   Unique patterns: {}", unique_symbols.len());
    if unique_symbols.len() <= 10 {
        println!("   Patterns: {:?}", unique_symbols);
    }
    
    // 检查是否有非零符号
    let non_zero_count = all_symbols.iter().filter(|s| s.norm() > 0.001).count();
    println!("   Non-zero symbols: {} / {}", non_zero_count, all_symbols.len());
    
    Ok(())
}
