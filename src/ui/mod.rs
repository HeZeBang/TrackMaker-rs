use crate::audio::{AppShared, AppState};
use crate::utils::progress::{ProgressManager, templates};

pub fn print_banner() {
    println!("TrackMaker-rs");
}

pub fn run_progress_loop(
    shared: &AppShared,
    recording_duration_samples: usize,
    progress_manager: &ProgressManager,
) {
    loop {
        std::thread::sleep(std::time::Duration::from_millis(crate::utils::consts::PROGRESS_UPDATE_INTERVAL_MS));

        let current_state = {
            let state = shared.app_state.lock().unwrap();
            state.clone()
        };

        match current_state {
            AppState::Recording => {
                let recorded_samples = {
                    let recorded = shared.recorded_audio.lock().unwrap();
                    recorded.len()
                };
                let _ = progress_manager.set_position("recording", recorded_samples as u64);
            }
            AppState::Playing => {
                if progress_manager.exists("recording") && !progress_manager.is_finished("recording").unwrap_or(true) {
                    let _ = progress_manager.finish_and_clear("recording");
                }

                if !progress_manager.exists("playback") {
                    let _ = progress_manager.create_bar(
                        "playback",
                        recording_duration_samples as u64,
                        templates::PLAYBACK,
                        "Playing...",
                    );
                }

                let remaining_samples = {
                    let playback = shared.playback_buffer.lock().unwrap();
                    playback.len()
                };
                let played_samples = recording_duration_samples - remaining_samples;
                let _ = progress_manager.set_position("playback", played_samples as u64);
            }
            AppState::Idle => {
                progress_manager.clear_all();
                break;
            }
        }
    }
}


