use clap::Parser;
use dialoguer::{theme::ColorfulTheme, Select};
use jack;
use std::io::Write;
mod audio;
mod device;
mod ui;
mod utils;
use audio::recorder;
use device::jack::{
    print_jack_info,
};
use tracing::{debug, info};
use ui::print_banner;
use ui::progress::{ProgressManager, templates};
use utils::consts::*;
use utils::logging::init_logging;

use crate::device::jack::connect_system_ports;

/// PSK800RC2 数字调制解调器
#[derive(Parser)]
#[command(name = "trackmaker")]
#[command(about = "基于 fldigi PSK800RC2 模式的数字调制解调器")]
struct Args {
    /// 运行测试模式（不使用 JACK 音频设备）
    #[arg(long)]
    test: bool,
}

// PSK800RC2 参数
const SYMBOL_RATE: f32 = 800.0; // 符号速率 800 Hz
const CARRIER_FREQ: f32 = 1500.0; // 载波频率 1500 Hz
const SAMPLE_RATE: u32 = 48000; // 采样率 48 kHz
const SAMPLES_PER_SYMBOL: usize = (SAMPLE_RATE as f32 / SYMBOL_RATE) as usize; // 60 samples per symbol
const RC_ALPHA: f32 = 0.35; // 根余弦滤波器滚降因子
const PREAMBLE_BITS: usize = 32; // 前导码长度
const SYNC_WORD: u32 = 0x1ACFFC1D; // 同步字

// PSK800RC2 调制器结构
struct PSK800RC2Modulator {
    sample_rate: f32,
    carrier_freq: f32,
    symbol_rate: f32,
    samples_per_symbol: usize,
    phase: f32,
    rc_filter: Vec<f32>,
}

impl PSK800RC2Modulator {
    fn new() -> Self {
        let samples_per_symbol = SAMPLES_PER_SYMBOL;
        let rc_filter = Self::generate_rc_filter(samples_per_symbol, RC_ALPHA);
        
        Self {
            sample_rate: SAMPLE_RATE as f32,
            carrier_freq: CARRIER_FREQ,
            symbol_rate: SYMBOL_RATE,
            samples_per_symbol,
            phase: 0.0,
            rc_filter,
        }
    }
    
    /// 生成根余弦滤波器
    fn generate_rc_filter(samples_per_symbol: usize, alpha: f32) -> Vec<f32> {
        let filter_length = samples_per_symbol * 8; // 8 个符号长度
        let mut filter = Vec::with_capacity(filter_length);
        let t_symbol = samples_per_symbol as f32;
        
        for i in 0..filter_length {
            let t = (i as f32 - filter_length as f32 / 2.0) / t_symbol;
            
            if t == 0.0 {
                filter.push(1.0);
            } else if (4.0 * alpha * t).abs() == 1.0 {
                let val = (std::f32::consts::PI / 4.0) * 
                    ((1.0 + alpha) * (std::f32::consts::PI * (1.0 + alpha) / (4.0 * alpha)).sin() + 
                     (1.0 - alpha) * (std::f32::consts::PI * (1.0 - alpha) / (4.0 * alpha)).cos());
                filter.push(val / (std::f32::consts::PI * alpha));
            } else {
                let numerator = (std::f32::consts::PI * t).sin() * 
                    (std::f32::consts::PI * alpha * t).cos();
                let denominator = std::f32::consts::PI * t * 
                    (1.0 - (4.0 * alpha * t).powi(2));
                filter.push(numerator / denominator);
            }
        }
        
        // 归一化滤波器
        let sum: f32 = filter.iter().sum();
        if sum > 0.0 {
            for coeff in &mut filter {
                *coeff /= sum;
            }
        }
        
        filter
    }
    
    /// 调制比特数据
    fn modulate(&mut self, bits: &[u8]) -> Vec<f32> {
        let mut output = Vec::new();
        let mut symbol_buffer = vec![0.0f32; self.rc_filter.len()];
        
        for &bit in bits {
            // BPSK: 0 -> -1, 1 -> +1
            let symbol = if bit == 0 { -1.0 } else { 1.0 };
            
            // 脉冲成形
            symbol_buffer.rotate_left(self.samples_per_symbol);
            for i in 0..self.samples_per_symbol {
                symbol_buffer[symbol_buffer.len() - self.samples_per_symbol + i] = 0.0;
            }
            symbol_buffer[symbol_buffer.len() - self.samples_per_symbol] = symbol;
            
            // 应用根余弦滤波器
            let mut filtered_samples = vec![0.0f32; self.samples_per_symbol];
            for i in 0..self.samples_per_symbol {
                let mut sum = 0.0;
                for (j, &coeff) in self.rc_filter.iter().enumerate() {
                    let buffer_idx = (symbol_buffer.len() - self.rc_filter.len() + j + i) % symbol_buffer.len();
                    sum += coeff * symbol_buffer[buffer_idx];
                }
                filtered_samples[i] = sum;
            }
            
            // 载波调制
            for sample in filtered_samples {
                let carrier = (2.0 * std::f32::consts::PI * self.carrier_freq * self.phase / self.sample_rate).cos();
                output.push(sample * carrier);
                self.phase += 1.0;
                if self.phase >= self.sample_rate {
                    self.phase = 0.0;
                }
            }
        }
        
        output
    }
}

// PSK800RC2 解调器结构
struct PSK800RC2Demodulator {
    sample_rate: f32,
    carrier_freq: f32,
    symbol_rate: f32,
    samples_per_symbol: usize,
    phase: f32,
    i_buffer: Vec<f32>,
    q_buffer: Vec<f32>,
    symbol_buffer: Vec<f32>,
    bit_sync_phase: f32,
    last_symbol: f32,
    sync_state: SyncState,
    sync_word_buffer: u32,
    bits_since_sync: usize,
}

#[derive(Debug, Clone, Copy)]
enum SyncState {
    Searching,
    Synchronized,
}

impl PSK800RC2Demodulator {
    fn new() -> Self {
        Self {
            sample_rate: SAMPLE_RATE as f32,
            carrier_freq: CARRIER_FREQ,
            symbol_rate: SYMBOL_RATE,
            samples_per_symbol: SAMPLES_PER_SYMBOL,
            phase: 0.0,
            i_buffer: Vec::new(),
            q_buffer: Vec::new(),
            symbol_buffer: Vec::new(),
            bit_sync_phase: 0.0,
            last_symbol: 0.0,
            sync_state: SyncState::Searching,
            sync_word_buffer: 0,
            bits_since_sync: 0,
        }
    }
    
    /// 解调音频信号
    fn demodulate(&mut self, samples: &[f32]) -> Vec<u8> {
        let mut bits = Vec::new();
        
        for &sample in samples {
            // 正交解调
            let i_sample = sample * (2.0 * std::f32::consts::PI * self.carrier_freq * self.phase / self.sample_rate).cos();
            let q_sample = sample * -(2.0 * std::f32::consts::PI * self.carrier_freq * self.phase / self.sample_rate).sin();
            
            self.i_buffer.push(i_sample);
            self.q_buffer.push(q_sample);
            
            // 低通滤波（简单的移动平均）
            let window_size = self.samples_per_symbol / 4;
            if self.i_buffer.len() > window_size {
                self.i_buffer.remove(0);
                self.q_buffer.remove(0);
            }
            
            let i_filtered: f32 = self.i_buffer.iter().sum::<f32>() / self.i_buffer.len() as f32;
            let q_filtered: f32 = self.q_buffer.iter().sum::<f32>() / self.q_buffer.len() as f32;
            
            // 计算符号
            let symbol = (i_filtered.atan2(q_filtered) - self.last_symbol).cos();
            self.symbol_buffer.push(symbol);
            
            // 符号采样
            self.bit_sync_phase += self.symbol_rate / self.sample_rate;
            if self.bit_sync_phase >= 1.0 {
                self.bit_sync_phase -= 1.0;
                
                if let Some(&sampled_symbol) = self.symbol_buffer.last() {
                    let bit = if sampled_symbol > 0.0 { 1 } else { 0 };
                    
                    // 同步检测
                    self.sync_word_buffer = (self.sync_word_buffer << 1) | bit as u32;
                    
                    match self.sync_state {
                        SyncState::Searching => {
                            if self.sync_word_buffer == SYNC_WORD {
                                self.sync_state = SyncState::Synchronized;
                                self.bits_since_sync = 0;
                                debug!("找到同步字，开始接收数据");
                            }
                        }
                        SyncState::Synchronized => {
                            if self.bits_since_sync >= PREAMBLE_BITS {
                                bits.push(bit);
                            }
                            self.bits_since_sync += 1;
                            
                            // 检查是否丢失同步
                            if self.bits_since_sync > 10000 { // 10000 比特后重新搜索同步
                                self.sync_state = SyncState::Searching;
                                debug!("同步丢失，重新搜索同步字");
                            }
                        }
                    }
                }
                
                self.symbol_buffer.clear();
            }
            
            self.phase += 1.0;
            if self.phase >= self.sample_rate {
                self.phase = 0.0;
            }
        }
        
        bits
    }
}

/// 将比特转换为字节
fn bits_to_bytes(bits: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for chunk in bits.chunks(8) {
        if chunk.len() == 8 {
            let mut byte = 0u8;
            for (i, &bit) in chunk.iter().enumerate() {
                byte |= (bit & 1) << (7 - i);
            }
            bytes.push(byte);
        }
    }
    bytes
}

/// 将字节转换为比特
fn bytes_to_bits(bytes: &[u8]) -> Vec<u8> {
    let mut bits = Vec::new();
    for &byte in bytes {
        for i in 0..8 {
            bits.push((byte >> (7 - i)) & 1);
        }
    }
    bits
}

/// 生成同步序列
fn generate_sync_sequence() -> Vec<u8> {
    let mut sync_bits = Vec::new();
    
    // 前导码：交替的 0101... 模式，用于时钟恢复
    for i in 0..PREAMBLE_BITS {
        sync_bits.push((i % 2) as u8);
    }
    
    // 同步字
    let sync_word_bits = bytes_to_bits(&SYNC_WORD.to_be_bytes());
    sync_bits.extend(sync_word_bits);
    
    sync_bits
}

/// 测试模式 - 不使用 JACK 音频设备
fn test_psk800rc2() {
    println!("开始 PSK800RC2 测试模式...");
    
    // 读取测试数据
    let test_data = std::fs::read_to_string("assets/think-different.txt")
        .expect("无法读取 think-different.txt 文件");
    println!("原始数据长度: {} 字节", test_data.len());
    println!("原始数据内容: {}", test_data.trim());
    
    // 准备发送数据
    let data_bytes = test_data.as_bytes();
    let data_bits = bytes_to_bits(data_bytes);
    
    // 生成完整的发送比特序列
    let mut tx_bits = generate_sync_sequence();
    tx_bits.extend(data_bits);
    println!("总发送比特数: {}", tx_bits.len());
    
    // 调制
    let mut modulator = PSK800RC2Modulator::new();
    let modulated_signal = modulator.modulate(&tx_bits);
    println!("调制信号长度: {} 样本", modulated_signal.len());
    
    // 保存调制信号到文件
    utils::dump::dump_to_wav("./tmp/psk800rc2_test.wav", &utils::dump::AudioData {
        sample_rate: SAMPLE_RATE,
        audio_data: modulated_signal.clone(),
        duration: modulated_signal.len() as f32 / SAMPLE_RATE as f32,
        channels: 1,
    }).expect("无法保存 WAV 文件");
    info!("已保存调制信号到 ./tmp/psk800rc2_test.wav");
    
    // 解调
    let mut demodulator = PSK800RC2Demodulator::new();
    let rx_bits = demodulator.demodulate(&modulated_signal);
    println!("接收到的比特数: {}", rx_bits.len());
    
    // 转换为字节并解码
    let rx_bytes = bits_to_bytes(&rx_bits);
    if let Ok(decoded_text) = String::from_utf8(rx_bytes) {
        println!("解码数据长度: {} 字节", decoded_text.len());
        println!("解码数据内容: {}", decoded_text.trim());
        
        // 比较结果
        let original = test_data.trim();
        let decoded = decoded_text.trim();
        
        if original == decoded {
            println!("✅ 测试通过！编码解码完全匹配");
        } else {
            println!("⚠️  部分匹配，比较前 {} 个字符:", original.len().min(decoded.len()));
            let match_len = original.chars().zip(decoded.chars())
                .take_while(|(a, b)| a == b)
                .count();
            println!("匹配长度: {} 字符", match_len);
            
            if match_len > 0 {
                println!("✅ 部分测试通过！前 {} 个字符匹配", match_len);
            } else {
                println!("❌ 测试失败！没有匹配的字符");
            }
        }
    } else {
        println!("❌ 解码失败！接收到的数据不是有效的 UTF-8");
    }
}

fn main() {
    let args = Args::parse();
    
    init_logging();
    
    if args.test {
        // 测试模式 - 不使用 JACK
        test_psk800rc2();
        return;
    }
    
    print_banner();

    let (client, status) = jack::Client::new(
        JACK_CLIENT_NAME,
        jack::ClientOptions::NO_START_SERVER,
    )
    .unwrap();
    tracing::info!("JACK client status: {:?}", status);
    let (sample_rate, _buffer_size) = print_jack_info(&client);

    let max_duration_samples = sample_rate * 15;

    // Shared State
    let shared = recorder::AppShared::new(max_duration_samples);
    let shared_cb = shared.clone();

    let in_port = client
        .register_port(INPUT_PORT_NAME, jack::AudioIn::default())
        .unwrap();
    let out_port = client
        .register_port(OUTPUT_PORT_NAME, jack::AudioOut::default())
        .unwrap();

    let in_port_name = in_port.name().unwrap();
    let out_port_name = out_port.name().unwrap();

    // Process Callback
    let process_cb = recorder::build_process_closure(
        in_port,
        out_port,
        shared_cb,
        max_duration_samples,
    );
    let process = jack::contrib::ClosureProcessHandler::new(process_cb);

    let active_client = client
        .activate_async((), process)
        .unwrap();

    let progress_manager = ProgressManager::new();

    connect_system_ports(
        active_client.as_client(),
        in_port_name.as_str(),
        out_port_name.as_str(),
    );

    let selections = &["PSK800RC2 发送模式", "PSK800RC2 接收模式"];
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("选择模式")
        .default(0)
        .items(&selections[..])
        .interact()
        .unwrap();

    {
        shared.record_buffer.lock().unwrap().clear();
    }

    if selection == 0 {
        // PSK800RC2 Sender
        run_psk800rc2_sender(shared, progress_manager, sample_rate as u32);
    } else {
        // PSK800RC2 Receiver
        run_psk800rc2_receiver(shared, progress_manager, sample_rate as u32, max_duration_samples as u32);
    }

    tracing::info!("正在优雅退出...");
    if let Err(err) = active_client.deactivate() {
        tracing::error!("关闭客户端时出错: {}", err);
    }
}

/// PSK800RC2 发送模式
fn run_psk800rc2_sender(
    shared: recorder::AppShared,
    progress_manager: ProgressManager,
    sample_rate: u32,
) {
    // 读取测试数据
    let file_content = std::fs::read_to_string("assets/think-different.txt")
        .expect("无法读取 think-different.txt");
    
    println!("发送数据: {}", file_content.trim());
    
    // 准备发送数据
    let data_bytes = file_content.as_bytes();
    let data_bits = bytes_to_bits(data_bytes);
    
    // 生成完整的发送比特序列
    let mut tx_bits = generate_sync_sequence();
    tx_bits.extend(data_bits);
    
    // PSK800RC2 调制
    let mut modulator = PSK800RC2Modulator::new();
    let modulated_signal = modulator.modulate(&tx_bits);
    
    let output_track_len = modulated_signal.len();
    
    {
        let mut playback = shared.playback_buffer.lock().unwrap();
        playback.extend(modulated_signal);
        info!("PSK800RC2 输出信号长度: {} 样本", playback.len());
    }

    progress_manager
        .create_bar(
            "playback",
            output_track_len as u64,
            templates::PLAYBACK,
            "PSK800RC2 发送",
        )
        .unwrap();

    *shared.app_state.lock().unwrap() = recorder::AppState::Playing;

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));

        ui::update_progress(&shared, output_track_len, &progress_manager);

        let state = { shared.app_state.lock().unwrap().clone() };
        if let recorder::AppState::Idle = state {
            progress_manager.finish_all();
            break;
        }
    }
}

/// PSK800RC2 接收模式
fn run_psk800rc2_receiver(
    shared: recorder::AppShared,
    progress_manager: ProgressManager,
    sample_rate: u32,
    max_recording_duration_samples: u32,
) {
    progress_manager
        .create_bar(
            "recording",
            max_recording_duration_samples as u64,
            templates::RECORDING,
            "PSK800RC2 接收",
        )
        .unwrap();

    *shared.app_state.lock().unwrap() = recorder::AppState::Recording;

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));

        ui::update_progress(
            &shared,
            max_recording_duration_samples as usize,
            &progress_manager,
        );

        let state = {
            shared
                .app_state
                .lock()
                .unwrap()
                .clone()
        };
        if let recorder::AppState::Idle = state {
            progress_manager.finish_all();
            break;
        }
    }

    let rx_samples: Vec<f32> = {
        let record = shared.record_buffer.lock().unwrap();
        record.iter().copied().collect()
    };

    println!("开始 PSK800RC2 解调...");
    let mut demodulator = PSK800RC2Demodulator::new();
    let rx_bits = demodulator.demodulate(&rx_samples);
    
    println!("接收到 {} 比特", rx_bits.len());
    
    // 转换为字节并解码
    let rx_bytes = bits_to_bytes(&rx_bits);
    if let Ok(decoded_text) = String::from_utf8(rx_bytes) {
        println!("解码结果: {}", decoded_text);
    } else {
        println!("解码失败：接收到的数据不是有效的 UTF-8");
    }
}

