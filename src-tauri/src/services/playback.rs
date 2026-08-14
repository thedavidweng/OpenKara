use crate::{
    audio::{
        error::PlaybackError,
        playback::{PlaybackController, PlaybackStateSnapshot},
    },
    commands::error::CommandError,
    state::AppState,
};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::{
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, serde::Serialize)]
pub struct PlaybackErrorEvent {
    pub song_id: String,
    pub error: CommandError,
}

pub(crate) fn spawn_airplay_control_refresh_worker(
    airplay_audience_active: Arc<AtomicBool>,
    airplay_control_refresh_token: Arc<AtomicU64>,
    airplay_audio_tap: Arc<crate::airplay_stream::AirPlayAudioTap>,
    airplay_stream_generation: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
) {
    spawn_airplay_control_refresh_worker_with_timing(
        airplay_audience_active,
        airplay_control_refresh_token,
        airplay_audio_tap,
        airplay_stream_generation,
        Duration::from_millis(180),
        Duration::from_millis(25),
        shutdown,
    );
}

fn spawn_airplay_control_refresh_worker_with_timing(
    airplay_audience_active: Arc<AtomicBool>,
    airplay_control_refresh_token: Arc<AtomicU64>,
    airplay_audio_tap: Arc<crate::airplay_stream::AirPlayAudioTap>,
    airplay_stream_generation: Arc<AtomicU64>,
    debounce_window: Duration,
    poll_interval: Duration,
    shutdown: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        let mut flushed_token = 0u64;
        let mut pending_token: Option<u64> = None;
        let mut pending_since: Option<Instant> = None;

        while !shutdown.load(Ordering::Relaxed) {
            let current_token = airplay_control_refresh_token.load(Ordering::SeqCst);
            if current_token != flushed_token && pending_token != Some(current_token) {
                pending_token = Some(current_token);
                pending_since = Some(Instant::now());
            }

            if let (Some(token), Some(since)) = (pending_token, pending_since) {
                if since.elapsed() >= debounce_window {
                    flushed_token = token;
                    pending_token = None;
                    pending_since = None;
                    if airplay_audience_active.load(Ordering::SeqCst) {
                        airplay_audio_tap.bump_epoch();
                        crate::airplay_stream::notify_audio_epoch(
                            airplay_audio_tap.current_epoch(),
                        );
                        airplay_stream_generation.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }

            thread::sleep(poll_interval);
        }
    });
}

pub fn get_state(state: &AppState) -> Result<PlaybackStateSnapshot, PlaybackError> {
    let mut playback =
        state.playback.playback.lock().map_err(|_| {
            PlaybackError::Internal("playback controller lock was poisoned".to_owned())
        })?;
    Ok(playback.snapshot())
}

pub(crate) fn ensure_output_thread_inner(
    audio_output_started: &Arc<AtomicBool>,
    audio_output_start_lock: &Arc<Mutex<()>>,
    playback: Arc<Mutex<PlaybackController>>,
    airplay_audio_tap: Arc<crate::airplay_stream::AirPlayAudioTap>,
    airplay_local_output_suppressed: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    peak_ring: Arc<crate::audio::peaks::PeakRing>,
    output_format: crate::audio::output_format::OutputFormatState,
) -> Result<(), PlaybackError> {
    crate::audio::output::ensure_output_thread(
        audio_output_started,
        audio_output_start_lock,
        playback,
        airplay_audio_tap,
        airplay_local_output_suppressed,
        shutdown,
        peak_ring,
        output_format,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        airplay_stream::AirPlayAudioTap,
        audio::decode,
        commands::bootstrap,
        state::{AirPlayState, AppShell, AppState, PlaybackState, RemoteState, SeparationState},
    };
    use std::sync::RwLock;
    use std::{
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc, Arc, Mutex,
        },
        time::Duration,
    };

    fn fixture_path(directory: &str, filename: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(directory)
            .join(filename)
    }

    fn airplay_state() -> AppState {
        let decoded = decode::decode_file(&fixture_path("audio", "fixture.wav"))
            .expect("fixture audio should decode");
        let mut controller = PlaybackController::default();
        controller.start_track("song-a".to_owned(), decoded, 0);

        let (command_tx, _) = mpsc::channel();
        AppState {
            playback: PlaybackState {
                playback: Arc::new(Mutex::new(controller)),
                cdg_state: Arc::new(Mutex::new(None)),
                playback_request_id: Arc::new(AtomicU64::new(0)),
                audio_output_started: Arc::new(AtomicBool::new(true)),
                audio_output_start_lock: Arc::new(Mutex::new(())),
                background_shutdown: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
                preload_shutdown: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
                preload_request_generation: Arc::new(AtomicU64::new(0)),
                command_tx,
                peak_ring: Arc::new(crate::audio::peaks::PeakRing::new()),
                output_format: Arc::new(RwLock::new(None)),
                waveform_singleflight: crate::state::WaveformSingleflight::new(),
            },
            airplay: AirPlayState {
                airplay_audio_tap: Arc::new(AirPlayAudioTap::new(4)),
                airplay_stream_generation: Arc::new(AtomicU64::new(7)),
                airplay_audience_active: Arc::new(AtomicBool::new(false)),
                airplay_control_refresh_token: Arc::new(AtomicU64::new(0)),
                airplay_http_server: Arc::new(Mutex::new(None)),
                airplay_local_output_suppressed: Arc::new(AtomicBool::new(false)),
            },
            separation: SeparationState::new(),
            remote: RemoteState::test_fixture(),
            shell: AppShell::new(
                Arc::new(Mutex::new(None)),
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests")
                    .join("tmp"),
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests")
                    .join("fixtures"),
                PathBuf::from("model.bin"),
                Arc::new(Mutex::new(bootstrap::pending_status("model.bin"))),
                Arc::new(Mutex::new(
                    crate::commands::runtime_bootstrap::RuntimeBootstrapStatusSnapshot {
                        state: crate::commands::runtime_bootstrap::RuntimeBootstrapState::Missing,
                        runtime_path: "test.dylib".to_owned(),
                        downloaded_bytes: None,
                        total_bytes: None,
                        version: "test".to_owned(),
                        active_artifact_id: None,
                        target_triple: crate::separator::catalog::current_target_triple()
                            .to_owned(),
                        candidate_version: None,
                        restart_required: false,
                        error: None,
                        failure_phase: None,
                        cpu_fallback_notice: None,
                    },
                )),
            ),
            amll_client: crate::lyrics::amll::AmllClient::new_default(),
            lrclib_client: crate::lyrics::lrclib::LrcLibClient::new_default(),
            lrcapi_client: crate::lyrics::lrcapi::LrcApiClient::new_default(),
        }
    }

    fn wait_for_generation(generation: &AtomicU64, expected: u64, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if generation.load(Ordering::SeqCst) == expected {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        generation.load(Ordering::SeqCst) == expected
    }

    #[test]
    fn airplay_control_refresh_debounces_multiple_stem_updates() {
        let state = airplay_state();
        state
            .airplay
            .airplay_audience_active
            .store(true, Ordering::SeqCst);

        let initial_generation = state
            .airplay
            .airplay_stream_generation
            .load(Ordering::SeqCst);
        let initial_epoch = state.airplay.airplay_audio_tap.current_epoch();

        spawn_airplay_control_refresh_worker_with_timing(
            Arc::clone(&state.airplay.airplay_audience_active),
            Arc::clone(&state.airplay.airplay_control_refresh_token),
            Arc::clone(&state.airplay.airplay_audio_tap),
            Arc::clone(&state.airplay.airplay_stream_generation),
            Duration::from_millis(300),
            Duration::from_millis(5),
            Arc::clone(&state.shell.shutdown),
        );

        // Simulate stem volume updates by incrementing the refresh token
        // directly — the coordinator normally does this via SetStemVolume.
        state
            .airplay
            .airplay_control_refresh_token
            .fetch_add(1, Ordering::SeqCst);
        state
            .airplay
            .airplay_control_refresh_token
            .fetch_add(1, Ordering::SeqCst);
        state
            .airplay
            .airplay_control_refresh_token
            .fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            state
                .airplay
                .airplay_control_refresh_token
                .load(Ordering::SeqCst),
            3
        );

        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            state
                .airplay
                .airplay_stream_generation
                .load(Ordering::SeqCst),
            initial_generation
        );

        assert!(wait_for_generation(
            &state.airplay.airplay_stream_generation,
            initial_generation + 1,
            Duration::from_millis(1_500),
        ));
        assert_eq!(
            state.airplay.airplay_audio_tap.current_epoch(),
            initial_epoch + 1
        );
        state.shell.shutdown.store(true, Ordering::Relaxed);
    }

    #[test]
    fn airplay_control_refresh_debounces_volume_updates_until_user_stops_dragging() {
        let state = airplay_state();
        state
            .airplay
            .airplay_audience_active
            .store(true, Ordering::SeqCst);

        let initial_generation = state
            .airplay
            .airplay_stream_generation
            .load(Ordering::SeqCst);
        let initial_epoch = state.airplay.airplay_audio_tap.current_epoch();

        spawn_airplay_control_refresh_worker_with_timing(
            Arc::clone(&state.airplay.airplay_audience_active),
            Arc::clone(&state.airplay.airplay_control_refresh_token),
            Arc::clone(&state.airplay.airplay_audio_tap),
            Arc::clone(&state.airplay.airplay_stream_generation),
            Duration::from_millis(300),
            Duration::from_millis(5),
            Arc::clone(&state.shell.shutdown),
        );

        // Simulate volume updates by incrementing the refresh token directly.
        state
            .airplay
            .airplay_control_refresh_token
            .fetch_add(1, Ordering::SeqCst);
        state
            .airplay
            .airplay_control_refresh_token
            .fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            state
                .airplay
                .airplay_control_refresh_token
                .load(Ordering::SeqCst),
            2
        );

        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            state
                .airplay
                .airplay_stream_generation
                .load(Ordering::SeqCst),
            initial_generation
        );

        assert!(wait_for_generation(
            &state.airplay.airplay_stream_generation,
            initial_generation + 1,
            Duration::from_millis(1_500),
        ));
        assert_eq!(
            state.airplay.airplay_audio_tap.current_epoch(),
            initial_epoch + 1
        );
        state.shell.shutdown.store(true, Ordering::Relaxed);
    }

    #[test]
    fn airplay_control_refresh_does_not_fire_while_idle() {
        let state = airplay_state();
        state
            .airplay
            .airplay_audience_active
            .store(false, Ordering::SeqCst);

        let initial_generation = state
            .airplay
            .airplay_stream_generation
            .load(Ordering::SeqCst);
        let initial_epoch = state.airplay.airplay_audio_tap.current_epoch();

        spawn_airplay_control_refresh_worker_with_timing(
            Arc::clone(&state.airplay.airplay_audience_active),
            Arc::clone(&state.airplay.airplay_control_refresh_token),
            Arc::clone(&state.airplay.airplay_audio_tap),
            Arc::clone(&state.airplay.airplay_stream_generation),
            Duration::from_millis(300),
            Duration::from_millis(5),
            Arc::clone(&state.shell.shutdown),
        );

        state
            .airplay
            .airplay_control_refresh_token
            .fetch_add(1, Ordering::SeqCst);

        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(
            state
                .airplay
                .airplay_stream_generation
                .load(Ordering::SeqCst),
            initial_generation
        );
        assert_eq!(
            state.airplay.airplay_audio_tap.current_epoch(),
            initial_epoch
        );
        state.shell.shutdown.store(true, Ordering::Relaxed);
    }
}
