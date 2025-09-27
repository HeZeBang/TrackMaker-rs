use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::path::Path;

pub fn write_to_txt(samples: &[f32], path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    for &sample in samples {
        writeln!(file, "{:.4}", sample)?;
    }
    Ok(())
}

pub fn write_int_to_txt(values: &[usize], path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    for &value in values {
        writeln!(file, "{}", value)?;
    }
    Ok(())
}

pub fn read_from_txt(path: &Path) -> io::Result<Vec<f32>> {
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);
    reader
        .lines()
        .map(|line| {
            let line = line?;
            line.trim()
                .parse::<f32>()
                .map_err(|err| {
                    io::Error::new(io::ErrorKind::InvalidData, format!("{err}"))
                })
        })
        .collect()
}

pub fn read_binary_file(path: &Path) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(buffer)
}

pub fn write_binary_file(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    file.write_all(data)
}

pub fn append_binary_bits(path: &Path, data: &[i32]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)?;
    for &value in data {
        write!(file, "{}", value)?;
    }
    file.flush()
}

pub fn write_to_wav(
    signal: &[f32],
    sample_rate: u32,
    filename: &Path,
) -> io::Result<()> {
    if let Some(parent) = filename.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(filename, spec)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{err}")))?;
    let amplitude = i16::MAX as f32;
    for &sample in signal {
        writer
            .write_sample((sample * amplitude) as i16)
            .map_err(|err| {
                io::Error::new(io::ErrorKind::Other, format!("{err}"))
            })?;
    }
    writer
        .finalize()
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{err}")))?;
    Ok(())
}

pub fn read_wav(filename: &Path) -> io::Result<Vec<f32>> {
    let reader = hound::WavReader::open(filename)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{err}")))?;
    let spec = reader.spec();

    match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .map(|sample| {
                sample.map_err(|err| {
                    io::Error::new(io::ErrorKind::Other, format!("{err}"))
                })
            })
            .collect(),
        hound::SampleFormat::Int => {
            let amplitude = (1i64
                << (spec
                    .bits_per_sample
                    .saturating_sub(1))) as f32;
            reader
                .into_samples::<i32>()
                .map(|sample| {
                    sample
                        .map(|value| value as f32 / amplitude)
                        .map_err(|err| {
                            io::Error::new(
                                io::ErrorKind::Other,
                                format!("{err}"),
                            )
                        })
                })
                .collect()
        }
    }
}
