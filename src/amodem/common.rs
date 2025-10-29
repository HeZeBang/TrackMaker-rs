use ringbuf::{HeapCons, traits::*};

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

// /// Iterate over a signal, taking each time *size* elements.
// pub fn iterate<T: Clone>(
//     data: &mut HeapCons<T>,
//     size: usize,
//     truncate: Option<bool>,
// ) -> impl Iterator<Item = (usize, Vec<T>)> {
//     let truncate = truncate.unwrap_or(true);

//     let mut offset = 0;
//     let mut done = false;

//     std::iter::from_fn(move || {
//         if done {
//             return None;
//         }

//         let occupied = data.occupied_len();
//         if occupied < size {
//             if truncate || occupied == 0 {
//                 done = true;
//                 return None;
//             }
//             done = true;
//         }

//         let buf: Vec<T> = data
//             .pop_iter()
//             .take(size.min(occupied))
//             .collect();
//         let current_offset = offset;
//         offset += size;

//         Some((current_offset, buf))
//     })
// }

pub fn take<T: Copy>(iter: &mut impl Iterator<Item = T>, n: usize) -> Vec<T> {
    iter.take(n).collect()
}

/// Iterate over an iterator, taking each time *size* elements (for generic iterators)
pub fn iterate<T>(
    data: impl Iterator<Item = T>,
    size: usize,
    truncate: Option<bool>,
) -> impl Iterator<Item = Vec<T>> {
    let truncate = truncate.unwrap_or(true);
    let mut iter = data.peekable();

    std::iter::from_fn(move || {
        let buf: Vec<T> = iter
            .by_ref()
            .take(size)
            .collect();

        if buf.is_empty() {
            None
        } else if buf.len() < size {
            if truncate { None } else { Some(buf) }
        } else {
            Some(buf)
        }
    })
}

pub fn iterate_index<T>(
    data: impl Iterator<Item = T>,
    size: usize,
    truncate: Option<bool>,
) -> impl Iterator<Item = (usize, Vec<T>)> {
    let truncate = truncate.unwrap_or(true);
    let mut iter = data.peekable();
    let mut offset = 0;

    std::iter::from_fn(move || {
        let buf: Vec<T> = iter
            .by_ref()
            .take(size)
            .collect();

        if buf.is_empty() {
            None
        } else if buf.len() < size {
            if truncate {
                None
            } else {
                let current = offset;
                offset += size;
                Some((current, buf))
            }
        } else {
            let current = offset;
            offset += size;
            Some((current, buf))
        }
    })
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
