const SCALING: f64 = 32000.0; // out of 2**15

pub fn dumps(samples: &[f64]) -> Vec<u8> {
    let mut result = Vec::new();
    for &sample in samples {
        let scaled = (sample * SCALING) as i16;
        result.extend_from_slice(&scaled.to_le_bytes());
    }
    result
}

pub fn dumps_with_metadata(
    samples: &[f64],
    sample_rate: u32,
    channels: u16,
    bit_depth: u16,
) -> Vec<u8> {
    let mut result = Vec::new();

    // 添加简单的元数据头 (16字节)
    // 格式: [magic: 4字节] [sample_rate: 4字节] [channels: 2字节] [bit_depth: 2字节] [data_length: 4字节]
    let magic = b"PCMR"; // PCM Rust
    result.extend_from_slice(magic);
    result.extend_from_slice(&sample_rate.to_le_bytes());
    result.extend_from_slice(&channels.to_le_bytes());
    result.extend_from_slice(&bit_depth.to_le_bytes());
    result.extend_from_slice(&(samples.len() as u32).to_le_bytes());

    // 添加音频数据
    let audio_data = dumps(samples);
    result.extend_from_slice(&audio_data);

    result
}

pub fn iterate<T>(
    data: impl Iterator<Item = T>,
    size: usize,
) -> impl Iterator<Item = Vec<T>> {
    let mut iter = data;
    std::iter::from_fn(move || {
        let mut chunk = Vec::new();
        for _ in 0..size {
            if let Some(item) = iter.next() {
                chunk.push(item);
            } else {
                break;
            }
        }
        if chunk.is_empty() {
            None
        } else if chunk.len() < size {
            // pad with default values if needed
            None
        } else {
            Some(chunk)
        }
    })
}

pub fn take<T: Copy>(iter: &mut impl Iterator<Item = T>, n: usize) -> Vec<T> {
    iter.take(n).collect()
}

// Helper function to load PCM data (matching Python's common.loads)
pub fn loads(data: &[u8]) -> Vec<f64> {
    data.chunks(2)
        .map(|chunk| {
            if chunk.len() == 2 {
                let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                sample as f64 / SCALING
            } else {
                0.0
            }
        })
        .collect()
}

#[derive(Debug)]
pub struct PcmMetadata {
    pub sample_rate: u32,
    pub channels: u16,
    pub bit_depth: u16,
    pub data_length: u32,
}

pub fn loads_with_metadata(
    data: &[u8],
) -> Result<(PcmMetadata, Vec<f64>), String> {
    if data.len() < 16 {
        return Err("文件太小，无法包含元数据".to_string());
    }

    // 读取元数据头
    let magic = &data[0..4];
    if magic != b"PCMR" {
        return Err("不是有效的 PCM Rust 文件".to_string());
    }

    let sample_rate = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let channels = u16::from_le_bytes([data[8], data[9]]);
    let bit_depth = u16::from_le_bytes([data[10], data[11]]);
    let data_length =
        u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

    let metadata = PcmMetadata {
        sample_rate,
        channels,
        bit_depth,
        data_length,
    };

    // 读取音频数据
    let audio_data = &data[16..];
    let samples = loads(audio_data);

    Ok((metadata, samples))
}
