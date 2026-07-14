//! PlaybackCoordinator — a dedicated control thread that serializes all
//! control-plane mutations of `PlaybackController`.
//!
//! The CPAL audio callback remains the only realtime renderer and continues
//! to mutate render-owned state (`render_frame`, buffering, fades). Background
//! decode/fetch threads produce immutable `ReadyTrack` payloads and send
//! `PlaybackCommand`s; they never install tracks directly.
//!
//! See `docs/references/contracts/playback.md` for the public IPC contract.

use crate::{
    airplay_stream,
    audio::{
        decode::DecodedAudio,
        error::PlaybackError,
        output,
        playback::{
            monotonic_now_ms, playback_position_event, LoadedStems, PlaybackController,
            PlaybackStateSnapshot, PreparedTrack, StemName, PLAYBACK_ERROR_EVENT,
            PLAYBACK_POSITION_EVENT,
        },
        streaming::StreamingTrack,
    },
    commands::{cdg::CdgPlaybackState, error::CommandError},
    services::{cdg::mark_cdg_reset_for_seek, playback::PlaybackErrorEvent},
    state::AirPlayState,
};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread::JoinHandle;
use tauri::{Emitter, Runtime};

/// An immutable, ready-to-install track payload produced by a background
/// decode/fetch thread.
pub enum ReadyTrack {
    /// Fully decoded audio (local file, Media+G ZIP, or remote full-file fallback).
    Decoded {
        audio: DecodedAudio,
        stems: Option<LoadedStems>,
        cdg: Option<CdgPlaybackState>,
    },
    /// Streaming track (low-latency byte-range playback).
    Streaming {
        sample_rate: u32,
        channels: usize,
        duration_ms: u64,
        original: StreamingTrack,
        stems: Option<Box<StreamingTrack>>,
        cdg: Option<CdgPlaybackState>,
    },
}

/// Commands sent to the coordinator thread. Synchronous commands (Resume,
/// Pause, Seek, SetVolume, SetStemVolume, AttachStems) carry a oneshot reply
/// channel. Fire-and-forget commands (InstallReady, FailLoad) do not.
pub enum PlaybackCommand {
    BeginLoad {
        request_id: u64,
        song_id: String,
        reply: SnapshotReply,
    },
    InstallReady {
        request_id: u64,
        song_id: String,
        ready: Box<ReadyTrack>,
    },
    FailLoad {
        request_id: u64,
        song_id: String,
        error: PlaybackError,
    },
    Resume {
        reply: SnapshotReply,
    },
    Pause {
        reply: SnapshotReply,
    },
    Seek {
        target_ms: u64,
        reply: SnapshotReply,
    },
    SetVolume {
        level: f32,
        reply: SnapshotReply,
    },
    SetStemVolume {
        stem: StemName,
        level: f32,
        reply: SnapshotReply,
    },
    AttachStems {
        request_id: u64,
        song_id: String,
        stems: LoadedStems,
        reply: SnapshotReply,
    },
    SetEqEnabled {
        enabled: bool,
        reply: SnapshotReply,
    },
    SetEqGains {
        gains_db: [f32; 5],
        reply: SnapshotReply,
    },
}

type SnapshotReply = tokio::sync::oneshot::Sender<Result<PlaybackStateSnapshot, PlaybackError>>;

/// Runtime dependencies the coordinator worker needs. Generic over `R:
/// tauri::Runtime` so mock-runtime tests compile.
pub struct CoordinatorRuntime<R: Runtime> {
    pub app_handle: tauri::AppHandle<R>,
    pub playback: Arc<Mutex<PlaybackController>>,
    pub cdg_state: Arc<Mutex<Option<CdgPlaybackState>>>,
    pub latest_request_id: Arc<AtomicU64>,
    pub output_started: Arc<AtomicBool>,
    pub output_start_lock: Arc<Mutex<()>>,
    pub airplay: AirPlayState,
    pub shutdown: Arc<AtomicBool>,
    pub peak_ring: Arc<crate::audio::peaks::PeakRing>,
}

/// Spawn the coordinator worker thread. Returns the `JoinHandle` so the caller
/// can join it in tests. The receiver is moved into the worker; only the sender
/// is stored in managed state.
pub fn spawn_coordinator<R: Runtime + 'static>(
    runtime: CoordinatorRuntime<R>,
    receiver: mpsc::Receiver<PlaybackCommand>,
) -> JoinHandle<()> {
    std::thread::spawn(move || coordinator_loop(runtime, receiver))
}

fn coordinator_loop<R: Runtime>(
    runtime: CoordinatorRuntime<R>,
    receiver: mpsc::Receiver<PlaybackCommand>,
) {
    while let Ok(command) = receiver.recv() {
        if runtime.shutdown.load(Ordering::Relaxed) {
            break;
        }
        handle_command(&runtime, command);
    }
}

fn handle_command<R: Runtime>(runtime: &CoordinatorRuntime<R>, command: PlaybackCommand) {
    match command {
        PlaybackCommand::BeginLoad {
            request_id,
            song_id,
            reply,
        } => handle_begin_load(runtime, request_id, &song_id, reply),
        PlaybackCommand::InstallReady {
            request_id,
            song_id,
            ready,
        } => handle_install_ready(runtime, request_id, &song_id, *ready),
        PlaybackCommand::FailLoad {
            request_id,
            song_id,
            error,
        } => handle_fail_load(runtime, request_id, &song_id, error),
        PlaybackCommand::Resume { reply } => handle_resume(runtime, reply),
        PlaybackCommand::Pause { reply } => handle_pause(runtime, reply),
        PlaybackCommand::Seek { target_ms, reply } => handle_seek(runtime, target_ms, reply),
        PlaybackCommand::SetVolume { level, reply } => handle_set_volume(runtime, level, reply),
        PlaybackCommand::SetStemVolume { stem, level, reply } => {
            handle_set_stem_volume(runtime, stem, level, reply)
        }
        PlaybackCommand::AttachStems {
            request_id,
            song_id,
            stems,
            reply,
        } => handle_attach_stems(runtime, request_id, &song_id, stems, reply),
        PlaybackCommand::SetEqEnabled { enabled, reply } => {
            handle_set_eq_enabled(runtime, enabled, reply)
        }
        PlaybackCommand::SetEqGains { gains_db, reply } => {
            handle_set_eq_gains(runtime, gains_db, reply)
        }
    }
}

// ── AirPlay helpers ──────────────────────────────────────────────────────

fn bump_airplay_epoch_and_generation(airplay: &AirPlayState) {
    airplay.airplay_audio_tap.bump_epoch();
    airplay_stream::notify_audio_epoch(airplay.airplay_audio_tap.current_epoch());
    airplay
        .airplay_stream_generation
        .fetch_add(1, Ordering::SeqCst);
}

fn increment_airplay_refresh_token_if_audience_active(airplay: &AirPlayState) {
    if airplay.airplay_audience_active.load(Ordering::SeqCst) {
        airplay
            .airplay_control_refresh_token
            .fetch_add(1, Ordering::SeqCst);
    }
}

// ── Output helper ────────────────────────────────────────────────────────

fn ensure_output(runtime: &CoordinatorRuntime<impl Runtime>) -> Result<(), PlaybackError> {
    output::ensure_output_thread(
        &runtime.output_started,
        &runtime.output_start_lock,
        runtime.playback.clone(),
        runtime.airplay.airplay_audio_tap.clone(),
        runtime.airplay.airplay_local_output_suppressed.clone(),
        runtime.shutdown.clone(),
        runtime.peak_ring.clone(),
    )
}

// ── Emit helpers ─────────────────────────────────────────────────────────

fn emit_position<R: Runtime>(
    app_handle: &tauri::AppHandle<R>,
    snapshot: &PlaybackStateSnapshot,
) -> Result<(), PlaybackError> {
    if snapshot.song_id.is_none() {
        return Ok(());
    }
    app_handle
        .emit(PLAYBACK_POSITION_EVENT, playback_position_event(snapshot))
        .map_err(|e| PlaybackError::Internal(e.to_string()))
}

fn emit_playback_error<R: Runtime>(
    app_handle: &tauri::AppHandle<R>,
    song_id: &str,
    error: PlaybackError,
) {
    let _ = app_handle.emit(
        PLAYBACK_ERROR_EVENT,
        PlaybackErrorEvent {
            song_id: song_id.to_owned(),
            error: CommandError::from(error),
        },
    );
}

// ── Latest-request guard ─────────────────────────────────────────────────

fn is_latest_request(runtime: &CoordinatorRuntime<impl Runtime>, request_id: u64) -> bool {
    runtime.latest_request_id.load(Ordering::SeqCst) == request_id
}

// ── Failure reporting for InstallReady output failures ───────────────────

/// Report a failure for the latest request after `InstallReady` has already
/// installed the track. Unlike `handle_fail_load` (which clears a pending
/// load via `cancel_loading_if_matching`), this clears the *installed* track
/// via `clear_track_if_matching` — `start_track` / `start_track_streaming`
/// already cleared `loading_song_id`, so the loading-state guard would
/// silently no-op and leave the UI in a silent `playing` state with no output
/// device.
fn report_install_output_failure<R: Runtime>(
    runtime: &CoordinatorRuntime<R>,
    song_id: &str,
    request_id: u64,
    error: PlaybackError,
) {
    // Defensive latest-request guard at the function boundary. The only
    // caller (`handle_install_ready`) already checks `is_latest_request`
    // before reaching this point, and the coordinator is single-threaded so
    // the id cannot change in between. The guard is kept as a safety net so
    // that a stale request can never clear a newer track if this helper is
    // ever called from an additional path in the future.
    if !is_latest_request(runtime, request_id) {
        return;
    }

    let snapshot = {
        let Ok(mut playback) = runtime.playback.lock() else {
            eprintln!("coordinator: playback lock poisoned in install output failure");
            return;
        };
        if !playback.clear_track_if_matching(song_id) {
            return;
        }
        playback.snapshot()
    };

    let _ = emit_position(&runtime.app_handle, &snapshot);
    emit_playback_error(&runtime.app_handle, song_id, error);
}

// ── Command handlers ─────────────────────────────────────────────────────

fn handle_begin_load<R: Runtime>(
    runtime: &CoordinatorRuntime<R>,
    request_id: u64,
    song_id: &str,
    reply: SnapshotReply,
) {
    if !is_latest_request(runtime, request_id) {
        let _ = reply.send(Err(PlaybackError::Internal(
            "stale begin-load request".to_owned(),
        )));
        return;
    }

    let snapshot = {
        let Ok(mut playback) = runtime.playback.lock() else {
            let _ = reply.send(Err(PlaybackError::Internal(
                "playback controller lock was poisoned".to_owned(),
            )));
            return;
        };
        playback.start_track_loading(song_id)
    };

    bump_airplay_epoch_and_generation(&runtime.airplay);

    if let Err(e) = emit_position(&runtime.app_handle, &snapshot) {
        let _ = reply.send(Err(e));
        return;
    }

    let _ = reply.send(Ok(snapshot));
}

fn handle_install_ready<R: Runtime>(
    runtime: &CoordinatorRuntime<R>,
    request_id: u64,
    song_id: &str,
    ready: ReadyTrack,
) {
    // First guard: check before locking.
    if !is_latest_request(runtime, request_id) {
        return;
    }

    // Install the track and extract CDG in one match — `ready` is consumed
    // and `cdg` is moved out alongside the install.
    let (snapshot, cdg) = {
        let Ok(mut playback) = runtime.playback.lock() else {
            eprintln!("coordinator: playback lock poisoned in InstallReady");
            return;
        };
        // Second guard: re-check after acquiring the lock.
        if !is_latest_request(runtime, request_id) {
            return;
        }

        match ready {
            ReadyTrack::Decoded { audio, stems, cdg } => {
                let snapshot = playback.start_track(song_id.to_owned(), audio, monotonic_now_ms());
                if let Some(stems) = stems {
                    if let Err(e) = playback.attach_stems(song_id, stems) {
                        eprintln!("coordinator: failed to attach decoded stems: {e}");
                    }
                }
                (snapshot, cdg)
            }
            ReadyTrack::Streaming {
                sample_rate,
                channels,
                duration_ms,
                original,
                stems,
                cdg,
            } => {
                let snapshot = playback.start_track_streaming(
                    song_id.to_owned(),
                    sample_rate,
                    channels,
                    duration_ms,
                    original,
                    monotonic_now_ms(),
                );
                if let Some(stem_track) = stems {
                    if let Err(e) = playback.attach_streaming_stems(song_id, *stem_track) {
                        eprintln!("coordinator: failed to attach streaming stems: {e}");
                    }
                }
                (snapshot, cdg)
            }
        }
    };

    // Replace CDG state only if this song still owns the player.
    if snapshot.song_id.as_deref() == Some(song_id) {
        if let Ok(mut cdg_state) = runtime.cdg_state.lock() {
            *cdg_state = cdg;
        } else {
            eprintln!("coordinator: CDG state lock poisoned in InstallReady");
        }
    }

    // Ensure output thread is running. On failure, clear the just-installed
    // track and emit playback-error for the latest request. The track was
    // already installed by start_track above, so we use the install-failure
    // path (clear_track_if_matching) rather than the loading-failure path
    // (cancel_loading_if_matching).
    if let Err(e) = ensure_output(runtime) {
        report_install_output_failure(runtime, song_id, request_id, e);
        return;
    }

    let _ = emit_position(&runtime.app_handle, &snapshot);
}

fn handle_fail_load<R: Runtime>(
    runtime: &CoordinatorRuntime<R>,
    request_id: u64,
    song_id: &str,
    error: PlaybackError,
) {
    if !is_latest_request(runtime, request_id) {
        return;
    }

    let snapshot = {
        let Ok(mut playback) = runtime.playback.lock() else {
            eprintln!("coordinator: playback lock poisoned in FailLoad");
            return;
        };
        if !playback.cancel_loading_if_matching(song_id) {
            return;
        }
        playback.snapshot()
    };

    let _ = emit_position(&runtime.app_handle, &snapshot);
    emit_playback_error(&runtime.app_handle, song_id, error);
}

fn handle_resume<R: Runtime>(runtime: &CoordinatorRuntime<R>, reply: SnapshotReply) {
    let snapshot = {
        let Ok(mut playback) = runtime.playback.lock() else {
            let _ = reply.send(Err(PlaybackError::Internal(
                "playback controller lock was poisoned".to_owned(),
            )));
            return;
        };
        match playback.play(monotonic_now_ms()) {
            Ok(s) => s,
            Err(e) => {
                let _ = reply.send(Err(e));
                return;
            }
        }
    };

    bump_airplay_epoch_and_generation(&runtime.airplay);

    if let Err(e) = ensure_output(runtime) {
        let _ = reply.send(Err(e));
        return;
    }

    if let Err(e) = emit_position(&runtime.app_handle, &snapshot) {
        let _ = reply.send(Err(e));
        return;
    }

    let _ = reply.send(Ok(snapshot));
}

fn handle_pause<R: Runtime>(runtime: &CoordinatorRuntime<R>, reply: SnapshotReply) {
    let snapshot = {
        let Ok(mut playback) = runtime.playback.lock() else {
            let _ = reply.send(Err(PlaybackError::Internal(
                "playback controller lock was poisoned".to_owned(),
            )));
            return;
        };
        match playback.pause(monotonic_now_ms()) {
            Ok(s) => s,
            Err(e) => {
                let _ = reply.send(Err(e));
                return;
            }
        }
    };

    bump_airplay_epoch_and_generation(&runtime.airplay);

    if let Err(e) = emit_position(&runtime.app_handle, &snapshot) {
        let _ = reply.send(Err(e));
        return;
    }

    let _ = reply.send(Ok(snapshot));
}

fn handle_seek<R: Runtime>(runtime: &CoordinatorRuntime<R>, target_ms: u64, reply: SnapshotReply) {
    let (previous_position_ms, snapshot) = {
        let Ok(mut playback) = runtime.playback.lock() else {
            let _ = reply.send(Err(PlaybackError::Internal(
                "playback controller lock was poisoned".to_owned(),
            )));
            return;
        };
        let previous_position_ms = playback.snapshot().position_ms;
        match playback.seek(target_ms, monotonic_now_ms()) {
            Ok(s) => (previous_position_ms, s),
            Err(e) => {
                let _ = reply.send(Err(e));
                return;
            }
        }
    };

    bump_airplay_epoch_and_generation(&runtime.airplay);

    // CDG reset must happen after releasing the playback lock.
    {
        let Ok(mut cdg_state) = runtime.cdg_state.lock() else {
            eprintln!("coordinator: CDG state lock poisoned in Seek");
            let _ = reply.send(Ok(snapshot));
            return;
        };
        mark_cdg_reset_for_seek(&mut cdg_state, previous_position_ms, snapshot.position_ms);
    }

    if let Err(e) = emit_position(&runtime.app_handle, &snapshot) {
        let _ = reply.send(Err(e));
        return;
    }

    let _ = reply.send(Ok(snapshot));
}

fn handle_set_volume<R: Runtime>(
    runtime: &CoordinatorRuntime<R>,
    level: f32,
    reply: SnapshotReply,
) {
    let snapshot = {
        let Ok(mut playback) = runtime.playback.lock() else {
            let _ = reply.send(Err(PlaybackError::Internal(
                "playback controller lock was poisoned".to_owned(),
            )));
            return;
        };
        match playback.set_volume(level) {
            Ok(s) => s,
            Err(e) => {
                let _ = reply.send(Err(e));
                return;
            }
        }
    };

    increment_airplay_refresh_token_if_audience_active(&runtime.airplay);
    let _ = reply.send(Ok(snapshot));
}

fn handle_set_stem_volume<R: Runtime>(
    runtime: &CoordinatorRuntime<R>,
    stem: StemName,
    level: f32,
    reply: SnapshotReply,
) {
    let snapshot = {
        let Ok(mut playback) = runtime.playback.lock() else {
            let _ = reply.send(Err(PlaybackError::Internal(
                "playback controller lock was poisoned".to_owned(),
            )));
            return;
        };
        match playback.set_stem_volume(stem, level) {
            Ok(s) => s,
            Err(e) => {
                let _ = reply.send(Err(e));
                return;
            }
        }
    };

    increment_airplay_refresh_token_if_audience_active(&runtime.airplay);
    let _ = reply.send(Ok(snapshot));
}

fn handle_attach_stems<R: Runtime>(
    runtime: &CoordinatorRuntime<R>,
    request_id: u64,
    song_id: &str,
    stems: LoadedStems,
    reply: SnapshotReply,
) {
    if !is_latest_request(runtime, request_id) {
        // Stale request — return current snapshot without attaching.
        let snapshot = {
            let Ok(mut playback) = runtime.playback.lock() else {
                let _ = reply.send(Err(PlaybackError::Internal(
                    "playback controller lock was poisoned".to_owned(),
                )));
                return;
            };
            playback.snapshot()
        };
        let _ = reply.send(Ok(snapshot));
        return;
    }

    let snapshot = {
        let Ok(mut playback) = runtime.playback.lock() else {
            let _ = reply.send(Err(PlaybackError::Internal(
                "playback controller lock was poisoned".to_owned(),
            )));
            return;
        };
        // Check that the song still matches.
        if playback.current_song_id() != Some(song_id) {
            let _ = reply.send(Ok(playback.snapshot()));
            return;
        }
        if let Err(e) = playback.attach_stems(song_id, stems) {
            let _ = reply.send(Err(e));
            return;
        }
        playback.snapshot()
    };

    let _ = reply.send(Ok(snapshot));
}

fn handle_set_eq_enabled<R: Runtime>(
    runtime: &CoordinatorRuntime<R>,
    enabled: bool,
    reply: SnapshotReply,
) {
    let snapshot = {
        let Ok(mut playback) = runtime.playback.lock() else {
            let _ = reply.send(Err(PlaybackError::Internal(
                "playback controller lock was poisoned".to_owned(),
            )));
            return;
        };
        playback.set_eq_enabled(enabled)
    };
    let _ = reply.send(Ok(snapshot));
}

fn handle_set_eq_gains<R: Runtime>(
    runtime: &CoordinatorRuntime<R>,
    gains_db: [f32; 5],
    reply: SnapshotReply,
) {
    let snapshot = {
        let Ok(mut playback) = runtime.playback.lock() else {
            let _ = reply.send(Err(PlaybackError::Internal(
                "playback controller lock was poisoned".to_owned(),
            )));
            return;
        };
        playback.set_eq_gains(gains_db)
    };
    let _ = reply.send(Ok(snapshot));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        audio::decode::DecodedAudio,
        commands::cdg::CdgPlaybackState,
        state::{AirPlayState, PlaybackState},
    };
    use std::sync::atomic::Ordering;
    use tauri::test::{mock_app, MockRuntime};

    /// Test harness: creates a CoordinatorRuntime with a mock app handle,
    /// spawns the coordinator, and provides helpers to send commands.
    struct Harness {
        runtime: Arc<CoordinatorRuntime<MockRuntime>>,
        tx: mpsc::Sender<PlaybackCommand>,
        handle: Option<JoinHandle<()>>,
    }

    impl Harness {
        fn with_request_id(initial_request_id: u64) -> Self {
            let app = mock_app();
            let app_handle = app.handle().clone();
            let playback = Arc::new(Mutex::new(PlaybackController::default()));
            let cdg_state = Arc::new(Mutex::new(None));
            let latest_request_id = Arc::new(AtomicU64::new(initial_request_id));
            // Pretend the output thread is already started so `ensure_output`
            // is a no-op. Tests exercise coordinator command logic, not CPAL
            // device initialization, which is unavailable on headless CI.
            let output_started = Arc::new(AtomicBool::new(true));
            let output_start_lock = Arc::new(Mutex::new(()));
            let airplay = AirPlayState::test_fixture();
            let shutdown = Arc::new(AtomicBool::new(false));

            let (tx, rx) = mpsc::channel();
            let runtime = Arc::new(CoordinatorRuntime {
                app_handle,
                playback: Arc::clone(&playback),
                cdg_state: Arc::clone(&cdg_state),
                latest_request_id: Arc::clone(&latest_request_id),
                output_started: Arc::clone(&output_started),
                output_start_lock: Arc::clone(&output_start_lock),
                airplay: airplay.clone(),
                shutdown: Arc::clone(&shutdown),
                peak_ring: Arc::new(crate::audio::peaks::PeakRing::new()),
            });
            let handle = spawn_coordinator(
                CoordinatorRuntime {
                    app_handle: runtime.app_handle.clone(),
                    playback: Arc::clone(&runtime.playback),
                    cdg_state: Arc::clone(&runtime.cdg_state),
                    latest_request_id: Arc::clone(&runtime.latest_request_id),
                    output_started: Arc::clone(&runtime.output_started),
                    output_start_lock: Arc::clone(&runtime.output_start_lock),
                    airplay: runtime.airplay.clone(),
                    shutdown: Arc::clone(&runtime.shutdown),
                    peak_ring: runtime.peak_ring.clone(),
                },
                rx,
            );
            Self {
                runtime,
                tx,
                handle: Some(handle),
            }
        }

        fn send(&self, command: PlaybackCommand) {
            self.tx.send(command).expect("coordinator channel open");
        }

        fn send_and_recv(
            &self,
            make_command: impl FnOnce(SnapshotReply) -> PlaybackCommand,
        ) -> Result<PlaybackStateSnapshot, PlaybackError> {
            let (tx_reply, rx_reply) = tokio::sync::oneshot::channel();
            self.send(make_command(tx_reply));
            rx_reply.blocking_recv().expect("coordinator should reply")
        }

        fn snapshot(&self) -> PlaybackStateSnapshot {
            self.runtime
                .playback
                .lock()
                .expect("playback lock")
                .snapshot()
        }

        /// Force `ensure_output_thread` to fail deterministically by poisoning
        /// the `output_start_lock`. The coordinator's `ensure_output` helper
        /// will receive a `PoisonError` and return
        /// `PlaybackError::AudioOutputUnavailable`.
        ///
        /// Also flips `output_started` to `false` so `ensure_output` actually
        /// enters the lock-acquisition path (the harness defaults to `true`
        /// to skip CPAL initialization on headless CI).
        fn poison_output_lock(&self) {
            self.runtime.output_started.store(false, Ordering::SeqCst);
            // Lock and explicitly leak the guard so the mutex becomes poisoned.
            // We do this by panicking while holding the lock.
            let lock = Arc::clone(&self.runtime.output_start_lock);
            let _ = std::thread::spawn(move || {
                let _guard = lock.lock().unwrap();
                panic!("intentional panic to poison output_start_lock");
            })
            .join();
        }

        fn shutdown(self) {
            self.runtime.shutdown.store(true, Ordering::Relaxed);
            // Send a dummy command to wake the coordinator so it checks shutdown.
            let (tx_reply, _) = tokio::sync::oneshot::channel();
            let _ = self.tx.send(PlaybackCommand::Pause { reply: tx_reply });
            if let Some(handle) = self.handle {
                let _ = handle.join();
            }
        }
    }

    fn dummy_audio() -> DecodedAudio {
        DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 5_000,
            samples: vec![0.0; 44_100 * 2 * 5],
        }
    }

    fn dummy_stems() -> LoadedStems {
        LoadedStems::TwoStem {
            vocals: dummy_audio(),
            accompaniment: dummy_audio(),
        }
    }

    /// Create a minimal `StreamingTrack::Single` for tests.
    fn dummy_streaming_track() -> StreamingTrack {
        let (_producer, consumer) = crate::audio::streaming::create_stream_pair(44_100, 2);
        StreamingTrack::Single { consumer }
    }

    /// Helper: install a decoded track via InstallReady so the controller
    /// has a playing track for subsequent commands.
    fn install_track(harness: &Harness, request_id: u64, song_id: &str) {
        harness.send(PlaybackCommand::InstallReady {
            request_id,
            song_id: song_id.to_owned(),
            ready: Box::new(ReadyTrack::Decoded {
                audio: dummy_audio(),
                stems: None,
                cdg: None,
            }),
        });
        // Give the coordinator time to process the fire-and-forget command.
        // Use a barrier: send a Pause after and wait for its reply.
        let _ = harness.send_and_recv(|reply| PlaybackCommand::Pause { reply });
        // Resume to restore playing state.
        let _ = harness.send_and_recv(|reply| PlaybackCommand::Resume { reply });
    }

    // ── FIFO ordering ────────────────────────────────────────────────────

    #[test]
    fn fifo_ordering_across_pause_resume_seek_volume() {
        let harness = Harness::with_request_id(1);
        install_track(&harness, 1, "song-a");

        // Send pause, then seek, then set_volume in rapid succession.
        // The coordinator must process them in FIFO order.
        let paused = harness.send_and_recv(|reply| PlaybackCommand::Pause { reply });
        assert!(!paused.unwrap().is_playing);

        let sought = harness.send_and_recv(|reply| PlaybackCommand::Seek {
            target_ms: 2_000,
            reply,
        });
        assert_eq!(sought.unwrap().position_ms, 2_000);

        let _ = harness.send_and_recv(|reply| PlaybackCommand::SetVolume { level: 0.5, reply });

        let snapshot = harness.snapshot();
        assert_eq!(snapshot.volume, 0.5);
        assert_eq!(snapshot.position_ms, 2_000);

        harness.shutdown();
    }

    // ── Post-operation snapshots and transport-generation increments ──────

    #[test]
    fn pause_and_resume_increment_transport_generation() {
        let harness = Harness::with_request_id(1);
        install_track(&harness, 1, "song-a");

        let gen_before = harness.snapshot().transport_generation;
        let _ = harness.send_and_recv(|reply| PlaybackCommand::Pause { reply });
        let gen_after_pause = harness.snapshot().transport_generation;
        assert!(gen_after_pause > gen_before);

        let _ = harness.send_and_recv(|reply| PlaybackCommand::Resume { reply });
        let gen_after_resume = harness.snapshot().transport_generation;
        assert!(gen_after_resume > gen_after_pause);

        harness.shutdown();
    }

    #[test]
    fn seek_increment_transport_generation() {
        let harness = Harness::with_request_id(1);
        install_track(&harness, 1, "song-a");

        let gen_before = harness.snapshot().transport_generation;
        let _ = harness.send_and_recv(|reply| PlaybackCommand::Seek {
            target_ms: 1_000,
            reply,
        });
        let gen_after = harness.snapshot().transport_generation;
        assert!(gen_after > gen_before);

        harness.shutdown();
    }

    // ── Stale request guards ─────────────────────────────────────────────

    #[test]
    fn stale_install_ready_is_ignored() {
        let harness = Harness::with_request_id(2);
        // Install a track for request 2 (latest).
        install_track(&harness, 2, "song-b");
        assert_eq!(harness.snapshot().song_id.as_deref(), Some("song-b"));

        // Send a stale InstallReady for request 1 — should be ignored.
        harness.send(PlaybackCommand::InstallReady {
            request_id: 1,
            song_id: "song-a".to_owned(),
            ready: Box::new(ReadyTrack::Decoded {
                audio: dummy_audio(),
                stems: None,
                cdg: None,
            }),
        });
        // Use a sync command as a barrier.
        let _ = harness.send_and_recv(|reply| PlaybackCommand::SetVolume { level: 0.7, reply });

        // Song-b should still be loaded, not song-a.
        assert_eq!(harness.snapshot().song_id.as_deref(), Some("song-b"));

        harness.shutdown();
    }

    #[test]
    fn stale_fail_load_is_ignored() {
        let harness = Harness::with_request_id(2);
        install_track(&harness, 2, "song-b");

        // Send a stale FailLoad for request 1 — should be ignored.
        harness.send(PlaybackCommand::FailLoad {
            request_id: 1,
            song_id: "song-a".to_owned(),
            error: PlaybackError::AudioDecodeFailed("stale".to_owned()),
        });
        // Barrier.
        let _ = harness.send_and_recv(|reply| PlaybackCommand::SetVolume { level: 0.8, reply });

        // song-b should still be loaded.
        assert_eq!(harness.snapshot().song_id.as_deref(), Some("song-b"));

        harness.shutdown();
    }

    #[test]
    fn stale_attach_stems_returns_current_snapshot_without_attaching() {
        let harness = Harness::with_request_id(2);
        install_track(&harness, 2, "song-b");

        // Send AttachStems with a stale request_id.
        let snapshot = harness
            .send_and_recv(|reply| PlaybackCommand::AttachStems {
                request_id: 1,
                song_id: "song-b".to_owned(),
                stems: dummy_stems(),
                reply,
            })
            .expect("stale attach should return snapshot");

        assert!(!snapshot.has_stems, "stale stems must not be attached");

        harness.shutdown();
    }

    #[test]
    fn attach_stems_for_wrong_song_returns_snapshot_without_attaching() {
        let harness = Harness::with_request_id(2);
        install_track(&harness, 2, "song-b");

        let snapshot = harness
            .send_and_recv(|reply| PlaybackCommand::AttachStems {
                request_id: 2,
                song_id: "song-a".to_owned(),
                stems: dummy_stems(),
                reply,
            })
            .expect("wrong-song attach should return snapshot");

        assert!(!snapshot.has_stems, "stems for wrong song must not attach");

        harness.shutdown();
    }

    // ── AirPlay epoch/generation/refresh-token matrix ────────────────────

    #[test]
    fn pause_bumps_airplay_epoch_and_generation() {
        let harness = Harness::with_request_id(1);
        install_track(&harness, 1, "song-a");

        let initial_gen = harness
            .runtime
            .airplay
            .airplay_stream_generation
            .load(Ordering::SeqCst);
        let initial_epoch = harness.runtime.airplay.airplay_audio_tap.current_epoch();

        let _ = harness.send_and_recv(|reply| PlaybackCommand::Pause { reply });

        assert_eq!(
            harness
                .runtime
                .airplay
                .airplay_stream_generation
                .load(Ordering::SeqCst),
            initial_gen + 1
        );
        assert_eq!(
            harness.runtime.airplay.airplay_audio_tap.current_epoch(),
            initial_epoch + 1
        );

        harness.shutdown();
    }

    #[test]
    fn resume_bumps_airplay_epoch_and_generation() {
        let harness = Harness::with_request_id(1);
        install_track(&harness, 1, "song-a");

        let _ = harness.send_and_recv(|reply| PlaybackCommand::Pause { reply });
        let gen_after_pause = harness
            .runtime
            .airplay
            .airplay_stream_generation
            .load(Ordering::SeqCst);
        let epoch_after_pause = harness.runtime.airplay.airplay_audio_tap.current_epoch();

        let _ = harness.send_and_recv(|reply| PlaybackCommand::Resume { reply });

        assert_eq!(
            harness
                .runtime
                .airplay
                .airplay_stream_generation
                .load(Ordering::SeqCst),
            gen_after_pause + 1
        );
        assert_eq!(
            harness.runtime.airplay.airplay_audio_tap.current_epoch(),
            epoch_after_pause + 1
        );

        harness.shutdown();
    }

    #[test]
    fn seek_bumps_airplay_epoch_and_generation() {
        let harness = Harness::with_request_id(1);
        install_track(&harness, 1, "song-a");

        let initial_gen = harness
            .runtime
            .airplay
            .airplay_stream_generation
            .load(Ordering::SeqCst);

        let _ = harness.send_and_recv(|reply| PlaybackCommand::Seek {
            target_ms: 1_000,
            reply,
        });

        assert_eq!(
            harness
                .runtime
                .airplay
                .airplay_stream_generation
                .load(Ordering::SeqCst),
            initial_gen + 1
        );

        harness.shutdown();
    }

    #[test]
    fn set_volume_does_not_bump_epoch_but_increments_refresh_token_when_audience_active() {
        let harness = Harness::with_request_id(1);
        install_track(&harness, 1, "song-a");
        harness
            .runtime
            .airplay
            .airplay_audience_active
            .store(true, Ordering::SeqCst);

        let initial_gen = harness
            .runtime
            .airplay
            .airplay_stream_generation
            .load(Ordering::SeqCst);
        let initial_epoch = harness.runtime.airplay.airplay_audio_tap.current_epoch();
        let initial_token = harness
            .runtime
            .airplay
            .airplay_control_refresh_token
            .load(Ordering::SeqCst);

        let _ = harness.send_and_recv(|reply| PlaybackCommand::SetVolume { level: 0.5, reply });

        assert_eq!(
            harness
                .runtime
                .airplay
                .airplay_stream_generation
                .load(Ordering::SeqCst),
            initial_gen,
            "volume must not bump stream generation"
        );
        assert_eq!(
            harness.runtime.airplay.airplay_audio_tap.current_epoch(),
            initial_epoch,
            "volume must not bump epoch"
        );
        assert_eq!(
            harness
                .runtime
                .airplay
                .airplay_control_refresh_token
                .load(Ordering::SeqCst),
            initial_token + 1,
            "volume must increment refresh token when audience active"
        );

        harness.shutdown();
    }

    #[test]
    fn set_volume_does_not_increment_refresh_token_when_audience_inactive() {
        let harness = Harness::with_request_id(1);
        install_track(&harness, 1, "song-a");
        harness
            .runtime
            .airplay
            .airplay_audience_active
            .store(false, Ordering::SeqCst);

        let initial_token = harness
            .runtime
            .airplay
            .airplay_control_refresh_token
            .load(Ordering::SeqCst);

        let _ = harness.send_and_recv(|reply| PlaybackCommand::SetVolume { level: 0.5, reply });

        assert_eq!(
            harness
                .runtime
                .airplay
                .airplay_control_refresh_token
                .load(Ordering::SeqCst),
            initial_token,
            "refresh token must not change when audience inactive"
        );

        harness.shutdown();
    }

    // ── CDG seek behavior ────────────────────────────────────────────────

    #[test]
    fn backward_seek_marks_cdg_for_reset() {
        let harness = Harness::with_request_id(1);
        // Install a track with CDG state.
        let cdg = Some(CdgPlaybackState::new(Vec::new()));
        harness.send(PlaybackCommand::InstallReady {
            request_id: 1,
            song_id: "song-a".to_owned(),
            ready: Box::new(ReadyTrack::Decoded {
                audio: dummy_audio(),
                stems: None,
                cdg,
            }),
        });
        // Barrier.
        let _ = harness.send_and_recv(|reply| PlaybackCommand::Pause { reply });
        let _ = harness.send_and_recv(|reply| PlaybackCommand::Resume { reply });

        // Set CDG state to non-reset.
        {
            let mut cdg_state = harness.runtime.cdg_state.lock().unwrap();
            if let Some(ref mut cdg) = *cdg_state {
                cdg.needs_reset = false;
                cdg.cached_frame = Some(vec![0xAA]);
            }
        }

        // Forward seek — should NOT reset CDG.
        let _ = harness.send_and_recv(|reply| PlaybackCommand::Seek {
            target_ms: 2_000,
            reply,
        });
        {
            let cdg_state = harness.runtime.cdg_state.lock().unwrap();
            let cdg = cdg_state.as_ref().expect("cdg state should exist");
            assert!(!cdg.needs_reset, "forward seek must not reset CDG");
        }

        // Backward seek — should reset CDG.
        let _ = harness.send_and_recv(|reply| PlaybackCommand::Seek {
            target_ms: 500,
            reply,
        });
        {
            let cdg_state = harness.runtime.cdg_state.lock().unwrap();
            let cdg = cdg_state.as_ref().expect("cdg state should exist");
            assert!(cdg.needs_reset, "backward seek must reset CDG");
            assert!(
                cdg.cached_frame.is_none(),
                "backward seek must clear cached frame"
            );
        }

        harness.shutdown();
    }

    // ── No output device: non-output commands complete ───────────────────

    #[test]
    fn non_output_commands_complete_without_output_device() {
        let harness = Harness::with_request_id(1);
        // The harness pretends the output thread is already started so
        // InstallReady succeeds without CPAL. Non-output commands (Pause,
        // SetVolume, Seek) never call ensure_output and thus work regardless.
        install_track(&harness, 1, "song-a");

        // Pause and SetVolume should work without an output device.
        let paused = harness.send_and_recv(|reply| PlaybackCommand::Pause { reply });
        assert!(!paused.unwrap().is_playing);

        let vol = harness.send_and_recv(|reply| PlaybackCommand::SetVolume { level: 0.3, reply });
        assert_eq!(vol.unwrap().volume, 0.3);

        // Seek should work without an output device.
        let sought = harness.send_and_recv(|reply| PlaybackCommand::Seek {
            target_ms: 1_000,
            reply,
        });
        assert_eq!(sought.unwrap().position_ms, 1_000);

        harness.shutdown();
    }

    // ── Coordinator disconnect ───────────────────────────────────────────

    #[test]
    fn disconnected_coordinator_returns_error_on_send() {
        // Create a PlaybackState with a disconnected sender.
        let (tx, _) = mpsc::channel();
        let playback_state = PlaybackState {
            playback: Arc::new(Mutex::new(PlaybackController::default())),
            cdg_state: Arc::new(Mutex::new(None)),
            playback_request_id: Arc::new(AtomicU64::new(1)),
            audio_output_started: Arc::new(AtomicBool::new(false)),
            audio_output_start_lock: Arc::new(Mutex::new(())),
            background_shutdown: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
            command_tx: tx,
            peak_ring: Arc::new(crate::audio::peaks::PeakRing::new()),
        };

        let (reply_tx, _reply_rx) = tokio::sync::oneshot::channel();
        let result = playback_state
            .command_tx
            .send(PlaybackCommand::Resume { reply: reply_tx });
        assert!(
            result.is_err(),
            "send to disconnected coordinator should fail"
        );
    }

    // ── Volume clamping ──────────────────────────────────────────────────

    #[test]
    fn set_volume_clamps_to_zero_one() {
        let harness = Harness::with_request_id(1);
        install_track(&harness, 1, "song-a");

        let snap = harness.send_and_recv(|reply| PlaybackCommand::SetVolume { level: 1.5, reply });
        assert_eq!(snap.unwrap().volume, 1.0);

        let snap = harness.send_and_recv(|reply| PlaybackCommand::SetVolume { level: -0.5, reply });
        assert_eq!(snap.unwrap().volume, 0.0);

        harness.shutdown();
    }

    // ── BeginLoad returns loading snapshot ───────────────────────────────

    #[test]
    fn begin_load_returns_loading_snapshot() {
        let harness = Harness::with_request_id(1);

        let snapshot = harness
            .send_and_recv(|reply| PlaybackCommand::BeginLoad {
                request_id: 1,
                song_id: "song-a".to_owned(),
                reply,
            })
            .expect("BeginLoad should succeed");

        assert_eq!(snapshot.song_id.as_deref(), Some("song-a"));
        assert_eq!(snapshot.state, "loading");
        assert!(!snapshot.is_playing);

        // AirPlay epoch and generation should have been bumped.
        assert_eq!(
            harness
                .runtime
                .airplay
                .airplay_stream_generation
                .load(Ordering::SeqCst),
            2 // initial 1 + 1 bump
        );

        harness.shutdown();
    }

    #[test]
    fn stale_begin_load_returns_error() {
        let harness = Harness::with_request_id(2);

        let result = harness.send_and_recv(|reply| PlaybackCommand::BeginLoad {
            request_id: 1, // stale
            song_id: "song-a".to_owned(),
            reply,
        });

        assert!(result.is_err(), "stale BeginLoad should return error");

        harness.shutdown();
    }

    // ── Output failure on InstallReady ───────────────────────────────────

    #[test]
    fn install_ready_output_failure_clears_track_and_emits_error() {
        let harness = Harness::with_request_id(1);
        // Poison the output start lock so ensure_output fails deterministically.
        harness.poison_output_lock();

        // Send InstallReady — the coordinator will install the track, then
        // fail to start the output thread, then clear the track and emit
        // playback-error for the latest request.
        harness.send(PlaybackCommand::InstallReady {
            request_id: 1,
            song_id: "song-a".to_owned(),
            ready: Box::new(ReadyTrack::Decoded {
                audio: dummy_audio(),
                stems: None,
                cdg: None,
            }),
        });
        // Barrier: send a sync command and wait for its reply to ensure the
        // InstallReady has been fully processed.
        let _ = harness.send_and_recv(|reply| PlaybackCommand::SetVolume { level: 0.5, reply });

        // The track must have been cleared — the controller should be idle.
        let snapshot = harness.snapshot();
        assert!(
            snapshot.song_id.is_none(),
            "output failure must clear the installed track, got song_id={:?}",
            snapshot.song_id
        );

        harness.shutdown();
    }

    #[test]
    fn install_ready_output_failure_does_not_clear_newer_track() {
        let harness = Harness::with_request_id(2);
        // Install a track for request 2 (latest) with working output.
        install_track(&harness, 2, "song-b");
        assert_eq!(harness.snapshot().song_id.as_deref(), Some("song-b"));

        // Directly invoke the install-output-failure helper with a stale
        // request_id (1) and a different song_id. The `is_latest_request`
        // guard inside `report_install_output_failure` must reject it so
        // the newer track (song-b) is not cleared.
        //
        // At the command level, a stale `InstallReady` is already blocked
        // by `handle_install_ready`'s first guard and never reaches the
        // failure path. This unit test exercises the defensive guard at
        // the helper boundary directly.
        report_install_output_failure(
            &harness.runtime,
            "song-a",
            1, // stale
            PlaybackError::AudioOutputUnavailable("test".to_owned()),
        );

        // song-b must still be loaded — the stale failure was a no-op.
        assert_eq!(
            harness.snapshot().song_id.as_deref(),
            Some("song-b"),
            "stale output failure must not clear the newer track"
        );

        harness.shutdown();
    }

    // ── Stale streaming InstallReady ─────────────────────────────────────

    #[test]
    fn stale_streaming_install_ready_is_ignored() {
        let harness = Harness::with_request_id(2);
        // Install a decoded track for request 2 (latest).
        install_track(&harness, 2, "song-b");
        assert_eq!(harness.snapshot().song_id.as_deref(), Some("song-b"));

        // Send a stale streaming InstallReady for request 1.
        harness.send(PlaybackCommand::InstallReady {
            request_id: 1,
            song_id: "song-a".to_owned(),
            ready: Box::new(ReadyTrack::Streaming {
                sample_rate: 44_100,
                channels: 2,
                duration_ms: 5_000,
                original: dummy_streaming_track(),
                stems: None,
                cdg: None,
            }),
        });
        // Barrier.
        let _ = harness.send_and_recv(|reply| PlaybackCommand::SetVolume { level: 0.6, reply });

        // song-b should still be loaded, not song-a.
        assert_eq!(harness.snapshot().song_id.as_deref(), Some("song-b"));

        harness.shutdown();
    }

    // ── BeginLoad → InstallReady full flow ───────────────────────────────

    #[test]
    fn begin_load_then_install_ready_transitions_loading_to_playing() {
        let harness = Harness::with_request_id(1);

        // Step 1: BeginLoad returns a loading snapshot.
        let loading = harness
            .send_and_recv(|reply| PlaybackCommand::BeginLoad {
                request_id: 1,
                song_id: "song-a".to_owned(),
                reply,
            })
            .expect("BeginLoad should succeed");
        assert_eq!(loading.song_id.as_deref(), Some("song-a"));
        assert_eq!(loading.state, "loading");
        assert!(!loading.is_playing);

        // Step 2: InstallReady transitions to playing.
        // The request id is still 1 (the latest), so the guard passes.
        harness.send(PlaybackCommand::InstallReady {
            request_id: 1,
            song_id: "song-a".to_owned(),
            ready: Box::new(ReadyTrack::Decoded {
                audio: dummy_audio(),
                stems: None,
                cdg: None,
            }),
        });
        // Barrier.
        let _ = harness.send_and_recv(|reply| PlaybackCommand::Pause { reply });

        let snapshot = harness.snapshot();
        assert_eq!(snapshot.song_id.as_deref(), Some("song-a"));
        assert_ne!(
            snapshot.state, "loading",
            "InstallReady must transition out of loading state"
        );

        harness.shutdown();
    }

    // ── Lock poisoning ───────────────────────────────────────────────────

    #[test]
    fn poisoned_playback_lock_returns_error_and_worker_stays_alive() {
        let harness = Harness::with_request_id(1);

        // Poison the playback mutex by panicking while holding it.
        let playback = Arc::clone(&harness.runtime.playback);
        let _ = std::thread::spawn(move || {
            let _guard = playback.lock().unwrap();
            panic!("intentional panic to poison playback lock");
        })
        .join();

        // A Pause command should return an internal error, not hang.
        let result = harness.send_and_recv(|reply| PlaybackCommand::Pause { reply });
        assert!(
            result.is_err(),
            "poisoned lock must return an error, not hang"
        );

        // The coordinator must still be alive — send a SetVolume command.
        // It uses the same (still-poisoned) playback mutex, so it will also
        // return an error. The key assertion is that the coordinator
        // *responds* at all (doesn't hang or terminate the worker thread).
        let result2 =
            harness.send_and_recv(|reply| PlaybackCommand::SetVolume { level: 0.5, reply });
        assert!(
            result2.is_err(),
            "coordinator must still respond after poisoned lock"
        );

        harness.shutdown();
    }
}
