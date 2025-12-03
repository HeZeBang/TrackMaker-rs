use std::fs::File;
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tracing::info;

pub fn decode_flac_to_f32(
    file_path: &str,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    // 打开媒体文件
    let file = File::open(file_path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    // 创建格式提示，指定文件扩展名
    let mut hint = Hint::new();
    hint.with_extension("flac");

    // 获取格式读取器
    let meta_opts: MetadataOptions = Default::default();
    let fmt_opts: FormatOptions = Default::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &fmt_opts, &meta_opts)?;
    let mut format = probed.format;

    // 查找第一个音频轨道
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("没有找到音频轨道")?;

    // 创建解码器
    let dec_opts: DecoderOptions = Default::default();
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &dec_opts)?;

    let track_id = track.id;
    let mut samples = Vec::new();

    // 解码音频包
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::ResetRequired) => {
                // 解码器需要重置，但我们可以忽略这个错误继续处理
                continue;
            }
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                // 文件结束
                break;
            }
            Err(err) => return Err(err.into()),
        };

        // 只处理我们感兴趣的轨道
        if packet.track_id() != track_id {
            continue;
        }

        // 解码数据包
        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                // 将音频缓冲区转换为 f32 样本
                convert_audio_buffer_to_f32(&audio_buf, &mut samples);
            }
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                // 解码器遇到文件结束
                break;
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => {
                // 解码错误，跳过这个包
                continue;
            }
            Err(err) => return Err(err.into()),
        }
    }

    Ok(samples)
}

fn convert_audio_buffer_to_f32(
    audio_buf: &AudioBufferRef,
    samples: &mut Vec<f32>,
) {
    // info!("Buffer spec: {:?}", audio_buf.spec());
    match audio_buf {
        AudioBufferRef::U8(buf) => {
            // Mono
            for &sample in buf.chan(0) {
                samples.push((sample as f32 - 128.0) / 128.0);
            }
        }
        AudioBufferRef::U16(buf) => {
            for &sample in buf.chan(0) {
                samples.push((sample as f32 - 32768.0) / 32768.0);
            }
        }
        AudioBufferRef::U24(buf) => {
            for &sample in buf.chan(0) {
                samples.push((sample.inner() as f32 - 8388608.0) / 8388608.0);
            }
        }
        AudioBufferRef::U32(buf) => {
            for &sample in buf.chan(0) {
                samples
                    .push((sample as f64 - 2147483648.0) as f32 / 2147483648.0);
            }
        }
        AudioBufferRef::S8(buf) => {
            for &sample in buf.chan(0) {
                samples.push(sample as f32 / 128.0);
            }
        }
        AudioBufferRef::S16(buf) => {
            for &sample in buf.chan(0) {
                samples.push(sample as f32 / 32768.0);
            }
        }
        AudioBufferRef::S24(buf) => {
            for &sample in buf.chan(0) {
                samples.push(sample.inner() as f32 / 8388608.0);
            }
        }
        AudioBufferRef::S32(buf) => {
            for &sample in buf.chan(0) {
                samples.push(sample as f32 / 2147483648.0);
            }
        }
        AudioBufferRef::F32(buf) => {
            for &sample in buf.chan(0) {
                samples.push(sample);
            }
        }
        AudioBufferRef::F64(buf) => {
            for &sample in buf.chan(0) {
                samples.push(sample as f32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_decode_flac() {
        // WIP
    }
}
