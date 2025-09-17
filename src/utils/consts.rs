/// 默认录音时长（秒）
pub const DEFAULT_RECORD_SECONDS: usize = 3;

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
