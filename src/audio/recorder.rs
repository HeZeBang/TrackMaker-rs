use jack;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub enum AppState {
    Recording,
    Playing,
    Idle,
    RecordingAndPlaying,
}

/// Thread-safe shared state
#[derive(Clone)]
pub struct AppShared {
    pub record_buffer: Arc<Mutex<Vec<f32>>>,
    pub playback_buffer: Arc<Mutex<VecDeque<f32>>>,
    pub app_state: Arc<Mutex<AppState>>,
    pub sample_counter: Arc<Mutex<usize>>,
}

impl AppShared {
    pub fn new(capacity_samples: usize) -> Self {
        Self {
            record_buffer: Arc::new(Mutex::new(Vec::with_capacity(
                capacity_samples,
            ))),
            playback_buffer: Arc::new(Mutex::new(VecDeque::new())),
            app_state: Arc::new(Mutex::new(AppState::Idle)),
            sample_counter: Arc::new(Mutex::new(0usize)),
        }
    }
}

pub fn build_process_closure(
    in_port: jack::Port<jack::AudioIn>,
    mut out_port: jack::Port<jack::AudioOut>,
    shared: AppShared,
    recording_duration_samples: usize,
) -> impl FnMut(&jack::Client, &jack::ProcessScope) -> jack::Control + Send + 'static
{
    let shared_cb = shared.clone();

    let process_cb =
        move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
            let in_buffer = in_port.as_slice(ps);
            let out_buffer = out_port.as_mut_slice(ps);

            for sample in out_buffer.iter_mut() {
                *sample = 0.0;
            }

            let current_state = {
                let state = shared_cb
                    .app_state
                    .lock()
                    .unwrap();
                state.clone()
            };

            match current_state {
                AppState::Recording => {
                    let mut recorded = shared_cb
                        .record_buffer
                        .lock()
                        .unwrap();
                    let mut counter = shared_cb
                        .sample_counter
                        .lock()
                        .unwrap();

                    for &sample in in_buffer {
                        if recorded.len() < recording_duration_samples {
                            recorded.push(sample);
                            *counter += 1;
                        } else {
                            let mut state = shared_cb
                                .app_state
                                .lock()
                                .unwrap();
                            *state = AppState::Idle;
                            break;
                        }
                    }

                    // out_buffer.copy_from_slice(in_buffer);
                }
                AppState::Playing => {
                    let mut playback = shared_cb
                        .playback_buffer
                        .lock()
                        .unwrap();
                    for out_sample in out_buffer.iter_mut() {
                        if let Some(sample) = playback.pop_front() {
                            *out_sample = sample;
                        } else {
                            let mut state = shared_cb
                                .app_state
                                .lock()
                                .unwrap();
                            *state = AppState::Idle;
                            break;
                        }
                    }
                }
                AppState::Idle => {}
                AppState::RecordingAndPlaying => {
                    // Record: in_buffer -> record_buffer
                    let mut recorded = shared_cb
                        .record_buffer
                        .lock()
                        .unwrap();

                    let mut counter = shared_cb
                        .sample_counter
                        .lock()
                        .unwrap();

                    for &sample in in_buffer {
                        if recorded.len() < recording_duration_samples {
                            recorded.push(sample);
                            *counter += 1;
                        } else {
                            break;
                        }
                    }

                    // Play: playback_buffer -> out_buffer
                    let mut playback = shared_cb
                        .playback_buffer
                        .lock()
                        .unwrap();

                    for out_sample in out_buffer.iter_mut() {
                        if let Some(sample) = playback.pop_front() {
                            *out_sample = sample;
                        } else {
                            let mut state = shared_cb
                                .app_state
                                .lock()
                                .unwrap();
                            *state = AppState::Idle;
                            break;
                        }
                    }
                }
            }

            jack::Control::Continue
        };

    process_cb
}
