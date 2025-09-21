/// 默认录音时长（秒）- 增加到30秒以应对FSK较低的传输效率
pub const DEFAULT_RECORD_SECONDS: usize = 30;

/// 接收端最大等待时间（秒）- FSK解调需要更长的处理时间
pub const RECEIVER_MAX_WAIT_SECONDS: usize = 18;

/// 日志级别（可被 RUST_LOG 覆盖）
pub const LOG_LEVEL: &str = "info";

/// JACK 客户端名称
pub const JACK_CLIENT_NAME: &str = "track_maker";

/// 输入端口名称
pub const INPUT_PORT_NAME: &str = "tm_in";

/// 输出端口名称
pub const OUTPUT_PORT_NAME: &str = "tm_out";

/// 进度更新间隔（毫秒）
pub const PROGRESS_UPDATE_INTERVAL_MS: u64 = 50;

// FSK 调制参数
/// FSK频率 - 表示bit '0'
pub const FSK_FREQ_0: f32 = 8000.0;

/// FSK频率 - 表示bit '1' 
pub const FSK_FREQ_1: f32 = 12000.0;

/// FSK码率 (bps)
pub const FSK_BAUD_RATE: f32 = 800.0;  // 降低码率以提高可靠性

/// 每bit的采样数 (48kHz / 800bps = 60 samples/bit)
pub const SAMPLES_PER_BIT: usize = 60;
