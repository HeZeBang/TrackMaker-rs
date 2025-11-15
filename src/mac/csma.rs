use std::collections::VecDeque;
use std::time::{Duration, Instant};
use rand::Rng;
use tracing::{info, warn};

use crate::phy::{Frame, FrameType};
use crate::utils::consts::{ACK_TIMEOUT_MS, MAX_BACKOFF_MS, CARRIER_SENSE_SAMPLES, CARRIER_SENSE_THRESHOLD};

#[derive(Debug, PartialEq, Clone)]
pub enum CsmaState {
    Idle,
    Sensing,
    Transmitting(Frame),
    WaitingForAck(Instant),
    Backoff(Instant),
}

pub struct Csma {
    pub state: CsmaState,
    addr: u8,
    dest_addr: u8,
    tx_queue: VecDeque<Frame>,
    rx_queue: VecDeque<Frame>,
    current_seq: u8,
    expected_ack_seq: u8,
}

impl Csma {
    pub fn new(addr: u8, dest_addr: u8) -> Self {
        Self {
            state: CsmaState::Idle,
            addr,
            dest_addr,
            tx_queue: VecDeque::new(),
            rx_queue: VecDeque::new(),
            current_seq: 0,
            expected_ack_seq: 0,
        }
    }

    pub fn queue_data_for_send(&mut self, data: Vec<u8>) {
        let frame = Frame::new_data(self.current_seq, self.addr, self.dest_addr, data);
        self.tx_queue.push_back(frame);
        self.current_seq = self.current_seq.wrapping_add(1);
        info!("Queued frame with seq {} for sending.", self.current_seq.wrapping_sub(1));
    }

    pub fn get_received_data(&mut self) -> Option<Frame> {
        self.rx_queue.pop_front()
    }

    pub fn handle_received_frame(&mut self, frame: Frame) {
        if frame.dst != self.addr {
            // Not for us
            return;
        }

        match frame.frame_type {
            FrameType::Data => {
                info!("Received DATA frame seq: {}", frame.sequence);
                // TODO: Handle duplicate frames
                self.rx_queue.push_back(frame);
            }
            FrameType::Ack => {
                if let CsmaState::WaitingForAck(_) = self.state {
                    if frame.sequence == self.expected_ack_seq {
                        info!("✅ ACK received for seq: {}", frame.sequence);

                        // Remove the successfully acknowledged frame from the TX queue.
                        // We expect the frame we just transmitted to be at the front of the queue
                        // (it was push_front'ed when entering Transmitting state).
                        if let Some(front) = self.tx_queue.front() {
                            if front.sequence == self.expected_ack_seq {
                                self.tx_queue.pop_front();
                            } else {
                                warn!(
                                    "ACK seq {} does not match tx_queue front seq {}",
                                    self.expected_ack_seq,
                                    front.sequence
                                );
                            }
                        }

                        // If there are more frames to send, go back to sensing;
                        // otherwise become idle.
                        if self.tx_queue.is_empty() {
                            self.state = CsmaState::Idle;
                        } else {
                            self.state = CsmaState::Sensing;
                        }
                    } else {
                        warn!(
                            "Received unexpected ACK seq: {}, expected: {}",
                            frame.sequence, self.expected_ack_seq
                        );
                    }
                }
            }
        }
    }

    pub fn tick(&mut self, is_channel_busy: bool) -> Option<Frame> {
        match self.state.clone() {
            CsmaState::Idle => {
                if !self.tx_queue.is_empty() {
                    self.state = CsmaState::Sensing;
                }
                None
            }
            CsmaState::Sensing => {
                if !is_channel_busy {
                    if let Some(frame) = self.tx_queue.pop_front() {
                        info!("Channel is idle. Transmitting frame seq: {}", frame.sequence);
                        self.expected_ack_seq = frame.sequence;
                        self.state = CsmaState::Transmitting(frame.clone());
                        return Some(frame);
                    } else {
                        self.state = CsmaState::Idle;
                    }
                } else {
                    info!("Channel is busy. Backing off.");
                    self.set_backoff();
                }
                None
            }
            CsmaState::Transmitting(frame) => {
                self.state = CsmaState::WaitingForAck(Instant::now());
                self.tx_queue.push_front(frame); // Put it back in case we need to retransmit
                None
            }
            CsmaState::WaitingForAck(start_time) => {
                if start_time.elapsed() > Duration::from_millis(ACK_TIMEOUT_MS) {
                    warn!("ACK timeout for seq: {}", self.expected_ack_seq);
                    self.set_backoff();
                }
                None
            }
            CsmaState::Backoff(end_time) => {
                if Instant::now() >= end_time {
                    info!("Backoff finished. Sensing channel.");
                    self.state = CsmaState::Sensing;
                }
                None
            }
        }
    }

    fn set_backoff(&mut self) {
        let backoff_ms = rand::thread_rng().gen_range(0..MAX_BACKOFF_MS);
        let backoff_duration = Duration::from_millis(backoff_ms);
        self.state = CsmaState::Backoff(Instant::now() + backoff_duration);
        info!("Backing off for {} ms.", backoff_ms);
    }
}

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
