use std::collections::VecDeque;

use std::sync::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::{
    audio::{recorder},
    mac,
    phy::{Frame, FrameType, LineCodingKind, PhyDecoder, PhyEncoder},
    ui::progress::{ProgressManager},
    utils::consts::*,
};
use tracing::{debug, error, info, trace, warn};

pub struct CsmaNode {
    shared: recorder::AppShared,
    progress_manager: Arc<Mutex<ProgressManager>>,
    encoder: PhyEncoder,
    decoder: PhyDecoder,
    sample_rate: u32,
    local_addr: mac::types::MacAddr,
    remote_addr: mac::types::MacAddr,
}

impl CsmaNode {
    pub fn new(
        shared: recorder::AppShared,
        progress_manager: Arc<Mutex<ProgressManager>>,
        sample_rate: u32,
        line_coding: LineCodingKind,
        local_mac: mac::types::MacAddr,
        remote_mac: mac::types::MacAddr,
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
            progress_manager,
            encoder,
            decoder,
            sample_rate,
            local_addr: local_mac,
            remote_addr: remote_mac,
        }
    }
    
    pub fn run_sender_loop(
        &mut self,
        tx_timeout: u64,
        queue: crossbeam_channel::Receiver<Vec<u8>>,
    ) {
        let overall_start_time = std::time::Instant::now();
        let mut seq = 0u8;
        let mut state = mac::CSMAState::Idle;

        while let Ok(chunk) = queue.recv() {
            let frame =
                Frame::new_data(seq, self.local_addr, self.remote_addr, chunk);
            seq = seq.wrapping_add(1);
            state = mac::CSMAState::Sensing;
            *self
                .shared
                .app_state
                .lock()
                .unwrap() = recorder::AppState::Recording;
            let mut stage = 0;

            'csma_loop: loop {
                match state {
                    mac::CSMAState::Sensing => {
                        trace!("Sensing channel for idleness...");
                        std::thread::sleep(std::time::Duration::from_millis(
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
                                trace!("Channel busy detected during sensing.");
                                self.shared
                                    .record_buffer
                                    .lock()
                                    .unwrap()
                                    .clear();
                            }
                            Some(false) => {
                                state = mac::CSMAState::WaitingForDIFS;
                                self.shared
                                    .record_buffer
                                    .lock()
                                    .unwrap()
                                    .clear();
                            }
                            None => {
                                trace!(
                                    "Not enough samples to determine channel state during sensing."
                                );
                                continue 'csma_loop;
                            }
                        }
                    }
                    mac::CSMAState::Backoff(mut counter) => {
                        trace!("Backoff counter: {}", counter);
                        if counter > 0 {
                            std::thread::sleep(
                                std::time::Duration::from_millis(SLOT_TIME_MS),
                            );
                            match mac::is_channel_busy(&{
                                self.shared
                                    .record_buffer
                                    .lock()
                                    .unwrap()
                                    .clone()
                            }) {
                                Some(true) => {
                                    trace!(
                                        "Channel busy detected during backoff."
                                    );
                                    state =
                                        mac::CSMAState::BackoffPaused(counter);
                                }
                                Some(false) => {
                                    // Channel idle, continue countdown
                                    self.shared
                                        .record_buffer
                                        .lock()
                                        .unwrap()
                                        .clear();
                                    counter -= 1;
                                    state = mac::CSMAState::Backoff(counter);
                                }
                                None => {
                                    trace!(
                                        "Not enough samples to determine channel state during backoff."
                                    );
                                }
                            }
                        } else {
                            state = mac::CSMAState::Transmitting;
                        }
                    }
                    mac::CSMAState::BackoffPaused(counter) => {
                        trace!("Backoff paused at counter {}", counter);
                        // 等待一个 DIFS 周期
                        std::thread::sleep(std::time::Duration::from_millis(
                            DIFS_DURATION_MS,
                        ));
                        match mac::is_channel_busy(&{
                            self.shared
                                .record_buffer
                                .lock()
                                .unwrap()
                                .clone()
                        }) {
                            Some(true) => {
                                trace!(
                                    "Channel still busy during backoff pause."
                                );
                                self.shared
                                    .record_buffer
                                    .lock()
                                    .unwrap()
                                    .clear();
                                state = mac::CSMAState::BackoffPaused(counter);
                            }
                            Some(false) => {
                                trace!("Channel idle again, resuming backoff.");
                                self.shared
                                    .record_buffer
                                    .lock()
                                    .unwrap()
                                    .clear();
                                state = mac::CSMAState::Backoff(counter);
                            }
                            None => {
                                trace!(
                                    "Not enough samples {} to determine channel state during backoff pause.",
                                    &{
                                        self.shared
                                            .record_buffer
                                            .lock()
                                            .unwrap()
                                            .len()
                                    }
                                );
                            }
                        }
                    }
                    mac::CSMAState::WaitingForDIFS => {
                        trace!("Channel idle, waiting for DIFS...");
                        std::thread::sleep(std::time::Duration::from_millis(
                            DIFS_DURATION_MS,
                        ));

                        match mac::is_channel_busy(&{
                            self.shared
                                .record_buffer
                                .lock()
                                .unwrap()
                                .clone()
                        }) {
                            Some(false) => {
                                trace!(
                                    "DIFS wait is over and channel is still idle. Starting backoff."
                                );
                                let cw = (CW_MIN as u16 * 2_u16 * (stage))
                                    .min(CW_MAX as u16)
                                    as usize;
                                state = mac::CSMAState::Backoff(
                                    rand::random_range(0..=cw),
                                );
                                self.shared
                                    .record_buffer
                                    .lock()
                                    .unwrap()
                                    .clear();
                            }
                            Some(true) => {
                                trace!(
                                    "Channel became busy during DIFS wait. Returning to sensing."
                                );
                                state = mac::CSMAState::Sensing;
                                self.shared
                                    .record_buffer
                                    .lock()
                                    .unwrap()
                                    .clear();
                            }
                            None => {
                                trace!(
                                    "Not enough samples to determine channel state after DIFS wait."
                                );
                            }
                        }
                    }
                    mac::CSMAState::Transmitting => {
                        trace!(
                            "Channel idle, proceeding to transmit frame seq: {}",
                            frame.sequence
                        );
                        // 1. Encode and send the frame
                        let output_track = self.encoder.encode_frames(
                            &[frame.clone()],
                            INTER_FRAME_GAP_SAMPLES,
                        );
                        {
                            let mut playback = self
                                .shared
                                .playback_buffer
                                .lock()
                                .unwrap();
                            playback.clear();
                            playback.extend(output_track);
                            {
                                // Clear previous recordings before listening for ACK
                                let mut rec_buf = self
                                    .shared
                                    .record_buffer
                                    .lock()
                                    .unwrap();
                                rec_buf.clear();
                            }
                        }
                        *self
                            .shared
                            .app_state
                            .lock()
                            .unwrap() = recorder::AppState::Playing;

                        // Wait for playback to finish
                        while let recorder::AppState::Playing = {
                            self.shared
                                .app_state
                                .lock()
                                .unwrap()
                                .clone()
                        } {
                            std::thread::sleep(
                                std::time::Duration::from_millis(1),
                            );
                        }
                        debug!(
                            "Frame {} sent, waiting for ACK...",
                            frame.sequence
                        );

                        // 2. Switch to recording to wait for ACK
                        *self
                            .shared
                            .app_state
                            .lock()
                            .unwrap() = recorder::AppState::Recording;
                        state = mac::CSMAState::WaitingForAck;
                    }
                    mac::CSMAState::WaitingForAck => {
                        let mut processed_samples_len = 0;
                        let ack_wait_start = std::time::Instant::now();
                        // Timeout for ACK
                        let ack_timeout =
                            std::time::Duration::from_millis(ACK_TIMEOUT_MS);

                        // 3. ACK waiting loop
                        'ack_wait_loop: loop {
                            if ack_wait_start.elapsed() > ack_timeout {
                                warn!(
                                    "ACK timeout for seq: {}, stage {}",
                                    frame.sequence, stage
                                );
                                stage = (stage + 1).min(20);
                                let cw = (CW_MIN as u16 * 2_u16 * (stage))
                                    .min(CW_MAX as u16)
                                    as usize; // Not BEB
                                warn!("Random range to {}", cw);
                                state = mac::CSMAState::Backoff(
                                    rand::random_range(0..=cw),
                                );
                                break 'ack_wait_loop; // Timed out, retransmit
                            }

                            std::thread::sleep(
                                std::time::Duration::from_millis(10),
                            );

                            let current_samples = {
                                self.shared
                                    .record_buffer
                                    .lock()
                                    .unwrap()
                                    .clone()
                            };

                            if current_samples.len() > processed_samples_len {
                                let new_samples =
                                    &current_samples[processed_samples_len..];
                                let decoded_frames = self
                                    .decoder
                                    .process_samples(new_samples);
                                processed_samples_len = current_samples.len();

                                for ack_frame in decoded_frames {
                                    if ack_frame.frame_type == FrameType::Ack
                                        && ack_frame.sequence == frame.sequence
                                    {
                                        debug!(
                                            "ACK received for seq: {}",
                                            frame.sequence
                                        );
                                        // frames_sent += 1;
                                        self.progress_manager
                                            .lock()
                                            .unwrap()
                                            .inc("sender", 1)
                                            .unwrap();
                                        break 'csma_loop; // ACK OK, send next frame
                                    } else {
                                        warn!(
                                            "Received unexpected frame while waiting for ACK {}: type={:?}, seq={}",
                                            frame.sequence,
                                            ack_frame.frame_type,
                                            ack_frame.sequence
                                        );
                                    }
                                }
                            }
                        } // end ack_wait_loop
                    }
                    mac::CSMAState::Idle => unreachable!(),
                } // end retransmit_loop
            } // end csma_loop
        } // end for frame_to_send

        self.progress_manager
            .lock()
            .unwrap()
            .finish("sender", "All frames acknowledged")
            .unwrap();
        let total_duration = overall_start_time
            .elapsed()
            .as_secs_f32();
        info!(
            "🎉 All {} frames transmitted and acknowledged in {:.2} seconds.",
            seq, total_duration
        );
    }

    pub fn run_receiver_loop(
        &mut self,
        max_recording_duration_samples: u32,
        rx_duration: u64,
        tx: crossbeam_channel::Sender<Vec<u8>>,
    ) {
        info!("=== Receiver Mode ===");

        let mut received_sequences = std::collections::HashSet::new();
        let mut processed_samples_len = 0;

        *self
            .shared
            .app_state
            .lock()
            .unwrap() = recorder::AppState::Recording;

        let start_time = std::time::Instant::now();
        let recording_timeout = std::time::Duration::from_secs(rx_duration);

        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();

        // Ctrl+C 设置标志
        ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
        })
        .expect("Error setting Ctrl-C handler");

        'main_loop: loop {
            if !running.load(Ordering::SeqCst) {
                break;
            }

            // Check for overall timeout
            if start_time.elapsed() > recording_timeout {
                info!("Receiver timeout reached. Exiting.");
                break 'main_loop;
            }

            // Wait for some audio to be recorded
            std::thread::sleep(std::time::Duration::from_millis(25));

            if self
                .shared
                .record_buffer
                .lock()
                .unwrap()
                .len()
                > 50
            {
                let new_samples = &self
                    .shared
                    .record_buffer
                    .lock()
                    .unwrap()
                    .drain(..)
                    .collect::<Vec<_>>()[..];
                let decoded_frames = self
                    .decoder
                    .process_samples(new_samples);
                processed_samples_len += new_samples.len();

                for frame in decoded_frames {
                    if frame.frame_type == FrameType::Data {
                        if !received_sequences.contains(&frame.sequence) {
                            debug!(
                                "Received new DATA frame with seq: {}",
                                frame.sequence
                            );
                            // Store data and mark sequence as received
                            tx.send(frame.data).unwrap_or_else(|err| {
                                error!("Error while sending received frame: {:?}", err)
                            });
                            received_sequences.insert(frame.sequence);
                        } else {
                            info!(
                                "Received duplicate DATA frame with seq: {}, re-sending ACK.",
                                frame.sequence
                            );
                        }

                        // Always send an ACK for a data frame
                        debug!("Sending ACK for seq: {}", frame.sequence);
                        let ack_frame = Frame::new_ack(
                            frame.sequence,
                            self.local_addr,
                            self.remote_addr,
                        );
                        let ack_track = self
                            .encoder
                            .encode_frames(&[ack_frame], 0);

                        // Put ACK in playback buffer
                        {
                            let mut playback = self
                                .shared
                                .playback_buffer
                                .lock()
                                .unwrap();
                            playback.clear();
                            playback.extend(ack_track);
                        }

                        // Switch to playing state
                        *self
                            .shared
                            .app_state
                            .lock()
                            .unwrap() = recorder::AppState::Playing;

                        // Wait for ACK playback to complete
                        while let recorder::AppState::Playing = {
                            self.shared
                                .app_state
                                .lock()
                                .unwrap()
                                .clone()
                        } {
                            std::thread::sleep(
                                std::time::Duration::from_millis(1),
                            );
                        }
                        debug!("ACK sent for seq: {}", frame.sequence);

                        {
                            self.shared
                                .record_buffer
                                .lock()
                                .unwrap()
                                .clear();
                        }

                        // After sending ACK, switch back to recording for the next frame
                        *self
                            .shared
                            .app_state
                            .lock()
                            .unwrap() = recorder::AppState::Recording;
                        debug!("Switched back to recording mode.");
                    }
                } // end for frame
            } // end if new samples

            self.progress_manager
                .lock()
                .unwrap()
                .set_position("recording", processed_samples_len as u64)
                .unwrap();

            // Check if user manually stopped
            let state = {
                self.shared
                    .app_state
                    .lock()
                    .unwrap()
                    .clone()
            };
            if let recorder::AppState::Idle = state {
                info!("Recording finished by user or duration limit.");
                break 'main_loop;
            }
        } // end main_loop

        drop(tx); // Close the channel

        let elapsed = start_time
            .elapsed()
            .as_secs_f32();
        info!("Receiver loop finished in {:.2} seconds", elapsed);
        self.progress_manager
            .lock()
            .unwrap()
            .finish("recording", "Finished")
            .unwrap();

        // // Final processing for any remaining samples
        // let final_samples = {
        //     let buffer = self
        //         .shared
        //         .record_buffer
        //         .lock()
        //         .unwrap();
        //     buffer.clone()
        // };
        // if !final_samples.is_empty() {
        //     let decoded_frames = self
        //         .decoder
        //         .process_samples(&final_samples);
        //     for frame in decoded_frames {
        //         if frame.frame_type == FrameType::Data
        //             && !received_sequences.contains(&frame.sequence)
        //         {
        //             info!(
        //                 "Decoded final DATA frame with seq: {}",
        //                 frame.sequence
        //             );
        //             self.rx_queue
        //                 .lock()
        //                 .unwrap()
        //                 .push_back(frame.data.clone());
        //             received_sequences.insert(frame.sequence);
        //         }
        //     }
        // }

        info!(
            "Total unique data frames received: {}",
            received_sequences.len()
        );
    }
}
