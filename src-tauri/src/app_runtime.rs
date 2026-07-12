use crate::audio::coordinator::{spawn_coordinator, CoordinatorRuntime};
use crate::library_root::LibraryRoot;
use crate::state::{AirPlayState, AppShell, PlaybackState, RemoteState, SeparationState};
use crate::{
    airplay_stream, audio,
    audio::playback::{PlaybackController, PLAYBACK_POSITION_POLL_INTERVAL_MS},
    cache, commands, config, derive_startup_model_bootstrap, separator, AppState,
};
use anyhow::Context;
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use tauri::{Emitter, Manager, Runtime};

pub fn setup_app<R: Runtime>(app: &mut tauri::App<R>) -> Result<(), Box<dyn std::error::Error>> {
    let app_resource_dir = app
        .path()
        .resource_dir()
        .context("failed to resolve bundled resource directory")?;

    let app_data_dir = app
        .path()
        .app_data_dir()
        .context("failed to resolve application data directory")?;
    fs::create_dir_all(&app_data_dir).with_context(|| {
        format!(
            "failed to create application data directory at {}",
            app_data_dir.display()
        )
    })?;

    // Resolve the runtime bootstrap status. The runtime may come from:
    // 1. Bundled resources (legacy, being phased out)
    // 2. Managed app-data location (new externalized path)
    // 3. Development fallback (staged by prepare-onnx-runtime.mjs)
    let runtime_status_snapshot =
        separator::runtime_bootstrap::runtime_status_snapshot(&app_data_dir);
    let runtime_bootstrap_status = Arc::new(Mutex::new(
        commands::runtime_bootstrap::RuntimeBootstrapStatusSnapshot::from(
            runtime_status_snapshot.clone(),
        ),
    ));

    // Attempt to load the runtime if available. If not, the app starts
    // without it — separation commands will gate on runtime readiness.
    if runtime_status_snapshot.status == separator::runtime_bootstrap::RuntimeStatus::Ready {
        // Try to initialize ORT from the verified runtime path.
        let runtime_path = separator::runtime_bootstrap::ensure_runtime_verified(&app_data_dir)
            .or_else(|_| separator::model::resolve_runtime_library_path(Some(&app_resource_dir)));
        match runtime_path {
            Ok(path) => {
                if let Err(err) = separator::model::ensure_runtime_loaded_from_path(&path) {
                    eprintln!(
                        "warning: failed to load ONNX Runtime from {}: {err:#}",
                        path.display()
                    );
                }
            }
            Err(err) => {
                eprintln!("warning: ONNX Runtime not available: {err:#}");
            }
        }
    } else {
        // Try the legacy bundled path as a fallback during the transition.
        match separator::model::resolve_runtime_library_path(Some(&app_resource_dir)) {
            Ok(path) => {
                if let Err(err) = separator::model::ensure_runtime_loaded_from_path(&path) {
                    eprintln!("warning: failed to load bundled ONNX Runtime: {err:#}");
                }
                // Update status to Ready since the bundled runtime loaded.
                let ready_snapshot = commands::runtime_bootstrap::RuntimeBootstrapStatusSnapshot {
                    state: commands::runtime_bootstrap::RuntimeBootstrapState::Ready,
                    runtime_path: path.display().to_string(),
                    downloaded_bytes: None,
                    total_bytes: None,
                    version: separator::runtime_bootstrap::ORT_RUNTIME_VERSION.to_owned(),
                    error: None,
                };
                if let Ok(mut current) = runtime_bootstrap_status.lock() {
                    *current = ready_snapshot;
                }
            }
            Err(err) => {
                eprintln!("warning: ONNX Runtime not available at startup: {err:#}");
                eprintln!("  Separation will be unavailable until the runtime is downloaded.");
            }
        }
    }

    let app_config = config::load_config(&app_data_dir).with_context(|| {
        format!(
            "failed to load application config from {}",
            app_data_dir.display()
        )
    })?;

    // Crash recovery: if a mirror operation was interrupted mid-sync, the
    // config's active_library_id may still point at the remote library.
    // Restore the original from the pending marker before proceeding.
    // The pending_mirror_restore flag distinguishes "interrupted sync with
    // original active_library_id = None" from "no pending sync at all".
    let app_config = if let Some(mut config) = app_config {
        if config.pending_mirror_restore {
            let original_id = config.pending_mirror_restore_active_library_id.take();
            eprintln!(
                "recovering from interrupted mirror: restoring active_library_id to {:?}",
                original_id
            );
            config.active_library_id = original_id;
            config.pending_mirror_restore = false;
            if let Err(e) = config::save_config(&app_data_dir, &config) {
                eprintln!("warning: failed to persist mirror recovery config: {e}");
            }
        }
        Some(config)
    } else {
        None
    };
    let configured_window_count = app.webview_windows().len();
    if configured_window_count == 0 {
        eprintln!("warning: no Tauri webview windows were created during startup");
    }
    let window_shell_state = crate::window_shell::initialize_main_window(app, app_config.as_ref());

    let playback = Arc::new(Mutex::new({
        let mut controller = PlaybackController::default();
        // Initialize EQ config from the persisted config so the output
        // callback starts with the correct enabled/gains state from the
        // first callback, without waiting for a settings command.
        if let Some(config) = app_config.as_ref() {
            let eq_enabled = config.effective_eq_enabled();
            let eq_gains_db = config.effective_eq_gains_db();
            controller.set_eq_enabled(eq_enabled);
            controller.set_eq_gains(eq_gains_db);
            // #89: Initialize crossfade config from persisted settings.
            controller.crossfade_config.enabled = config.effective_crossfade_enabled();
            controller.crossfade_config.duration_ms = config.effective_crossfade_duration_ms();
        }
        controller
    }));
    let cdg_state: Arc<Mutex<Option<commands::cdg::CdgPlaybackState>>> = Arc::new(Mutex::new(None));
    let airplay_audio_tap = Arc::new(airplay_stream::AirPlayAudioTap::new(12));
    let airplay_stream_generation = Arc::new(AtomicU64::new(1));
    let airplay_audience_active = Arc::new(AtomicBool::new(false));
    let airplay_control_refresh_token = Arc::new(AtomicU64::new(0));
    let airplay_local_output_suppressed = Arc::new(AtomicBool::new(false));
    let model_bootstrap = build_startup_model_bootstrap(&app_data_dir, app_config.as_ref())?;
    let model_bootstrap_status = Arc::new(Mutex::new(model_bootstrap.status.clone()));

    app.manage(window_shell_state);
    let shutdown = Arc::new(AtomicBool::new(false));

    // Construct domain states first — they are the source of truth for shared Arc references.
    let library = Arc::new(Mutex::new(load_library(app_config.as_ref())));
    let (playback_state, command_rx) = PlaybackState::new(Arc::clone(&playback));
    let airplay_state = AirPlayState {
        airplay_audio_tap: Arc::clone(&airplay_audio_tap),
        airplay_stream_generation: Arc::clone(&airplay_stream_generation),
        airplay_audience_active: Arc::clone(&airplay_audience_active),
        airplay_control_refresh_token: Arc::clone(&airplay_control_refresh_token),
        airplay_http_server: Arc::new(Mutex::new(None)),
        airplay_local_output_suppressed: Arc::clone(&airplay_local_output_suppressed),
    };
    let separation_state = SeparationState::new();
    let remote_cache_bytes_limit = app_config
        .as_ref()
        .and_then(|config| config.effective_remote_cache_bytes_limit());
    let remote_state = RemoteState::new_with_limit(&app_data_dir, remote_cache_bytes_limit);
    let shell_state = AppShell::new(
        Arc::clone(&library),
        app_data_dir.clone(),
        app_resource_dir.clone(),
        model_bootstrap.model_path.clone(),
        Arc::clone(&model_bootstrap_status),
        Arc::clone(&runtime_bootstrap_status),
    );

    // Register domain states — commands can extract State<'_, PlaybackState> etc.
    // Clone before manage() since ensure_output_thread needs refs below.
    let playback_state_for_output = playback_state.clone();
    let airplay_state_for_output = airplay_state.clone();

    // Build the composition AppState from domain states.
    let app_state = AppState {
        playback: playback_state.clone(),
        airplay: airplay_state.clone(),
        separation: separation_state.clone(),
        remote: remote_state.clone(),
        shell: shell_state.clone(),
        lrclib_client: crate::lyrics::lrclib::LrcLibClient::new_default(),
        lrcapi_client: crate::lyrics::lrcapi::LrcApiClient::new_default(),
    };

    // Extract coordinator runtime Arcs before managing moves the states.
    let coordinator_runtime = CoordinatorRuntime {
        app_handle: app.handle().clone(),
        playback: Arc::clone(&playback),
        cdg_state: Arc::clone(&cdg_state),
        latest_request_id: Arc::clone(&playback_state.playback_request_id),
        output_started: Arc::clone(&playback_state.audio_output_started),
        output_start_lock: Arc::clone(&playback_state.audio_output_start_lock),
        airplay: airplay_state.clone(),
        shutdown: Arc::clone(&shutdown),
        peak_ring: Arc::clone(&playback_state.peak_ring),
        output_format: Arc::clone(&playback_state.output_format),
    };

    app.manage(playback_state);
    app.manage(airplay_state);
    app.manage(separation_state);
    app.manage(remote_state);
    app.manage(shell_state);
    app.manage(app_state);

    // Spawn the PlaybackCoordinator before pre-warming the output thread.
    // The coordinator serializes all control-plane mutations; the receiver
    // is moved into the worker and only the sender remains in managed state.
    spawn_coordinator(coordinator_runtime, command_rx);

    if let Err(err) = crate::services::playback::ensure_output_thread_inner(
        &playback_state_for_output.audio_output_started,
        &playback_state_for_output.audio_output_start_lock,
        playback_state_for_output.playback.clone(),
        airplay_state_for_output.airplay_audio_tap.clone(),
        airplay_state_for_output
            .airplay_local_output_suppressed
            .clone(),
        Arc::clone(&shutdown),
        playback_state_for_output.peak_ring.clone(),
        playback_state_for_output.output_format.clone(),
    ) {
        eprintln!("warning: failed to pre-warm audio output: {err:#}");
    }

    airplay_stream::spawn_audio_forwarder(Arc::clone(&airplay_audio_tap));
    crate::services::playback::spawn_airplay_control_refresh_worker(
        Arc::clone(&airplay_audience_active),
        Arc::clone(&airplay_control_refresh_token),
        Arc::clone(&airplay_audio_tap),
        Arc::clone(&airplay_stream_generation),
        Arc::clone(&shutdown),
    );
    spawn_playback_position_emitter(app.handle().clone(), playback);

    if model_bootstrap.should_spawn_bootstrap_worker {
        spawn_model_bootstrap_worker(
            app.handle().clone(),
            model_bootstrap.managed_model_path,
            model_bootstrap.descriptor,
            model_bootstrap_status,
        );
    }

    Ok(())
}

struct StartupBootstrapResources {
    model_path: PathBuf,
    managed_model_path: PathBuf,
    status: commands::bootstrap::ModelBootstrapStatusSnapshot,
    should_spawn_bootstrap_worker: bool,
    descriptor: &'static separator::bootstrap::ModelDescriptor,
}

fn build_startup_model_bootstrap(
    app_data_dir: &std::path::Path,
    app_config: Option<&config::AppConfig>,
) -> anyhow::Result<StartupBootstrapResources> {
    let active_variant = app_config
        .map(|config| config.effective_model_variant())
        .unwrap_or_default();
    let descriptor = separator::bootstrap::descriptor_for(active_variant);
    let startup_bootstrap = derive_startup_model_bootstrap(
        app_data_dir,
        &separator::model::default_model_path_for_filename(descriptor.filename),
        active_variant,
        descriptor.sha256,
    )?;

    Ok(StartupBootstrapResources {
        model_path: startup_bootstrap.model_path,
        managed_model_path: startup_bootstrap.managed_model_path,
        status: startup_bootstrap.status,
        should_spawn_bootstrap_worker: startup_bootstrap.should_spawn_bootstrap_worker,
        descriptor,
    })
}

fn load_library(app_config: Option<&config::AppConfig>) -> Option<LibraryRoot> {
    let path = app_config
        .and_then(|config| config.active_library())
        .and_then(|library| library.working_copy_root())?;
    let lib_path = PathBuf::from(&path);

    match LibraryRoot::open(&lib_path) {
        Ok(lib) => {
            let db_path = lib.database_path();
            if let Err(err) = cache::initialize_library_database(&db_path) {
                eprintln!(
                    "warning: failed to apply migrations on library at {}: {}",
                    lib_path.display(),
                    err
                );
            }
            Some(lib)
        }
        Err(err) => {
            eprintln!(
                "warning: could not open library at {}: {}",
                lib_path.display(),
                err
            );
            None
        }
    }
}

fn spawn_playback_position_emitter<R: Runtime>(
    app_handle: tauri::AppHandle<R>,
    playback: Arc<Mutex<PlaybackController>>,
) {
    thread::spawn(move || {
        let mut last_emitted_position = None;
        let mut was_playing = false;
        let mut last_song_id: Option<String> = None;
        let mut last_emitted_state: Option<String> = None;
        let mut last_emitted_is_playing: Option<bool> = None;

        loop {
            thread::sleep(Duration::from_millis(PLAYBACK_POSITION_POLL_INTERVAL_MS));

            // #88: Drain any completed gapless transition before taking the
            // snapshot. The realtime callback stamps a `CompletedTransition`
            // after a gapless swap; we emit `track-transitioned` here so the
            // frontend can reconcile its queue head before the next position
            // event arrives with the new song_id.
            let pending_transition = match playback.lock() {
                Ok(mut controller) => controller.drain_pending_transition(),
                Err(_) => break,
            };
            if let Some(transition) = pending_transition {
                let _ = app_handle.emit(
                    audio::playback::TRACK_TRANSITIONED_EVENT,
                    audio::playback::TrackTransitionedEvent {
                        transition_serial: transition.transition_serial,
                        from_song_id: transition.from_song_id,
                        to_song_id: transition.to_song_id,
                    },
                );
                // Force the next position event to emit regardless of delta —
                // the song_id has changed and the frontend needs the new
                // snapshot immediately.
                last_emitted_position = None;
                last_emitted_state = None;
                last_emitted_is_playing = None;
            }

            let snapshot = match playback.lock() {
                Ok(mut controller) => controller.snapshot(),
                Err(_) => break,
            };

            if was_playing
                && !snapshot.is_playing
                && snapshot
                    .duration_ms
                    .is_some_and(|d| snapshot.position_ms >= d)
                && last_song_id.is_some()
                && last_song_id == snapshot.song_id
            {
                if let Some(ref song_id) = snapshot.song_id {
                    let _ = app_handle.emit(
                        audio::playback::PLAYBACK_ENDED_EVENT,
                        audio::playback::PlaybackEndedEvent {
                            song_id: song_id.clone(),
                        },
                    );
                }
            }

            was_playing = snapshot.is_playing;
            last_song_id = snapshot.song_id.clone();

            if snapshot.song_id.is_none() {
                last_emitted_position = None;
                last_emitted_state = None;
                last_emitted_is_playing = None;
                continue;
            }

            // AirPlay scene content and local playback telemetry are separate
            // concerns. `airplay_audience_active` previously meant merely that
            // the projected scene mode was lyrics/CDG, which is true whenever a
            // song is loaded on macOS even with no AirPlay route selected. Using
            // it to suppress this emitter removed the local WebView's only
            // post-seek `buffering` -> `playing` clock update and froze lyrics.
            // Always publish local transport position/state; AirPlay surfaces
            // consume their own displayed-position clock independently.

            if should_emit_playback_position(
                last_emitted_position,
                last_emitted_state.as_deref(),
                last_emitted_is_playing,
                &snapshot,
            ) {
                let _ = app_handle.emit(
                    audio::playback::PLAYBACK_POSITION_EVENT,
                    audio::playback::playback_position_event(&snapshot),
                );
                last_emitted_position = Some(snapshot.position_ms);
                last_emitted_state = Some(snapshot.state.clone());
                last_emitted_is_playing = Some(snapshot.is_playing);
            }
        }
    });
}

fn should_emit_playback_position(
    last_position_ms: Option<u64>,
    last_state: Option<&str>,
    last_is_playing: Option<bool>,
    snapshot: &audio::playback::PlaybackStateSnapshot,
) -> bool {
    let position_delta_ms = last_position_ms
        .map(|last| snapshot.position_ms.abs_diff(last))
        .unwrap_or(u64::MAX);
    let state_changed = last_state != Some(snapshot.state.as_str());
    let is_playing_changed = last_is_playing != Some(snapshot.is_playing);

    // State transitions must be emitted even at an unchanged position. A
    // streaming seek intentionally holds position while buffering; the
    // buffering -> playing edge is what lets the frontend resume its local
    // monotonic clock without running lyrics ahead of silent audio.
    position_delta_ms > 16 || state_changed || is_playing_changed
}

pub(crate) fn spawn_model_bootstrap_worker<R: Runtime>(
    app_handle: tauri::AppHandle<R>,
    model_path: PathBuf,
    descriptor: &'static separator::bootstrap::ModelDescriptor,
    status: Arc<Mutex<commands::bootstrap::ModelBootstrapStatusSnapshot>>,
) {
    let progress_path = model_path.display().to_string();
    tauri::async_runtime::spawn(async move {
        let blocking_status = Arc::clone(&status);
        let blocking_app_handle = app_handle.clone();
        let blocking_model_path = model_path.clone();
        let progress_path = progress_path.clone();

        let result = tauri::async_runtime::spawn_blocking(move || {
            separator::bootstrap::download_and_install_model(
                &blocking_model_path,
                descriptor.download_url,
                descriptor.sha256,
                |downloaded_bytes, total_bytes| {
                    let snapshot = commands::bootstrap::downloading_status(
                        progress_path.clone(),
                        downloaded_bytes,
                        total_bytes,
                    );
                    if let Ok(mut current) = blocking_status.lock() {
                        *current = snapshot.clone();
                    }
                    let _ = blocking_app_handle.emit(
                        commands::bootstrap::MODEL_BOOTSTRAP_PROGRESS_EVENT,
                        snapshot,
                    );
                },
            )
        })
        .await;

        let snapshot = match result {
            Ok(Ok(())) => commands::bootstrap::ready_status(model_path.display().to_string()),
            Ok(Err(error)) => commands::bootstrap::failed_status(
                model_path.display().to_string(),
                commands::error::model_bootstrap_error(error.to_string()),
            ),
            Err(error) => commands::bootstrap::failed_status(
                model_path.display().to_string(),
                commands::error::model_bootstrap_error(error.to_string()),
            ),
        };

        if let Ok(mut current) = status.lock() {
            *current = snapshot.clone();
        }

        let event = match snapshot.state {
            commands::bootstrap::ModelBootstrapState::Ready => {
                commands::bootstrap::MODEL_BOOTSTRAP_READY_EVENT
            }
            commands::bootstrap::ModelBootstrapState::Failed => {
                commands::bootstrap::MODEL_BOOTSTRAP_ERROR_EVENT
            }
            commands::bootstrap::ModelBootstrapState::Pending
            | commands::bootstrap::ModelBootstrapState::Downloading
            | commands::bootstrap::ModelBootstrapState::Outdated => return,
        };
        let _ = app_handle.emit(event, snapshot);
    });
}

#[cfg(test)]
mod playback_position_emitter_tests {
    use super::should_emit_playback_position;
    use crate::audio::playback::{PlaybackStateSnapshot, StemVolumes};

    fn snapshot(state: &str, is_playing: bool, position_ms: u64) -> PlaybackStateSnapshot {
        PlaybackStateSnapshot {
            song_id: Some("song-1".to_owned()),
            transport_generation: 2,
            state: state.to_owned(),
            is_playing,
            position_ms,
            duration_ms: Some(60_000),
            buffered_ms: position_ms,
            volume: 1.0,
            stem_volumes: StemVolumes::default(),
            has_stems: false,
            stem_mode: None,
        }
    }

    #[test]
    fn emits_buffering_recovery_without_position_delta() {
        let recovered = snapshot("playing", true, 15_000);
        assert!(should_emit_playback_position(
            Some(15_000),
            Some("buffering"),
            Some(true),
            &recovered,
        ));
    }

    #[test]
    fn emits_normal_position_progress() {
        let progressed = snapshot("playing", true, 15_033);
        assert!(should_emit_playback_position(
            Some(15_000),
            Some("playing"),
            Some(true),
            &progressed,
        ));
    }

    #[test]
    fn suppresses_unchanged_duplicate_snapshot() {
        let duplicate = snapshot("playing", true, 15_000);
        assert!(!should_emit_playback_position(
            Some(15_000),
            Some("playing"),
            Some(true),
            &duplicate,
        ));
    }
}
