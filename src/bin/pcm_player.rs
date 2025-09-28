use clap::Parser;
use jack;
use rubato::{Resampler, SincFixedIn, SincInterpolationType, SincInterpolationParameters, WindowFunction};
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tracing::{error, info, warn};
use trackmaker_rs::amodem::common;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// PCM 文件路径
    #[arg(short, long)]
    input: String,
    
    /// 采样率 (Hz)
    #[arg(short, long, default_value_t = 8000)]
    sample_rate: u32,
    
    /// 声道数 (1=单声道, 2=立体声)
    #[arg(short, long, default_value_t = 1)]
    channels: u16,
    
    /// 位深度 (8, 16, 24, 32)
    #[arg(short, long, default_value_t = 16)]
    bit_depth: u16,
    
    /// 音量增益 (0.0-2.0)
    #[arg(short, long, default_value_t = 1.0)]
    gain: f32,
    
    /// 循环播放
    #[arg(short, long)]
    loop_play: bool,
    
    /// 播放时长 (秒，0表示播放完整文件)
    #[arg(short, long, default_value_t = 0)]
    duration: u32,
}

#[derive(Clone)]
struct PlaybackState {
    samples: Arc<Mutex<Vec<f32>>>,
    position: Arc<Mutex<usize>>,
    is_playing: Arc<Mutex<bool>>,
    should_loop: bool,
}

impl PlaybackState {
    fn new(samples: Vec<f32>, should_loop: bool) -> Self {
        Self {
            samples: Arc::new(Mutex::new(samples)),
            position: Arc::new(Mutex::new(0)),
            is_playing: Arc::new(Mutex::new(true)),
            should_loop,
        }
    }
}

fn read_pcm_file(
    path: &str,
    sample_rate: u32,
    channels: u16,
    bit_depth: u16,
) -> io::Result<Vec<f32>> {
    let mut file = BufReader::new(File::open(path)?);
    let mut samples = Vec::new();
    
    info!("读取 PCM 文件: {}", path);
    info!("参数: {}Hz, {}声道, {}位", sample_rate, channels, bit_depth);
    
    match bit_depth {
        8 => {
            let mut buffer = [0u8; 1024];
            loop {
                let bytes_read = file.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                
                for &byte in &buffer[..bytes_read] {
                    // 8位无符号转有符号，然后归一化到 [-1.0, 1.0]
                    let sample = (byte as i8 as f32) / 128.0;
                    samples.push(sample);
                }
            }
        }
        16 => {
            let mut buffer = [0u8; 2048];
            loop {
                let bytes_read = file.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                
                for chunk in buffer[..bytes_read].chunks(2) {
                    if chunk.len() == 2 {
                        let sample = i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0;
                        samples.push(sample);
                    }
                }
            }
        }
        24 => {
            let mut buffer = [0u8; 3072];
            loop {
                let bytes_read = file.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                
                for chunk in buffer[..bytes_read].chunks(3) {
                    if chunk.len() == 3 {
                        // 24位转32位有符号整数
                        let sample = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]) as f32 / 8388608.0;
                        samples.push(sample);
                    }
                }
            }
        }
        32 => {
            let mut buffer = [0u8; 4096];
            loop {
                let bytes_read = file.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                
                for chunk in buffer[..bytes_read].chunks(4) {
                    if chunk.len() == 4 {
                        let sample = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f32 / 2147483648.0;
                        samples.push(sample);
                    }
                }
            }
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("不支持的位深度: {}", bit_depth),
            ));
        }
    }
    
    // 如果是立体声，转换为单声道（取平均值）
    if channels == 2 {
        let mut mono_samples = Vec::new();
        for chunk in samples.chunks(2) {
            if chunk.len() == 2 {
                mono_samples.push((chunk[0] + chunk[1]) / 2.0);
            }
        }
        samples = mono_samples;
    }
    
    info!("读取了 {} 个样本", samples.len());
    Ok(samples)
}

fn read_pcm_with_metadata(path: &str) -> io::Result<(u32, u16, u16, Vec<f32>)> {
    let mut file = File::open(path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    
    // 尝试读取带元数据的 PCM 文件
    match common::loads_with_metadata(&data) {
        Ok((metadata, samples_f64)) => {
            info!("读取带元数据的 PCM 文件: {}", path);
            info!("元数据: {} Hz, {} 声道, {} 位, {} 样本", 
                  metadata.sample_rate, metadata.channels, metadata.bit_depth, metadata.data_length);
            
            let samples_f32: Vec<f32> = samples_f64.iter().map(|&x| x as f32).collect();
            Ok((metadata.sample_rate, metadata.channels, metadata.bit_depth, samples_f32))
        }
        Err(_) => {
            // 如果不是带元数据的文件，回退到原始方法
            info!("文件不是带元数据的 PCM 格式，使用默认参数读取");
            let samples = read_pcm_file(path, 8000, 1, 16)?;
            Ok((8000, 1, 16, samples))
        }
    }
}

fn high_quality_resample(
    input_samples: &[f32],
    input_rate: u32,
    output_rate: u32,
) -> io::Result<Vec<f32>> {
    if input_rate == output_rate {
        return Ok(input_samples.to_vec());
    }
    
    let ratio = output_rate as f64 / input_rate as f64;
    info!("高质量重采样: {} Hz -> {} Hz (比例: {:.3})", input_rate, output_rate, ratio);
    
    // 配置高质量重采样参数
    let params = SincInterpolationParameters {
        sinc_len: 256,                    // 更长的 sinc 滤波器
        f_cutoff: 0.95,                   // 截止频率
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,         // 高过采样因子
        window: WindowFunction::BlackmanHarris2, // 高质量窗函数
    };
    
    // 创建重采样器
    let mut resampler = SincFixedIn::<f32>::new(
        ratio,
        2.0, // 最大比例
        params,
        input_samples.len(),
        1, // 单声道
    ).map_err(|e| io::Error::new(io::ErrorKind::Other, format!("重采样器创建失败: {}", e)))?;
    
    // 准备输入数据（Rubato 需要 Vec<Vec<f32>> 格式）
    let input = vec![input_samples.to_vec()];
    
    // 执行重采样
    let output = resampler.process(&input, None)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("重采样处理失败: {}", e)))?;
    
    let result = output[0].clone();
    info!("高质量重采样完成: {} -> {} 样本", input_samples.len(), result.len());
    Ok(result)
}

fn create_process_callback(
    mut out_port: jack::Port<jack::AudioOut>,
    state: PlaybackState,
    gain: f32,
) -> impl FnMut(&jack::Client, &jack::ProcessScope) -> jack::Control + Send + 'static {
    move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        let out_buffer = out_port.as_mut_slice(ps);
        
        // 清零输出缓冲区
        for sample in out_buffer.iter_mut() {
            *sample = 0.0;
        }
        
        // 检查是否正在播放
        let is_playing = {
            let playing = state.is_playing.lock().unwrap();
            *playing
        };
        
        if !is_playing {
            return jack::Control::Continue;
        }
        
        // 获取当前播放位置
        let (current_pos, samples_len) = {
            let mut pos = state.position.lock().unwrap();
            let samples = state.samples.lock().unwrap();
            let current = *pos;
            let len = samples.len();
            *pos = current;
            (current, len)
        };
        
        if current_pos >= samples_len {
            if state.should_loop {
                // 循环播放：重置位置
                let mut pos = state.position.lock().unwrap();
                *pos = 0;
            } else {
                // 停止播放
                let mut playing = state.is_playing.lock().unwrap();
                *playing = false;
                return jack::Control::Continue;
            }
        }
        
        // 填充音频缓冲区
        let samples = state.samples.lock().unwrap();
        let mut pos = state.position.lock().unwrap();
        
        for out_sample in out_buffer.iter_mut() {
            if *pos < samples.len() {
                *out_sample = samples[*pos] * gain;
                *pos += 1;
            } else if state.should_loop {
                *pos = 0;
                if *pos < samples.len() {
                    *out_sample = samples[*pos] * gain;
                    *pos += 1;
                }
            } else {
                break;
            }
        }
        
        jack::Control::Continue
    }
}

fn main() -> io::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt::init();
    
    let cli = Cli::parse();
    
    // 读取 PCM 文件（优先使用元数据，否则使用命令行参数）
    let (file_sample_rate, file_channels, file_bit_depth, samples) = read_pcm_with_metadata(&cli.input)?;
    
    if samples.is_empty() {
        error!("PCM 文件为空");
        return Ok(());
    }
    
    // 使用文件中的元数据，除非用户明确指定了参数
    let actual_sample_rate = if cli.sample_rate != 8000 { cli.sample_rate } else { file_sample_rate };
    let actual_channels = if cli.channels != 1 { cli.channels } else { file_channels };
    let actual_bit_depth = if cli.bit_depth != 16 { cli.bit_depth } else { file_bit_depth };
    
    info!("使用参数: {} Hz, {} 声道, {} 位", actual_sample_rate, actual_channels, actual_bit_depth);
    
    // 设置 JACK 客户端以获取目标采样率
    let (client, _status) = jack::Client::new(
        "pcm_player",
        jack::ClientOptions::NO_START_SERVER,
    ).map_err(|e| io::Error::new(io::ErrorKind::Other, format!("JACK 客户端创建失败: {}", e)))?;
    
    let jack_sample_rate = client.sample_rate();
    
    // 重采样到 JACK 采样率
    let resampled_samples = high_quality_resample(&samples, actual_sample_rate, jack_sample_rate as u32)?;
    
    // 创建播放状态（使用重采样后的样本）
    let state = PlaybackState::new(resampled_samples, cli.loop_play);
    
    // 显示 JACK 服务器信息
    let buffer_size = client.buffer_size();
    info!("JACK 服务器信息:");
    info!("  采样率: {} Hz", jack_sample_rate);
    info!("  缓冲区大小: {} 样本", buffer_size);
    info!("  缓冲区时长: {:.2} ms", (buffer_size as f64 / jack_sample_rate as f64) * 1000.0);
    
    // 检查采样率匹配
    if jack_sample_rate != actual_sample_rate as usize {
        warn!("警告: JACK 采样率 ({}) 与文件采样率 ({}) 不匹配", jack_sample_rate, actual_sample_rate);
    }
    
    // 注册输出端口
    let out_port = client.register_port("pcm_out", jack::AudioOut::default())
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("端口注册失败: {}", e)))?;
    let out_port_name = out_port.name()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("获取端口名称失败: {}", e)))?;
    
    // 创建处理回调
    let process_callback = create_process_callback(out_port, state.clone(), cli.gain);
    let process = jack::contrib::ClosureProcessHandler::new(process_callback);
    
    // 激活客户端
    let active_client = client.activate_async((), process)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("客户端激活失败: {}", e)))?;
    
    // 连接到系统输出端口
    let system_input_ports = active_client.as_client().ports(
        None,
        None,
        jack::PortFlags::IS_INPUT | jack::PortFlags::IS_PHYSICAL,
    );
    
    if let Some(system_in) = system_input_ports.first() {
        match active_client.as_client().connect_ports_by_name(&out_port_name, system_in) {
            Ok(_) => info!("已连接输出: {} -> {}", out_port_name, system_in),
            Err(e) => error!("连接输出失败: {}", e),
        }
    } else {
        warn!("未找到系统输入端口");
    }
    
    info!("开始播放 PCM 文件...");
    
    // 计算播放时长
    let play_duration = if cli.duration > 0 {
        Duration::from_secs(cli.duration as u64)
    } else {
        // 根据样本数和采样率计算时长
        let samples_len = state.samples.lock().unwrap().len();
        let duration_secs = samples_len as f64 / jack_sample_rate as f64;
        Duration::from_secs_f64(duration_secs)
    };
    
    // 等待播放完成
    thread::sleep(play_duration);
    
    // 停止播放
    {
        let mut playing = state.is_playing.lock().unwrap();
        *playing = false;
    }
    
    info!("播放完成");
    
    // 断开连接并停用客户端
    if let Err(err) = active_client.deactivate() {
        error!("停用客户端时出错: {}", err);
    }
    
    Ok(())
}
