use crate::audio::recorder::{AppShared, AppState};
pub mod progress;
use crate::ui::progress::ProgressManager;

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
            // 防止下溢：确保 remaining_samples 不会超过 recording_duration_samples
            let played_samples =
                if remaining_samples > recording_duration_samples {
                    0 // 如果剩余样本超过总时长，说明刚开始播放，设为0
                } else {
                    recording_duration_samples - remaining_samples
                };
            let _ =
                progress_manager.set_position("playback", played_samples as u64);
        }
        AppState::RecordingAndPlaying => {
            let recorded_samples = {
                let recorded = shared
                    .record_buffer
                    .lock()
                    .unwrap();
                recorded.len()
            };
            let _ = progress_manager
                .set_position("playrec", recorded_samples as u64);
        }
        AppState::Idle => {}
    }
}
