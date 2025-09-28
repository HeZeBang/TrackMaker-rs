use std::io::{Read, Write};
use crate::amodem::{
    config::Configuration,
    detect::Detector,
    recv::Receiver,
    common,
};

pub fn recv<R: Read, W: Write>(
    config: &Configuration,
    mut src: R,
    dst: W,
) -> Result<bool, String> {
    // Read PCM data
    let mut data = Vec::new();
    src.read_to_end(&mut data).map_err(|e| e.to_string())?;
    
    // Convert PCM data to float samples
    let samples = common::loads(&data);
    
    eprintln!("Waiting for carrier tone: {:.1} kHz", config.fc / 1e3);
    
    // Skip initial silence (like Python's skip_start)
    let skip_start = (config.skip_start * config.fs) as usize;
    let samples_after_skip = if samples.len() > skip_start {
        &samples[skip_start..]
    } else {
        &samples[..]
    };
    
    // Detect carrier
    let detector = Detector::new(config);
    let (signal, amplitude, freq_error) = detector.run(samples_after_skip.iter().cloned())?;
    
    // Frequency and gain correction
    let freq = 1.0 / (1.0 + freq_error);
    eprintln!("Frequency correction: {:.3} ppm", (freq - 1.0) * 1e6);
    
    let gain = 1.0 / amplitude;
    eprintln!("Gain correction: {:.3}", gain);
    
    // Create receiver and run
    let mut receiver = Receiver::new(config);
    receiver.run(signal, gain, dst)?;
    
    Ok(true)
}
