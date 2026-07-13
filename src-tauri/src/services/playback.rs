use crate::{
    audio::{
        coordinator::{PlaybackCommand, ReadyTrack},
        error::PlaybackError,
        playback::{PlaybackController, PlaybackStateSnapshot, PLAYBACK_ERROR_EVENT},
        remote_source,
    },
    cache,
    commands::error::{CommandError, ErrorCode, FallbackAction},
    library,
    library_root::LibraryRoot,
    services::{
        cdg::{load_cdg_packets_for_song, CdgLoadResult},
        playback_source::{
            self, ensure_remote_stem_files_cached, load_cached_stems_for_song,
            load_playback_source, PlaybackSourceLoad,
        },
    },
    state::AppState,
};
use rusqlite::Connection;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::{
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Runtime};

#[derive(Clone, serde::Serialize)]
pub struct PlaybackErrorEvent {
    pub song_id: String,
    pub error: CommandError,
}

/// Load CDG packets for a song and return them as a shared `Arc<[CdgPacket]>`.
/// Returns `None` when no CDG file exists or loading fails (CDG is optional).
/// Load failures are logged by the CDG service; audio playback continues.
fn load_cdg_packets_as_arc(
    library_root: &LibraryRoot,
    song: &library::Song,
) -> Option<Arc<[crate::cdg::CdgPacket]>> {
    match load_cdg_packets_for_song(library_root, song) {
        CdgLoadResult::Loaded(result) => {
            if let Some(diag) = &result.diagnostic {
                eprintln!(
                    "warning: CDG parse diagnostic for {}: {:?}",
                    song.hash, diag
                );
            }
            Some(Arc::from(result.packets.into_boxed_slice()))
        }
        CdgLoadResult::Missing => None,
        CdgLoadResult::ReadFailed | CdgLoadResult::ZipFailed => None,
    }
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

/// Play a song from the library. Resolves the song, increments the request id,
/// sends a `BeginLoad` command to the coordinator (which handles
/// `start_track_loading`, AirPlay epoch bump, and position emission), awaits
/// the loading snapshot, then spawns a background thread to decode/fetch and
/// install the ready track.
pub fn play<R: Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_id: &str,
) -> Result<PlaybackStateSnapshot, PlaybackError> {
    let library_root = state
        .shell
        .library_root()
        .map_err(|error| PlaybackError::Internal(error.message))?;
    let connection = cache::open_database(&library_root.database_path())
        .map_err(|e| PlaybackError::Internal(e.to_string()))?;
    let request_id = state
        .playback
        .playback_request_id
        .fetch_add(1, Ordering::SeqCst)
        + 1;
    let song = cache::get_song_by_hash(&connection, song_id)
        .map_err(|e| PlaybackError::Internal(e.to_string()))?
        .ok_or_else(|| PlaybackError::SongNotFound(song_id.to_owned()))?;

    // R9: Signal any previously running background thread to stop. Replace the
    // old Arc with a fresh one so the new thread gets its own un-signalled flag.
    let shutdown = {
        let mut guard = state.playback.background_shutdown.lock().map_err(|_| {
            PlaybackError::Internal("background_shutdown lock was poisoned".to_owned())
        })?;
        guard.store(true, Ordering::Relaxed);
        let new_shutdown = Arc::new(AtomicBool::new(false));
        *guard = new_shutdown.clone();
        new_shutdown
    };

    // Send BeginLoad to the coordinator and await the loading snapshot.
    // The coordinator handles start_track_loading, AirPlay epoch bump, and
    // the initial position event emission.
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    state
        .playback
        .command_tx
        .send(PlaybackCommand::BeginLoad {
            request_id,
            song_id: song.hash.clone(),
            reply: reply_tx,
        })
        .map_err(|_| PlaybackError::Internal("playback coordinator disconnected".to_owned()))?;
    let snapshot = reply_rx
        .blocking_recv()
        .map_err(|_| PlaybackError::Internal("playback coordinator dropped reply".to_owned()))??;

    let background_state = state.clone();
    let background_handle = app_handle.clone();
    let app_data_dir = state.shell.app_data_dir.clone();
    let library_root = library_root.clone();
    let song = song.clone();
    std::thread::spawn(move || {
        if shutdown.load(Ordering::Relaxed) {
            return;
        }
        if let Err(error) = play_track_background(
            &background_state,
            &background_handle,
            &app_data_dir,
            &library_root,
            &song,
            request_id,
            &shutdown,
        ) {
            // Send FailLoad to the coordinator — it handles cancel_loading,
            // position emission, and error event emission.
            let _ = background_state
                .playback
                .command_tx
                .send(PlaybackCommand::FailLoad {
                    request_id,
                    song_id: song.hash.clone(),
                    error,
                });
        }
    });

    Ok(snapshot)
}

/// Background task: load, decode, and install a ready track for any song
/// (local or remote). Runs on a spawned thread so the UI stays responsive
/// during decode/download. Sends `InstallReady` to the coordinator instead
/// of directly mutating the controller.
fn play_track_background<R: Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    app_data_dir: &Path,
    library_root: &LibraryRoot,
    song: &library::Song,
    request_id: u64,
    shutdown: &AtomicBool,
) -> Result<(), PlaybackError> {
    // R9: Early exit if a newer play() has signalled shutdown.
    if shutdown.load(Ordering::Relaxed) {
        return Ok(());
    }
    // Try streaming path first for local and remote files (low latency, bounded memory).
    if let Some(streaming_source) = playback_source::load_playback_source_streaming(
        Some(app_data_dir),
        &state.remote.remote_chunk_cache,
        library_root,
        song,
    )? {
        if shutdown.load(Ordering::Relaxed) {
            return Ok(());
        }

        // Load streaming stems (non-fatal: if stems can't be loaded, playback
        // continues with the base audio).
        let connection = cache::open_database(&library_root.database_path())
            .map_err(|e| PlaybackError::Internal(e.to_string()))?;
        if song.is_remote_stems() {
            let _ = ensure_remote_stem_files_cached(Some(app_data_dir), &connection, song);
        }
        let stems_track = match playback_source::load_cached_stems_for_song_streaming(
            Some(app_data_dir),
            &connection,
            library_root,
            song,
        ) {
            Ok(Some(stems_source)) => {
                // Log stem decode errors in the background.
                for handle in stems_source.decode_handles {
                    let song_id = song.hash.clone();
                    std::thread::spawn(move || {
                        if let Err(e) = handle.join() {
                            eprintln!("stem decode thread panicked for {song_id}: {e:?}");
                        }
                    });
                }
                Some(Box::new(stems_source.streaming_track))
            }
            Ok(None) => None, // No cached stems — play base audio only.
            Err(e) => {
                eprintln!("streaming stem load failed for {}: {e}", song.hash);
                None
            }
        };

        // Load CDG packets for this song (optional graphics sidecar).
        let cdg = load_cdg_packets_as_arc(library_root, song);

        let ready = ReadyTrack::Streaming {
            sample_rate: streaming_source.metadata.sample_rate,
            channels: streaming_source.metadata.channels,
            // Item 12: duration_ms may be None if the container doesn't expose
            // frame count metadata. Use 0 as fallback; it will be resolved async.
            duration_ms: streaming_source.metadata.duration_ms.unwrap_or(0),
            original: streaming_source.streaming_track,
            stems: stems_track,
            cdg,
        };

        // Send InstallReady — the coordinator handles track installation,
        // stem attachment, CDG state replacement, output startup, and position
        // emission atomically.
        state
            .playback
            .command_tx
            .send(PlaybackCommand::InstallReady {
                request_id,
                song_id: song.hash.clone(),
                ready: Box::new(ready),
            })
            .map_err(|_| PlaybackError::Internal("playback coordinator disconnected".to_owned()))?;

        // Consume fetch events in the background (for remote streaming).
        if let Some(fetch_event_rx) = streaming_source.fetch_event_rx {
            let event_state = state.clone();
            let event_app_handle = app_handle.clone();
            let event_song_id = song.hash.clone();
            let event_request_id = request_id;
            let event_library_root = library_root.clone();
            let event_app_data_dir = app_data_dir.to_path_buf();
            std::thread::spawn(move || {
                for event in fetch_event_rx {
                    match event {
                        remote_source::FetchEvent::ConsecutiveFailures { count } => {
                            eprintln!(
                                "remote fetch: {count} consecutive failures for {event_song_id}"
                            );
                            let _ = event_app_handle.emit(
                                PLAYBACK_ERROR_EVENT,
                                PlaybackErrorEvent {
                                    song_id: event_song_id.clone(),
                                    error: CommandError::new(
                                        ErrorCode::NetworkUnavailable,
                                        format!("remote fetch failed {count} times consecutively"),
                                        true,
                                        FallbackAction::Retry,
                                    ),
                                },
                            );
                        }
                        remote_source::FetchEvent::RangeNotSupported
                        | remote_source::FetchEvent::UrlExpired => {
                            let reason = match event {
                                remote_source::FetchEvent::RangeNotSupported => {
                                    "Range requests not supported"
                                }
                                remote_source::FetchEvent::UrlExpired => "download URL expired",
                                _ => unreachable!(),
                            };
                            eprintln!("remote fetch: {reason} for {event_song_id}, falling back to full-file playback");
                            if let Err(error) = fallback_remote_playback_to_full_file(
                                &event_state,
                                event_request_id,
                                &event_library_root,
                                &event_app_data_dir,
                                &event_song_id,
                            ) {
                                eprintln!(
                                    "remote fetch fallback failed for {event_song_id}: {error:#}"
                                );
                                let _ = event_state.playback.command_tx.send(
                                    PlaybackCommand::FailLoad {
                                        request_id: event_request_id,
                                        song_id: event_song_id.clone(),
                                        error,
                                    },
                                );
                            }
                        }
                    }
                }
            });
        }

        // Log decode errors in the background (non-blocking).
        let song_id = song.hash.clone();
        std::thread::spawn(move || {
            if let Err(e) = streaming_source.decode_handle.join() {
                eprintln!("decode thread panicked for {song_id}: {e:?}");
            }
        });

        return Ok(());
    }

    // Fallback: full decode path (remote, Media+G, or streaming not available).
    let connection = cache::open_database(&library_root.database_path())
        .map_err(|e| PlaybackError::Internal(e.to_string()))?;
    let PlaybackSourceLoad {
        decoded_audio,
        stems,
    } = load_playback_source(Some(app_data_dir), &connection, library_root, song)?;

    // R9: After the expensive decode, check if a newer play() has started.
    if shutdown.load(Ordering::Relaxed) {
        return Ok(());
    }

    // Load CDG packets for this song (optional graphics sidecar).
    let cdg = load_cdg_packets_as_arc(library_root, song);

    let ready = ReadyTrack::Decoded {
        audio: decoded_audio,
        stems,
        cdg,
    };

    state
        .playback
        .command_tx
        .send(PlaybackCommand::InstallReady {
            request_id,
            song_id: song.hash.clone(),
            ready: Box::new(ready),
        })
        .map_err(|_| PlaybackError::Internal("playback coordinator disconnected".to_owned()))?;

    Ok(())
}

/// Fallback when the remote byte-range stream becomes unusable.
///
/// We fully decode from the provider-backed cached full-file path (or an
/// equivalent non-range route) and send `InstallReady` to the coordinator.
fn fallback_remote_playback_to_full_file(
    state: &AppState,
    request_id: u64,
    library_root: &LibraryRoot,
    app_data_dir: &Path,
    song_id: &str,
) -> Result<(), PlaybackError> {
    let connection = cache::open_database(&library_root.database_path())
        .map_err(|e| PlaybackError::Internal(e.to_string()))?;

    let song = cache::get_song_by_hash(&connection, song_id)
        .map_err(|e| PlaybackError::Internal(e.to_string()))?
        .ok_or_else(|| PlaybackError::SongNotFound(song_id.to_owned()))?;

    let PlaybackSourceLoad {
        decoded_audio,
        stems,
    } = load_playback_source(Some(app_data_dir), &connection, library_root, &song)?;

    let cdg = load_cdg_packets_as_arc(library_root, &song);

    let ready = ReadyTrack::Decoded {
        audio: decoded_audio,
        stems,
        cdg,
    };

    state
        .playback
        .command_tx
        .send(PlaybackCommand::InstallReady {
            request_id,
            song_id: song.hash.clone(),
            ready: Box::new(ready),
        })
        .map_err(|_| PlaybackError::Internal("playback coordinator disconnected".to_owned()))?;

    Ok(())
}

/// Load and attach stems for the current song. Captures the current song_id
/// and latest request id before decode, then sends `AttachStems` to the
/// coordinator. A stale or switched song returns the current snapshot without
/// attaching data.
pub fn load_stems(state: &AppState) -> Result<PlaybackStateSnapshot, PlaybackError> {
    let library_root = state
        .shell
        .library_root()
        .map_err(|error| PlaybackError::Internal(error.message))?;
    let connection = cache::open_database(&library_root.database_path())
        .map_err(|e| PlaybackError::Internal(e.to_string()))?;
    let (song_id, request_id) = {
        let mut playback = state.playback.playback.lock().map_err(|_| {
            PlaybackError::Internal("playback controller lock was poisoned".to_owned())
        })?;

        let song_id = playback
            .current_song_id()
            .ok_or_else(|| PlaybackError::InvalidPlaybackState("no track is loaded".to_owned()))?
            .to_owned();

        if playback.has_stems() {
            return Ok(playback.snapshot());
        }

        let request_id = state.playback.playback_request_id.load(Ordering::SeqCst);
        (song_id, request_id)
    };

    let song = cache::get_song_by_hash(&connection, &song_id)
        .map_err(|e| PlaybackError::Internal(e.to_string()))?
        .ok_or_else(|| PlaybackError::SongNotFound(song_id.clone()))?;

    // Decode stems before sending the command — expensive work stays
    // outside the coordinator.
    let loaded_stems = load_cached_stems_for_song(
        Some(&state.shell.app_data_dir),
        &connection,
        &library_root,
        &song,
    )?;

    // Send AttachStems and await the reply.
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    state
        .playback
        .command_tx
        .send(PlaybackCommand::AttachStems {
            request_id,
            song_id: song_id.clone(),
            stems: loaded_stems,
            reply: reply_tx,
        })
        .map_err(|_| PlaybackError::Internal("playback coordinator disconnected".to_owned()))?;
    reply_rx
        .blocking_recv()
        .map_err(|_| PlaybackError::Internal("playback coordinator dropped reply".to_owned()))?
}

pub fn get_state(state: &AppState) -> Result<PlaybackStateSnapshot, PlaybackError> {
    let mut playback =
        state.playback.playback.lock().map_err(|_| {
            PlaybackError::Internal("playback controller lock was poisoned".to_owned())
        })?;
    Ok(playback.snapshot())
}

pub fn play_song_from_library(
    connection: &Connection,
    library_root: &LibraryRoot,
    controller: &mut PlaybackController,
    song_id: &str,
    now_ms: u64,
) -> Result<PlaybackStateSnapshot, PlaybackError> {
    let song = cache::get_song_by_hash(connection, song_id)
        .map_err(|e| PlaybackError::Internal(e.to_string()))?
        .ok_or_else(|| PlaybackError::SongNotFound(song_id.to_owned()))?;
    let PlaybackSourceLoad {
        decoded_audio,
        stems,
    } = load_playback_source(None, connection, library_root, &song)?;
    let snapshot = controller.start_track(song.hash.clone(), decoded_audio, now_ms);
    if let Some(stems) = stems {
        controller
            .attach_stems(&song.hash, stems)
            .map_err(|e| PlaybackError::Internal(e.to_string()))?;
        return Ok(controller.snapshot());
    }
    Ok(snapshot)
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
                output_format: crate::audio::output_format::create_output_format_state(),
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
                        error: None,
                    },
                )),
            ),
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
