use serde::{Deserialize, Serialize};
use symphonia;

#[derive(Serialize, Deserialize)]
pub struct AudioData {
    pub sample_rate: u32,
    pub audio_data: Vec<f32>,
    pub duration: f32,
    pub channels: u32,
}

pub fn dump_to_json(
    file_path: &str,
    audio_data: &AudioData,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(audio_data)?;

    std::fs::write(file_path, json)?;

    Ok(())
}

pub fn dump_to_wav(
    file_path: &str,
    audio_data: &AudioData,
) -> Result<(), Box<dyn std::error::Error>> {
    use hound;

    let spec = hound::WavSpec {
        channels: audio_data.channels as u16,
        sample_rate: audio_data.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(file_path, spec)?;

    for &sample in &audio_data.audio_data {
        let amplitude = (sample * i16::MAX as f32) as i16;
        writer.write_sample(amplitude)?;
    }

    writer.finalize()?;

    Ok(())
}
