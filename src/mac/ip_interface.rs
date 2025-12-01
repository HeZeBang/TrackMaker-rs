use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, info, trace, warn};

use crate::audio::recorder::{AppShared, AppState};
use crate::mac::{self, CSMAState};
use crate::phy::{Frame, FrameType, LineCodingKind, PhyDecoder, PhyEncoder};
use crate::utils::consts::*;

pub struct IpInterface {
    shared: AppShared,
    encoder: PhyEncoder,
    decoder: PhyDecoder,
    local_mac: u8,
    sample_rate: u32,
}

impl IpInterface {
    pub fn new(
        shared: AppShared,
        sample_rate: u32,
        line_coding: LineCodingKind,
        local_mac: u8,
    ) -> Self {
        let encoder = PhyEncoder::new(
            SAMPLES_PER_LEVEL,
            PREAMBLE_PATTERN_BYTES,
            line_coding,
        );
        let decoder = PhyDecoder::new(
            SAMPLES_PER_LEVEL,
            PREAMBLE_PATTERN_BYTES,
            line_coding,
            local_mac,
        );

        Self {
            shared,
            encoder,
            decoder,
            local_mac,
            sample_rate,
        }
    }

    // Send a packet for the given destination MAC address
    pub fn send_packet(
        &mut self,
        data: &[u8],
        dest_mac: u8,
        frame_type: FrameType,
    ) -> Result<(), String> {
        // Create frame
        let frame = if let FrameType::Ack = frame_type {
            Frame::new_ack_mix(0, self.local_mac, dest_mac, data.to_vec())
        } else {
            Frame::new_data(0, self.local_mac, dest_mac, data.to_vec())
        };
        let frames = vec![frame.clone()];

        let mut state = CSMAState::Transmitting;
        let mut stage = 0;

        // Start recording for sensing
        *self
            .shared
            .app_state
            .lock()
            .unwrap() = AppState::Recording;

        'csma_loop: loop {
            match state {
                CSMAState::Sensing => {
                    trace!("Sensing channel...");
                    std::thread::sleep(Duration::from_millis(
                        ENERGY_DETECTION_SAMPLES as u64 * 1000
                            / self.sample_rate as u64,
                    ));

                    let recorded_samples = {
                        self.shared
                            .record_buffer
                            .lock()
                            .unwrap()
                            .clone()
                    };

                    match mac::is_channel_busy(&recorded_samples) {
                        Some(true) => {
                            trace!("Channel busy.");
                            self.shared
                                .record_buffer
                                .lock()
                                .unwrap()
                                .clear();
                        }
                        Some(false) => {
                            state = CSMAState::WaitingForDIFS;
                            self.shared
                                .record_buffer
                                .lock()
                                .unwrap()
                                .clear();
                        }
                        None => continue,
                    }
                }
                CSMAState::WaitingForDIFS => {
                    trace!("Waiting for DIFS...");
                    std::thread::sleep(Duration::from_millis(DIFS_DURATION_MS));

                    match mac::is_channel_busy(&{
                        self.shared
                            .record_buffer
                            .lock()
                            .unwrap()
                            .clone()
                    }) {
                        Some(false) => {
                            let cw = (CW_MIN as u16 * 2_u16 * (stage))
                                .min(CW_MAX as u16)
                                as usize;
                            state =
                                CSMAState::Backoff(rand::random_range(0..=cw));
                            self.shared
                                .record_buffer
                                .lock()
                                .unwrap()
                                .clear();
                        }
                        Some(true) => {
                            state = CSMAState::Sensing;
                            self.shared
                                .record_buffer
                                .lock()
                                .unwrap()
                                .clear();
                        }
                        None => {}
                    }
                }
                CSMAState::Backoff(mut counter) => {
                    if counter > 0 {
                        std::thread::sleep(Duration::from_millis(SLOT_TIME_MS));
                        match mac::is_channel_busy(&{
                            self.shared
                                .record_buffer
                                .lock()
                                .unwrap()
                                .clone()
                        }) {
                            Some(true) => {
                                state = CSMAState::BackoffPaused(counter);
                            }
                            Some(false) => {
                                self.shared
                                    .record_buffer
                                    .lock()
                                    .unwrap()
                                    .clear();
                                counter -= 1;
                                state = CSMAState::Backoff(counter);
                            }
                            None => {}
                        }
                    } else {
                        state = CSMAState::Transmitting;
                    }
                }
                CSMAState::BackoffPaused(counter) => {
                    std::thread::sleep(Duration::from_millis(DIFS_DURATION_MS));
                    match mac::is_channel_busy(&{
                        self.shared
                            .record_buffer
                            .lock()
                            .unwrap()
                            .clone()
                    }) {
                        Some(true) => {
                            self.shared
                                .record_buffer
                                .lock()
                                .unwrap()
                                .clear();
                            state = CSMAState::BackoffPaused(counter);
                        }
                        Some(false) => {
                            self.shared
                                .record_buffer
                                .lock()
                                .unwrap()
                                .clear();
                            state = CSMAState::Backoff(counter);
                        }
                        None => {}
                    }
                }
                CSMAState::Transmitting => {
                    debug!("Transmitting frame...");
                    let output_track = self
                        .encoder
                        .encode_frames(&frames, INTER_FRAME_GAP_SAMPLES);

                    {
                        let mut playback = self
                            .shared
                            .playback_buffer
                            .lock()
                            .unwrap();
                        playback.clear();
                        playback.extend(output_track);
                        self.shared
                            .record_buffer
                            .lock()
                            .unwrap()
                            .clear();
                    }

                    *self
                        .shared
                        .app_state
                        .lock()
                        .unwrap() = AppState::Playing;

                    while let AppState::Playing = {
                        self.shared
                            .app_state
                            .lock()
                            .unwrap()
                            .clone()
                    } {
                        std::thread::sleep(Duration::from_millis(1));
                    }

                    *self
                        .shared
                        .app_state
                        .lock()
                        .unwrap() = AppState::Recording;
                    // state = CSMAState::WaitingForAck
                    return Ok(());
                }
                CSMAState::WaitingForAck => {
                    let start = Instant::now();
                    let timeout = Duration::from_millis(ACK_TIMEOUT_MS);
                    let mut processed_len = 0;

                    loop {
                        if start.elapsed() > timeout {
                            warn!("ACK timeout, retrying...");
                            stage = (stage + 1).min(10);
                            let cw = (CW_MIN as u16 * 2_u16 * (stage))
                                .min(CW_MAX as u16)
                                as usize;
                            state =
                                CSMAState::Backoff(rand::random_range(0..=cw));
                            break;
                        }

                        std::thread::sleep(Duration::from_millis(10));
                        let samples = self
                            .shared
                            .record_buffer
                            .lock()
                            .unwrap()
                            .clone();

                        if samples.len() > processed_len {
                            let new_samples = &samples[processed_len..];
                            let decoded = self
                                .decoder
                                .process_samples(new_samples);
                            processed_len = samples.len();

                            for f in decoded {
                                if f.frame_type == FrameType::Ack
                                    && f.sequence == 0
                                {
                                    debug!("ACK received!");
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
                CSMAState::Idle => unreachable!(),
            }
        }
    }

    pub fn receive_packet(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<Vec<u8>, String> {
        *self
            .shared
            .app_state
            .lock()
            .unwrap() = AppState::Recording;
        let start = Instant::now();
        let mut processed_len = 0;

        loop {
            if let Some(t) = timeout {
                if start.elapsed() > t {
                    return Err("Timeout".to_string());
                }
            }

            std::thread::sleep(Duration::from_millis(1));

            // Check for user interrupt or logic to stop?
            // For now just loop

            let samples = self
                .shared
                .record_buffer
                .lock()
                .unwrap()
                .clone();
            if samples.len() > processed_len {
                let new_samples = &samples[processed_len..];
                let decoded = self
                    .decoder
                    .process_samples(new_samples);
                processed_len = samples.len();

                for f in decoded {
                    if f.frame_type == FrameType::Data || f.frame_type == FrameType::Ack && !f.data.is_empty() {
                        return Ok(f.data);
                    }
                }
            }
        }
    }
}
