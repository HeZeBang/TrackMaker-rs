use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct AudioData {
    pub sample_rate: u32,
    pub audio_data: Vec<f32>,
    pub duration: f32,
    pub channels: u32,
}
