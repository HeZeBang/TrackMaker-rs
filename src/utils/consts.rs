/// 默认录音时长（秒）
pub const DEFAULT_RECORD_SECONDS: usize = 20;

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

/// Samples per level (Manchester level or 4B5B bit)
pub const SAMPLES_PER_LEVEL: usize = 3;

// Frame Parameters
/// Number of 0xAA pattern bytes in preamble
pub const PREAMBLE_PATTERN_BYTES: usize = 4;

/// Maximum data payload per frame (bytes)
pub const MAX_FRAME_DATA_SIZE: usize = 768;

/// Milliseconds between frames
pub const INTER_FRAME_GAP_MS: u32 = 5;

/// Samples between frames
pub const INTER_FRAME_GAP_SAMPLES: usize =
    (SAMPLE_RATE as usize * INTER_FRAME_GAP_MS as usize) / 1000;

pub const ACK_TIMEOUT_MS: u64 = 300;

pub const PHY_HEADER_BYTES: usize = 7; // Length (2) + CRC (1) + Frame Type (1) + Sequence (1) + Src (1) + Dst (1)

// --- CSMA/CA Constants ---
/// Energy level threshold to consider the channel busy.
pub const ENERGY_THRESHOLD: f32 = 0.05;
/// Energy detection minimum samples
pub const ENERGY_DETECTION_SAMPLES: usize = 240; // 5 ms at 48 kHz
/// Distributed Inter-frame Space (DIFS) in milliseconds.
/// The duration to sense the channel to see if it's idle.
pub const DIFS_DURATION_MS: u64 = 50;
/// Minimum contention window size (in slots).
pub const CW_MIN: u32 = 15;
/// Maximum contention window size (in slots).
pub const CW_MAX: u32 = 1023;
/// Duration of a single backoff slot in milliseconds.
pub const SLOT_TIME_MS: u64 = 10;