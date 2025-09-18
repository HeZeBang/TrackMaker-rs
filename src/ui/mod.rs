use crate::audio::recorder::{AppShared, AppState};
use crate::utils::progress::{ProgressManager, templates};

pub fn print_banner() {
    println!("TrackMaker-rs");
}

pub fn update_progress(
    shared: &AppShared,
    recording_duration_samples: usize,
    progress_manager: &ProgressManager,
) {
    let current_state = {
        let state = shared
            .app_state
            .lock()
            .unwrap();
        state.clone()
    };

    match current_state {
        AppState::Recording => {
            let recorded_samples = {
                let recorded = shared
                    .record_buffer
                    .lock()
                    .unwrap();
                recorded.len()
            };
            let _ = progress_manager
                .set_position("recording", recorded_samples as u64);
        }
        AppState::Playing => {
            let remaining_samples = {
                let playback = shared
                    .playback_buffer
                    .lock()
                    .unwrap();
                playback.len()
            };
            let played_samples =
                recording_duration_samples - remaining_samples;
            let _ = progress_manager
                .set_position("playback", played_samples as u64);
        }
        AppState::Idle => {
        }
    }
}
