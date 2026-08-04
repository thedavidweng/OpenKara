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

/// Resolve the application data directory.
///
/// When the binary is built with `automation-smoke`, callers may set
/// `OPENKARA_APP_DATA_DIR` so installed-app UI Automation and fault suites
/// share the same isolated tree as the automation driver. Normal user builds
/// always use the OS-managed app data path.
fn resolve_app_data_dir<R: Runtime>(app: &tauri::App<R>) -> anyhow::Result<PathBuf> {
    #[cfg(feature = "automation-smoke")]
    if let Ok(override_dir) = std::env::var("OPENKARA_APP_DATA_DIR") {
        let path = PathBuf::from(override_dir);
        if !path.as_os_str().is_empty() {
            return Ok(path);
        }
    }

    app.path()
        .app_data_dir()
        .context("failed to resolve application data directory")
}

pub fn setup_app<R: Runtime>(app: &mut tauri::App<R>) -> Result<(), Box<dyn std::error::Error>> {
    match app.path().app_log_dir() {
        Ok(log_dir) => {
            if let Err(err) = crate::logging::init(&log_dir) {
                eprintln!("warning: failed to initialize file logging: {err:#}");
            } else {
                tracing::info!(
                    log_dir = %log_dir.display(),
                    "OpenKara starting; file logging initialized"
                );
            }
        }
        Err(err) => {
            eprintln!("warning: could not resolve log directory; file logging disabled: {err:#}");
        }
    }

    let app_resource_dir = app
        .path()
        .resource_dir()
        .context("failed to resolve bundled resource directory")?;

    let app_data_dir = resolve_app_data_dir(app)?;
    fs::create_dir_all(&app_data_dir).with_context(|| {
        format!(
            "failed to create application data directory at {}",
            app_data_dir.display()
        )
    })?;

    match separator::runtime_bootstrap::begin_startup(&app_data_dir) {
        Ok(Some(plan)) => {
            match commands::runtime_bootstrap::ensure_runtime_loaded_with_watchdog(
                &plan.library_path,
            ) {
                Ok(_) => {
                    if plan.proving_candidate {
                        if let Err(err) =
                            separator::runtime_bootstrap::finish_activation_success(&app_data_dir)
                        {
                            tracing::warn!("failed to finalize runtime activation: {err:#}");
                        }
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "failed to load ONNX Runtime from {}: {err:#}",
                        plan.library_path.display()
                    );
                    if !plan.proving_candidate && !plan.is_legacy {
                        let failed_id = plan
                            .record
                            .as_ref()
                            .map(|record| record.artifact_id.clone())
                            .unwrap_or_default();
                        match separator::runtime_bootstrap::rollback_failed_activation(
                            &app_data_dir,
                            &failed_id,
                            &err.to_string(),
                        ) {
                            Ok(Some(previous)) => {
                                if let Err(load_err) =
                                    commands::runtime_bootstrap::ensure_runtime_loaded_with_watchdog(
                                        &previous.library_path,
                                    )
                                {
                                    tracing::warn!(
                                        "failed to load previous ONNX Runtime {}: {load_err:#}",
                                        previous.library_path.display()
                                    );
                                }
                            }
                            Ok(None) => {}
                            Err(rollback_err) => {
                                tracing::warn!(
                                    "failed to record runtime load failure: {rollback_err:#}"
                                );
                            }
                        }
                    }
                    if plan.proving_candidate {
                        let failed_id = plan
                            .record
                            .as_ref()
                            .map(|record| record.artifact_id.clone())
                            .unwrap_or_default();
                        match separator::runtime_bootstrap::rollback_failed_activation(
                            &app_data_dir,
                            &failed_id,
                            &err.to_string(),
                        ) {
                            Ok(Some(previous)) => {
                                if let Err(load_err) =
                                    commands::runtime_bootstrap::ensure_runtime_loaded_with_watchdog(
                                        &previous.library_path,
                                    )
                                {
                                    tracing::warn!(
                                        "failed to restore previous ONNX Runtime {}: {load_err:#}",
                                        previous.library_path.display()
                                    );
                                }
                            }
                            Ok(None) => {
                                tracing::warn!(
                                    "no previous ONNX Runtime available after failed activation"
                                );
                            }
                            Err(rollback_err) => {
                                tracing::warn!(
                                    "failed to roll back runtime activation: {rollback_err:#}"
                                );
                            }
                        }
                    }
                }
            }
        }
        Ok(None) => {}
        Err(err) => {
            tracing::warn!("runtime startup resolution failed: {err:#}");
        }
    }

    let runtime_bootstrap_status = Arc::new(Mutex::new(
        commands::runtime_bootstrap::snapshot_from_disk(&app_data_dir),
    ));

    // A config problem must never brick startup (issue #208). `load_config`
    // already quarantines a corrupt file and returns `Ok(None)`; a residual
    // I/O error here is treated as recoverable too, falling back to defaults
    // instead of aborting the app.
    let app_config = match config::load_config(&app_data_dir) {
        Ok(config) => config,
        Err(err) => {
            tracing::warn!(
                "failed to load application config from {} ({err:#}); \
                 starting with default settings",
                app_data_dir.display()
            );
            None
        }
    };

    let app_config = if let Some(mut config) = app_config {
        if config.pending_mirror_restore {
            let original_id = config.pending_mirror_restore_active_library_id.take();
            tracing::info!(
                "recovering from interrupted mirror: restoring active_library_id to {:?}",
                original_id
            );
            config.active_library_id = original_id;
            config.pending_mirror_restore = false;
            if let Err(e) = config::save_config(&app_data_dir, &config) {
                tracing::warn!("failed to persist mirror recovery config: {e}");
            }
        }
        Some(config)
    } else {
        None
    };
    let configured_window_count = app.webview_windows().len();
    if configured_window_count == 0 {
        tracing::warn!("no Tauri webview windows were created during startup");
    }
    let window_shell_state = crate::window_shell::initialize_main_window(app, app_config.as_ref());

    let playback = Arc::new(Mutex::new({
        let mut controller = PlaybackController::default();
        if let Some(config) = app_config.as_ref() {
            let eq_enabled = config.effective_eq_enabled();
            let eq_gains_db = config.effective_eq_gains_db();
            controller.set_eq_enabled(eq_enabled);
            controller.set_eq_gains(eq_gains_db);
            let crossfade_enabled = config.effective_crossfade_enabled();
            let crossfade_duration_ms = config.effective_crossfade_duration_ms();
            let _ = controller.set_crossfade_enabled(crossfade_enabled);
            let _ = controller.set_crossfade_duration(crossfade_duration_ms);
        }
        controller
    }));
    let airplay_audio_tap = Arc::new(airplay_stream::AirPlayAudioTap::new(12));
    let airplay_stream_generation = Arc::new(AtomicU64::new(1));
    let airplay_audience_active = Arc::new(AtomicBool::new(false));
    let airplay_control_refresh_token = Arc::new(AtomicU64::new(0));
    let airplay_local_output_suppressed = Arc::new(AtomicBool::new(false));
    let model_bootstrap = build_startup_model_bootstrap(&app_data_dir, app_config.as_ref())?;
    let model_bootstrap_status = Arc::new(Mutex::new(model_bootstrap.status.clone()));

    app.manage(window_shell_state);
    let shutdown = Arc::new(AtomicBool::new(false));

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
        .and_then(|config| config.remote_cache_bytes_limit);
    let remote_state = RemoteState::new_with_limit(&app_data_dir, remote_cache_bytes_limit);

    // Run startup recovery for the durable remote control plane. The recovery
    // pass completes before the executor thread processes pending work.
    // It must not block library startup because it runs after the control DB
    // is open.
    run_remote_recovery(&remote_state, &app_data_dir);

    let shell_state = AppShell::new(
        Arc::clone(&library),
        app_data_dir.clone(),
        app_resource_dir.clone(),
        model_bootstrap.model_path.clone(),
        Arc::clone(&model_bootstrap_status),
        Arc::clone(&runtime_bootstrap_status),
    );
    let catalog_cache_for_updates = Arc::clone(&shell_state.catalog_cache);
    let runtime_status_for_updates = Arc::clone(&shell_state.runtime_bootstrap_status);

    let playback_state_for_output = playback_state.clone();
    let airplay_state_for_output = airplay_state.clone();

    let app_state = AppState {
        playback: playback_state.clone(),
        airplay: airplay_state.clone(),
        separation: separation_state.clone(),
        remote: remote_state.clone(),
        shell: shell_state.clone(),
        lrclib_client: crate::lyrics::lrclib::LrcLibClient::new_default(),
        lrcapi_client: crate::lyrics::lrcapi::LrcApiClient::new_default(),
    };

    let coordinator_runtime = CoordinatorRuntime {
        app_handle: app.handle().clone(),
        playback: Arc::clone(&playback),
        cdg_state: Arc::clone(&playback_state.cdg_state),
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
    app.manage(app_state.clone());

    spawn_durable_operation_executor(app_state, app.handle().clone());

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
        tracing::warn!("failed to pre-warm audio output: {err:#}");
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

    let update_policy = app_config
        .as_ref()
        .map(|config| config.effective_update_policy())
        .unwrap_or_default();
    if update_policy != config::UpdatePolicy::Manual {
        spawn_runtime_update_check_worker(
            app.handle().clone(),
            app_data_dir.clone(),
            Arc::clone(&catalog_cache_for_updates),
            Arc::clone(&runtime_status_for_updates),
            update_policy,
        );
    }

    // Last statement on purpose: the deadline must start when the event loop
    // starts, not when setup starts. See `spawn_window_reveal_watchdog`.
    spawn_window_reveal_watchdog(app);

    Ok(())
}

/// Deadline for the native reveal watchdog, measured from the end of
/// `setup_app`.
///
/// RATIONALE: the main window starts hidden and the reveal is a frontend
/// handshake (`window_ready`), but macOS suspends animation frames *and* JS
/// timers in a WKWebView whose window has never been shown. With both
/// suspended no frontend path out of the hidden state survives, and the app
/// runs as an invisible process the user can only kill from Activity Monitor.
///
/// 2 s is chosen against the healthy handshake budget: after the event loop
/// starts it costs at most about 900 ms (the 750 ms native-`setTheme` guard in
/// `useThemeRuntime` plus the 120 ms reveal backstop, on top of two local IPC
/// reads), so the flash-free path keeps better than 2x headroom to win.
const WINDOW_REVEAL_WATCHDOG: Duration = Duration::from_secs(2);

/// Reveals the main window if the frontend handshake never arrives.
///
/// Spawned as the last step of `setup_app` because Tauri runs the setup hook
/// on the main thread before the event loop starts: `window_ready` cannot be
/// served until setup returns, so an earlier deadline would charge startup
/// work (an ONNX Runtime load, a library scan) against the frontend's budget
/// and fire on healthy launches.
fn spawn_window_reveal_watchdog<R: Runtime>(app: &tauri::App<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    thread::spawn(move || {
        thread::sleep(WINDOW_REVEAL_WATCHDOG);

        if window.is_visible().unwrap_or(false) {
            return;
        }

        tracing::warn!(
            "main window still hidden after {:?}; revealing without the frontend handshake",
            WINDOW_REVEAL_WATCHDOG
        );
        if let Err(error) = window.show() {
            tracing::warn!("reveal watchdog could not show the main window: {error}");
        }
    });
}

fn spawn_runtime_update_check_worker<R: Runtime>(
    app_handle: tauri::AppHandle<R>,
    app_data_dir: PathBuf,
    catalog_cache: Arc<Mutex<Option<separator::catalog::VerifiedCatalog>>>,
    status: Arc<Mutex<commands::runtime_bootstrap::RuntimeBootstrapStatusSnapshot>>,
    policy: config::UpdatePolicy,
) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;

        let _ = tauri::async_runtime::spawn_blocking(move || {
            let catalog = match separator::catalog::fetch_stable_catalog() {
                Ok(catalog) => catalog,
                Err(error) => {
                    tracing::warn!("runtime update check skipped: {error:#}");
                    return;
                }
            };
            let Ok(runtime) = separator::catalog::resolve_runtime(
                &catalog.manifest,
                separator::catalog::current_target_triple(),
            ) else {
                return;
            };

            let inventory = separator::runtime_bootstrap::runtime_inventory(&app_data_dir);
            if inventory.candidate.is_some() {
                return;
            }
            let installed = inventory
                .active
                .as_ref()
                .map(|active| active.record.clone());
            let file_exists = inventory.active.is_some() || inventory.legacy_path.is_some();
            let comparison = match separator::catalog::compare_installed_artifact(
                installed,
                &runtime.artifact_id,
                &runtime.archive_digest,
                &catalog,
                file_exists,
            ) {
                Ok(comparison) => comparison,
                Err(error) => {
                    tracing::warn!("runtime update check skipped: {error:#}");
                    return;
                }
            };

            if let Ok(mut cache) = catalog_cache.lock() {
                *cache = Some(catalog.clone());
            }

            let update_available = matches!(
                comparison.state,
                separator::catalog::ModelUpdateState::UpdateAvailable
                    | separator::catalog::ModelUpdateState::InstalledWithoutIdentity
            ) && file_exists;
            if !update_available {
                return;
            }

            match policy {
                config::UpdatePolicy::AutoDownload => {
                    let mut emit = |event, snapshot| {
                        let _ = app_handle.emit(event, snapshot);
                    };
                    let is_update = commands::runtime_bootstrap::prepare_runtime_download(
                        &app_data_dir,
                        &status,
                        &mut emit,
                    );
                    if let Err(error) =
                        commands::runtime_bootstrap::download_runtime_blocking_with_catalog(
                            &app_data_dir,
                            &catalog,
                            is_update,
                            &status,
                            &mut emit,
                        )
                    {
                        tracing::warn!(
                            code = ?error.code,
                            message = %error.message,
                            "automatic runtime update download failed"
                        );
                    }
                }
                config::UpdatePolicy::Notify | config::UpdatePolicy::Manual => {
                    if let Ok(mut current) = status.lock() {
                        if current.state
                            == commands::runtime_bootstrap::RuntimeBootstrapState::Ready
                        {
                            current.state =
                                commands::runtime_bootstrap::RuntimeBootstrapState::UpdateAvailable;
                            let _ = app_handle.emit(
                                commands::runtime_bootstrap::RUNTIME_BOOTSTRAP_PROGRESS_EVENT,
                                current.clone(),
                            );
                        }
                    }
                }
            }
        })
        .await;
    });
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
        &separator::model::default_model_path_for_filename(&descriptor.filename),
        active_variant,
        &descriptor.file_sha256,
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
                tracing::warn!(
                    "failed to apply migrations on library at {}: {}",
                    lib_path.display(),
                    err
                );
            }
            Some(lib)
        }
        Err(err) => {
            tracing::warn!("could not open library at {}: {}", lib_path.display(), err);
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
                        state: transition.snapshot.clone(),
                    },
                );
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
                descriptor,
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

/// Run the startup recovery pass for the durable remote control plane.
///
/// This transitions interrupted operations (running/committing/verifying →
/// retry_wait, prepared → cancelled/pending/conflicted). The executor thread
/// re-executes pending work after app state initialization.
///
/// The recovery pass runs after the control DB is open and must not block
/// library startup. Errors are logged but do not abort app startup — a
/// failed recovery leaves operations in their pre-recovery states, which is
/// safe because the next startup will retry recovery.
fn run_remote_recovery(remote_state: &RemoteState, app_data_dir: &std::path::Path) {
    use crate::remote::recovery::{run_recovery, Clock, FileDigestResolver};

    if !remote_state.is_available() {
        tracing::warn!("remote repository state is unavailable; skipping recovery");
        return;
    }

    project_library_outboxes_into_control_db(remote_state, app_data_dir);

    let recovery_result = {
        let conn = match remote_state.control_db().and_then(|db| {
            db.lock().map_err(|_| {
                crate::commands::error::state_lock_error("control DB lock was poisoned")
            })
        }) {
            Ok(conn) => conn,
            Err(_) => {
                tracing::warn!("remote control DB lock was poisoned during recovery");
                return;
            }
        };

        let app_data_dir_owned = app_data_dir.to_path_buf();
        let resolver = FileDigestResolver::new(move |library_id: &str| {
            let config = crate::config::load_config(&app_data_dir_owned)
                .ok()
                .flatten()?;
            let library = config.libraries.iter().find(|l| l.id() == library_id)?;
            let root_path = library.working_copy_root()?;
            let root = crate::library_root::LibraryRoot::open(&root_path).ok()?;
            Some(root.database_path())
        });

        let clock: Clock = Box::new(crate::remote::types::current_unix_time_ms);

        run_recovery(&conn, &resolver, &clock)
    };

    if let Err(error) = recovery_result {
        tracing::warn!("remote control DB recovery failed: {:?}", error);
    }

    let control_db_path = crate::remote::control_db::control_db_path(app_data_dir);
    if let Ok(part_cleanup_conn) = crate::remote::control_db::open_control_db(&control_db_path) {
        recover_stale_part_files_for_all_libraries(app_data_dir, &part_cleanup_conn);
    } else {
        // Fail closed: without the control plane we cannot tell which
        // partials are resumable. Leave every `*.part.*` file in place
        // rather than deleting them as orphans.
        tracing::warn!(
            "control DB unavailable during part-file recovery; \
             preserving all partial downloads (fail-closed)"
        );
    }
}

/// Project unprojected library-DB publish outbox rows into remote-state.db.
/// Fail closed: never delete / mark projected unless control projection
/// succeeded. Leave outbox retryable on any error.
/// Whether a residual library outbox row may be deleted because a terminal
/// control-DB operation already covers its intent.
///
/// Safe only when:
/// - operation `library_id` matches the outbox library
/// - terminal payload is non-empty
/// - every outbox song id is in the terminal payload (outbox ⊆ payload)
///
/// Empty terminal payload must never authorize deleting a non-empty outbox.
/// `payload ⊆ outbox` is intentionally rejected so a partial payload cannot
/// discard unmerged songs still sitting in the outbox.
fn residual_outbox_safe_to_drop(
    op_library_id: &str,
    outbox_library_id: &str,
    outbox_song_ids: &[String],
    outbox_whole_repository: bool,
    terminal_payload_song_ids: &[String],
    terminal_whole_repository: bool,
) -> bool {
    if op_library_id != outbox_library_id {
        return false;
    }
    if outbox_whole_repository {
        return terminal_whole_repository;
    }
    if terminal_payload_song_ids.is_empty() {
        return false;
    }
    outbox_song_ids
        .iter()
        .all(|s| terminal_payload_song_ids.contains(s))
}

fn project_library_outboxes_into_control_db(
    remote_state: &RemoteState,
    app_data_dir: &std::path::Path,
) {
    use crate::remote::control_db::{
        bind_scope_mark_pending_and_dirty_tx, get_operation, upsert_operation, OperationKind,
        OperationPayload, OperationRow, OperationState,
    };
    use crate::remote::library_outbox::{
        delete_library_publish_outbox, list_unprojected_library_outbox,
    };

    let config = match crate::config::load_config(app_data_dir) {
        Ok(Some(config)) => config,
        _ => return,
    };
    for library in &config.libraries {
        if !matches!(library, crate::config::RegisteredLibrary::Remote { .. }) {
            continue;
        }
        let library_id = library.id().to_owned();
        let Some(root_path) = library.working_copy_root() else {
            continue;
        };
        let Ok(root) = crate::library_root::LibraryRoot::open(&root_path) else {
            continue;
        };
        let Ok(lib_conn) = crate::cache::open_database(&root.database_path()) else {
            continue;
        };
        if let Err(error) = crate::cache::apply_migrations(&lib_conn) {
            tracing::warn!(
                "library migrations failed during outbox projection for {}: {error}",
                root_path.display()
            );
            continue;
        }
        let rows = match list_unprojected_library_outbox(&lib_conn) {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(
                    "failed to list library outbox for {}: {:?}",
                    root_path.display(),
                    error
                );
                continue;
            }
        };
        if rows.is_empty() {
            continue;
        }
        let Ok(control_db) = remote_state.control_db() else {
            tracing::warn!("control DB unavailable during outbox projection");
            continue;
        };
        let Ok(control) = control_db.lock() else {
            tracing::warn!("control DB lock poisoned during outbox projection");
            continue;
        };
        let now = crate::remote::types::current_unix_time_ms();
        for row in rows {
            if row.song_ids.is_empty() && !row.whole_repository {
                continue;
            }
            let projected = match get_operation(&control, &row.operation_id) {
                Ok(Some(existing)) if existing.state.is_terminal() => {
                    // Projection already finished (or op completed). Do not
                    // reopen a terminal row — only drop residual outbox when
                    // residual_outbox_safe_to_drop says the intent is covered.
                    let payload_ids = crate::remote::control_db::OperationPayload::from_json(
                        &existing.payload_json,
                    )
                    .map(|p| (p.song_ids, p.whole_repository))
                    .unwrap_or_default();
                    if residual_outbox_safe_to_drop(
                        &existing.library_id,
                        &library_id,
                        &row.song_ids,
                        row.whole_repository,
                        &payload_ids.0,
                        payload_ids.1,
                    ) {
                        true
                    } else {
                        tracing::warn!(
                            "residual outbox {} not covered by terminal op; keeping",
                            row.operation_id
                        );
                        false
                    }
                }
                Ok(Some(_)) => bind_scope_mark_pending_and_dirty_tx(
                    &control,
                    &row.operation_id,
                    &library_id,
                    &row.song_ids,
                    row.whole_repository,
                )
                .is_ok(),
                Ok(None) => {
                    let payload = OperationPayload {
                        song_ids: row.song_ids.clone(),
                        whole_repository: row.whole_repository,
                        percent: 0,
                        detail: Some("Recovered from library outbox".to_owned()),
                        ..Default::default()
                    };
                    let payload_json = match payload.to_json() {
                        Ok(json) => json,
                        Err(error) => {
                            tracing::warn!(
                                "outbox payload serialize failed for {}: {:?}",
                                row.operation_id,
                                error
                            );
                            continue;
                        }
                    };
                    let op = OperationRow {
                        operation_id: row.operation_id.clone(),
                        library_id: library_id.clone(),
                        operation_kind: OperationKind::Publish,
                        state: OperationState::Pending,
                        expected_generation: row.expected_generation,
                        target_generation: None,
                        source_db_digest: row.source_db_digest.clone(),
                        candidate_db_digest: None,
                        payload_json,
                        attempt_count: 0,
                        next_attempt_at_ms: None,
                        error_code: None,
                        error_detail: None,
                        created_at_ms: row.created_at_ms,
                        updated_at_ms: now,
                    };
                    match upsert_operation(&control, &op) {
                        Ok(()) => bind_scope_mark_pending_and_dirty_tx(
                            &control,
                            &row.operation_id,
                            &library_id,
                            &row.song_ids,
                            row.whole_repository,
                        )
                        .is_ok(),
                        Err(error) => {
                            tracing::warn!(
                                "control projection upsert failed for {}: {:?}",
                                row.operation_id,
                                error
                            );
                            false
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        "control get_operation failed for {}: {:?}",
                        row.operation_id,
                        error
                    );
                    false
                }
            };
            // Only remove the library outbox after control projection succeeds.
            if projected {
                if let Err(error) = delete_library_publish_outbox(&lib_conn, &row.operation_id) {
                    tracing::warn!(
                        "failed to delete projected outbox {}: {:?}",
                        row.operation_id,
                        error
                    );
                }
            }
        }
    }
}

fn recover_stale_part_files_for_all_libraries(
    app_data_dir: &std::path::Path,
    control_db: &rusqlite::Connection,
) {
    let config = match crate::config::load_config(app_data_dir) {
        Ok(Some(config)) => config,
        _ => return,
    };
    for library in &config.libraries {
        if !matches!(library, crate::config::RegisteredLibrary::Remote { .. }) {
            continue;
        }
        let Some(root_path) = library.working_copy_root() else {
            continue;
        };
        if let Err(error) =
            crate::remote::recovery::recover_stale_part_files(&root_path, control_db)
        {
            tracing::warn!(
                "stale part-file recovery failed for {}: {:?}",
                root_path.display(),
                error
            );
        }
    }
}

fn spawn_durable_operation_executor<R: tauri::Runtime>(
    app_state: AppState,
    app_handle: tauri::AppHandle<R>,
) {
    std::thread::spawn(move || {
        let publish_changes = crate::remote::PublishChanges::new(&app_state, &app_handle);
        if let Err(error) = publish_changes.recover_pending() {
            tracing::warn!(
                "durable operation executor initial pass failed: {:?}",
                error
            );
        }

        let poll_interval = std::time::Duration::from_secs(30);
        loop {
            std::thread::sleep(poll_interval);
            if let Err(error) = publish_changes.recover_pending() {
                tracing::warn!(
                    "durable operation executor periodic pass failed: {:?}",
                    error
                );
            }
        }
    });
}

#[cfg(test)]
mod residual_outbox_tests {
    use super::residual_outbox_safe_to_drop;

    #[test]
    fn equal_sets_are_safe() {
        let ids = vec!["a".to_owned(), "b".to_owned()];
        assert!(residual_outbox_safe_to_drop(
            "lib", "lib", &ids, false, &ids, false
        ));
    }

    #[test]
    fn outbox_subset_of_payload_is_safe() {
        let outbox = vec!["a".to_owned()];
        let payload = vec!["a".to_owned(), "b".to_owned()];
        assert!(residual_outbox_safe_to_drop(
            "lib", "lib", &outbox, false, &payload, false
        ));
    }

    #[test]
    fn payload_subset_of_outbox_is_not_safe() {
        let outbox = vec!["a".to_owned(), "b".to_owned()];
        let payload = vec!["a".to_owned()];
        assert!(!residual_outbox_safe_to_drop(
            "lib", "lib", &outbox, false, &payload, false
        ));
    }

    #[test]
    fn empty_terminal_payload_never_authorizes_delete() {
        let outbox = vec!["a".to_owned()];
        let payload: Vec<String> = vec![];
        assert!(!residual_outbox_safe_to_drop(
            "lib", "lib", &outbox, false, &payload, false
        ));
    }

    #[test]
    fn library_mismatch_is_not_safe() {
        let ids = vec!["a".to_owned()];
        assert!(!residual_outbox_safe_to_drop(
            "lib-a", "lib-b", &ids, false, &ids, false
        ));
    }

    #[test]
    fn matching_whole_repository_scopes_are_safe() {
        assert!(residual_outbox_safe_to_drop(
            "lib",
            "lib",
            &[],
            true,
            &[],
            true
        ));
        assert!(!residual_outbox_safe_to_drop(
            "lib",
            "lib",
            &[],
            true,
            &[],
            false
        ));
    }
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
