use crate::acoustic::config::FRAME_LEN;

/// Frame metadata for acoustic transmission
#[derive(Clone, Debug)]
pub struct Frame {
    pub id: u8,
    pub data: Vec<u8>,
}

#[derive(Clone)]
pub struct FrameConfig {
    payload_bytes: usize,
}

impl Default for FrameConfig {
    fn default() -> Self {
        Self {
            payload_bytes: FRAME_LEN,
        }
    }
}

impl FrameConfig {
    pub fn bytes_per_frame(&self) -> usize {
        self.payload_bytes
    }
}

/// Frame manager for splitting data into fixed-length payloads
pub struct FrameManager {
    config: FrameConfig,
}

impl FrameManager {
    pub fn new(config: FrameConfig) -> Self {
        Self { config }
    }

    pub fn new_default() -> Self {
        Self::new(FrameConfig::default())
    }

    pub fn create_frames(&self, data: &[u8]) -> Vec<Frame> {
        if data.is_empty() {
            return Vec::new();
        }

        let bytes_per_frame = self.config.bytes_per_frame();
        let total_frames = (data.len() + bytes_per_frame - 1) / bytes_per_frame;
        let mut frames = Vec::with_capacity(total_frames);

        for frame_idx in 0..total_frames {
            let start_byte = frame_idx * bytes_per_frame;
            let end_byte =
                std::cmp::min(start_byte + bytes_per_frame, data.len());
            let mut frame_data = vec![0u8; bytes_per_frame];
            let slice = &data[start_byte..end_byte];
            frame_data[..slice.len()].copy_from_slice(slice);
            frames.push(Frame {
                id: (frame_idx + 1) as u8,
                data: frame_data,
            });
        }

        frames
    }

    pub fn reconstruct_data(&self, mut frames: Vec<Frame>) -> Vec<u8> {
        if frames.is_empty() {
            return Vec::new();
        }

        frames.sort_by_key(|f| f.id);

        let mut reconstructed_data = Vec::new();
        for frame in frames {
            reconstructed_data.extend(frame.data);
        }

        while reconstructed_data
            .last()
            .copied()
            == Some(0)
        {
            reconstructed_data.pop();
        }

        reconstructed_data
    }

    pub fn config(&self) -> &FrameConfig {
        &self.config
    }
}
