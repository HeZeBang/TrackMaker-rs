use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct ProgressManager {
    mp: MultiProgress,
    bars: Arc<Mutex<HashMap<String, ProgressBar>>>,
}

impl ProgressManager {
    pub fn new() -> Self {
        Self {
            mp: MultiProgress::new(),
            bars: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 创建新的进度条
    /// - `id`: 进度条唯一标识
    /// - `total`: 总进度值
    /// - `template`: 进度条模板
    /// - `message`: 初始消息
    pub fn create_bar(
        &self,
        id: &str,
        total: u64,
        template: &str,
        message: &str,
    ) -> Result<(), String> {
        let mut bars = self.bars.lock().map_err(|e| format!("Lock error: {}", e))?;
        
        if bars.contains_key(id) {
            return Err(format!("Progress bar '{}' already exists", id));
        }

        let pb = self.mp.add(ProgressBar::new(total));
        pb.set_style(
            ProgressStyle::default_bar()
                .template(template)
                .unwrap()
                .progress_chars("█▉▊▋▌▍▎▏ "),
        );
        pb.set_message(message.to_string());

        bars.insert(id.to_string(), pb);
        Ok(())
    }

    /// 更新进度条位置
    pub fn set_position(&self, id: &str, pos: u64) -> Result<(), String> {
        let bars = self.bars.lock().map_err(|e| format!("Lock error: {}", e))?;
        if let Some(pb) = bars.get(id) {
            pb.set_position(pos);
            Ok(())
        } else {
            Err(format!("Progress bar '{}' not found", id))
        }
    }

    /// 更新进度条消息
    pub fn set_message(&self, id: &str, message: &str) -> Result<(), String> {
        let bars = self.bars.lock().map_err(|e| format!("Lock error: {}", e))?;
        if let Some(pb) = bars.get(id) {
            pb.set_message(message.to_string());
            Ok(())
        } else {
            Err(format!("Progress bar '{}' not found", id))
        }
    }

    /// 完成并清理进度条
    pub fn finish_and_clear(&self, id: &str) -> Result<(), String> {
        let mut bars = self.bars.lock().map_err(|e| format!("Lock error: {}", e))?;
        if let Some(pb) = bars.remove(id) {
            pb.finish_and_clear();
            Ok(())
        } else {
            Err(format!("Progress bar '{}' not found", id))
        }
    }

    /// 完成进度条（保留显示）
    pub fn finish(&self, id: &str, message: &str) -> Result<(), String> {
        let bars = self.bars.lock().map_err(|e| format!("Lock error: {}", e))?;
        if let Some(pb) = bars.get(id) {
            pb.finish_with_message(message.to_string());
            Ok(())
        } else {
            Err(format!("Progress bar '{}' not found", id))
        }
    }

    /// 检查进度条是否存在
    pub fn exists(&self, id: &str) -> bool {
        if let Ok(bars) = self.bars.lock() {
            bars.contains_key(id)
        } else {
            false
        }
    }

    /// 检查进度条是否已完成
    pub fn is_finished(&self, id: &str) -> Result<bool, String> {
        let bars = self.bars.lock().map_err(|e| format!("Lock error: {}", e))?;
        if let Some(pb) = bars.get(id) {
            Ok(pb.is_finished())
        } else {
            Err(format!("Progress bar '{}' not found", id))
        }
    }

    /// 清理所有进度条
    pub fn clear_all(&self) {
        if let Ok(mut bars) = self.bars.lock() {
            for (_, pb) in bars.drain() {
                pb.finish_and_clear();
            }
        }
    }
}

impl Default for ProgressManager {
    fn default() -> Self {
        Self::new()
    }
}

pub mod templates {
    pub const RECORDING: &str = "RECORDING: [{bar:30.red}] {percent}% ({pos}/{len} samples) {msg}";
    pub const PLAYBACK: &str = "PLAYBACK: [{bar:30.green}] {percent}% ({pos}/{len} samples) {msg}";
    pub const PROCESSING: &str = "PROCESSING: [{bar:30.blue}] {percent}% ({pos}/{len}) {msg}";
    pub const DOWNLOAD: &str = "DOWNLOAD: [{bar:30.yellow}] {percent}% ({pos}/{len} bytes) {msg}";
}

pub mod colors {
    pub const RED: &str = "red";
    pub const GREEN: &str = "green";
    pub const BLUE: &str = "blue";
    pub const YELLOW: &str = "yellow";
    pub const CYAN: &str = "cyan";
    pub const MAGENTA: &str = "magenta";
}
