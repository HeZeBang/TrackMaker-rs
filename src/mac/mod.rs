pub mod types;

pub enum CSMAState {
    Idle, // No Data
    Sensing, // Sensing Channel
    Backoff(u8), // Backoff(i, k) at stage i, counter k
    BackoffPaused(u8), // Backoff Paused at stage i, counter k
    Transmitting, // Transmitting Frame
    WaitingForDIFS, // Waiting for DIFS
    WaitingForAck, // Waiting for ACK
}

use crate::utils::consts::{ENERGY_THRESHOLD, ENERGY_DETECTION_SAMPLES};

pub fn is_channel_busy(samples: &[f32]) -> Option<bool> {
    if samples.len() < ENERGY_DETECTION_SAMPLES {
        return None;
    }
    Some(samples.iter().any(|&s| s.abs() > ENERGY_THRESHOLD))
}