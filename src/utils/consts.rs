/// 默认录音时长（秒）
pub const DEFAULT_RECORD_SECONDS: usize = 10;

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

// ============================================================================
// Physical Layer Parameters (Project 2)
// ============================================================================

/// Sample rate (Hz)
pub const SAMPLE_RATE: u32 = 48000;

/// Target bit rate (bps) - Project 2 requires >= 12 Kbps
pub const BIT_RATE: u32 = 12000;

/// Samples per bit (SAMPLE_RATE / BIT_RATE)
pub const SAMPLES_PER_BIT: usize = 4;  // 48000 / 12000 = 4

/// Samples per Manchester level (half of samples per bit)
pub const SAMPLES_PER_LEVEL: usize = 4;  // Manchester: 4 levels per bit

// Frame Parameters
/// Number of 0xAA pattern bytes in preamble
pub const PREAMBLE_PATTERN_BYTES: usize = 4;

/// Maximum data payload per frame (bytes)
pub const MAX_FRAME_DATA_SIZE: usize = 64;

/// Milliseconds between frames
pub const INTER_FRAME_GAP_MS: u32 = 5;

/// Samples between frames
pub const INTER_FRAME_GAP_SAMPLES: usize = 
    (SAMPLE_RATE as usize * INTER_FRAME_GAP_MS as usize) / 1000;

// // Carrier Sensing (CSMA)
// /// Power threshold for detecting busy channel
// pub const CARRIER_SENSE_THRESHOLD: f32 = 0.01;

// /// Carrier sensing delay (~10ms as per requirements)
// pub const CARRIER_SENSE_DELAY_MS: u32 = 10;

