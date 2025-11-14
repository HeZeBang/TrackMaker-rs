use crate::utils::consts::{CARRIER_SENSE_SAMPLES, CARRIER_SENSE_THRESHOLD};

/// Checks if the channel is busy by analyzing the energy of the last few samples.
pub fn is_channel_busy(samples: &[f32]) -> bool {
    if samples.len() < CARRIER_SENSE_SAMPLES {
        return false; // Not enough data to make a decision, assume idle
    }

    let start_index = samples.len() - CARRIER_SENSE_SAMPLES;
    let recent_samples = &samples[start_index..];

    let rms = (recent_samples
        .iter()
        .map(|&s| s * s)
        .sum::<f32>()
        / CARRIER_SENSE_SAMPLES as f32)
        .sqrt();

    if rms > CARRIER_SENSE_THRESHOLD {
        tracing::trace!("Channel BUSY (rms={:.4})", rms);
        true
    } else {
        tracing::trace!("Channel IDLE (rms={:.4})", rms);
        false
    }
}
