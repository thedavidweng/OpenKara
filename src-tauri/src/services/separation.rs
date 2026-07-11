//! Separation lifecycle orchestration.
//!
//! Deep inference stays in `separator::job::separate_song_into_cache`.
//! This module owns status DTOs, bootstrap prerequisites, progress/terminal
//! event emission, and shared single/batch job launching so commands remain
//! thin IPC adapters and `state` does not import `commands::`.

use crate::{
    cache,
    commands::bootstrap::{self, ModelBootstrapStatusSnapshot},
    commands::error::{database_error, state_lock_error, CommandError, CommandResult},
    commands::runtime_bootstrap::{self, RuntimeBootstrapStatusSnapshot},
    config::{self, ExecutionProviderPreference, StemMode},
    library_root::LibraryRoot,
    remote,
    separator::{
        self, error::SeparationError, job::SeparationArtifacts, model::LoadedModel,
        model_cache::ModelCache,
    },
    AppState,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{AppHandle, Emitter, Manager, Runtime};

// ---------------------------------------------------------------------------
// Status / event contracts (IPC-facing; moved out of commands for locality)
// ---------------------------------------------------------------------------

pub const SEPARATION_PROGRESS_EVENT: &str = "separation-progress";
pub const SEPARATION_COMPLETE_EVENT: &str = "separation-complete";
pub const SEPARATION_ERROR_EVENT: &str = "separation-error";

pub const BATCH_SEPARATION_PROGRESS_EVENT: &str = "batch-separation-progress";
pub const BATCH_SEPARATION_COMPLETE_EVENT: &str = "batch-separation-complete";
pub const BATCH_SEPARATION_CANCELLED_EVENT: &str = "batch-separation-cancelled";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeparationState {
    Idle,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeparationStatusSnapshot {
    pub song_id: String,
    pub state: SeparationState,
    pub percent: u8,
    pub cache_hit: bool,
    pub vocals_path: Option<String>,
    pub accomp_path: Option<String>,
    pub drums_path: Option<String>,
    pub bass_path: Option<String>,
    pub other_path: Option<String>,
    pub model_variant: Option<String>,
    pub error: Option<CommandError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeparationProgressEvent {
    pub song_id: String,
    pub percent: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeparationCompleteEvent {
    pub song_id: String,
    pub status: SeparationStatusSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeparationErrorEvent {
    pub song_id: String,
    pub error: CommandError,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchSeparationProgress {
    pub total: usize,
    pub completed: usize,
    pub skipped: usize,
    pub failed: usize,
    pub current_song_id: Option<String>,
    pub current_percent: u8,
}

// ---------------------------------------------------------------------------
// Status constructors
// ---------------------------------------------------------------------------

pub fn idle_status(song_id: impl Into<String>) -> SeparationStatusSnapshot {
    SeparationStatusSnapshot {
        song_id: song_id.into(),
        state: SeparationState::Idle,
        percent: 0,
        cache_hit: false,
        vocals_path: None,
        accomp_path: None,
        drums_path: None,
        bass_path: None,
        other_path: None,
        model_variant: None,
        error: None,
    }
}

pub fn running_status(song_id: impl Into<String>, percent: u8) -> SeparationStatusSnapshot {
    SeparationStatusSnapshot {
        song_id: song_id.into(),
        state: SeparationState::Running,
        percent: percent.min(100),
        cache_hit: false,
        vocals_path: None,
        accomp_path: None,
        drums_path: None,
        bass_path: None,
        other_path: None,
        model_variant: None,
        error: None,
    }
}

pub fn completed_status(
    song_id: impl Into<String>,
    vocals_path: impl Into<String>,
    accomp_path: impl Into<String>,
    cache_hit: bool,
    drums_path: Option<String>,
    bass_path: Option<String>,
    other_path: Option<String>,
) -> SeparationStatusSnapshot {
    SeparationStatusSnapshot {
        song_id: song_id.into(),
        state: SeparationState::Completed,
        percent: 100,
        cache_hit,
        vocals_path: Some(vocals_path.into()),
        accomp_path: Some(accomp_path.into()),
        drums_path,
        bass_path,
        other_path,
        model_variant: None,
        error: None,
    }
}

pub fn completed_status_with_model(
    song_id: impl Into<String>,
    vocals_path: impl Into<String>,
    accomp_path: impl Into<String>,
    cache_hit: bool,
    drums_path: Option<String>,
    bass_path: Option<String>,
    other_path: Option<String>,
    model_variant: String,
) -> SeparationStatusSnapshot {
    SeparationStatusSnapshot {
        song_id: song_id.into(),
        state: SeparationState::Completed,
        percent: 100,
        cache_hit,
        vocals_path: Some(vocals_path.into()),
        accomp_path: Some(accomp_path.into()),
        drums_path,
        bass_path,
        other_path,
        model_variant: Some(model_variant),
        error: None,
    }
}

pub fn failed_status(song_id: impl Into<String>, error: CommandError) -> SeparationStatusSnapshot {
    SeparationStatusSnapshot {
        song_id: song_id.into(),
        state: SeparationState::Failed,
        percent: 100,
        cache_hit: false,
        vocals_path: None,
        accomp_path: None,
        drums_path: None,
        bass_path: None,
        other_path: None,
        model_variant: None,
        error: Some(error),
    }
}

// ---------------------------------------------------------------------------
// Execution context (shared by single-song + batch)
// ---------------------------------------------------------------------------

/// Shared configuration + handles gathered once before spawning workers.
/// Built from AppState so single and batch paths cannot drift.
#[derive(Clone)]
pub struct SeparationExecutionContext {
    pub library_root: LibraryRoot,
    pub model_variant: String,
    pub ep_preference: ExecutionProviderPreference,
    pub stem_mode: StemMode,
    pub app_data_dir: PathBuf,
    pub model_bootstrap_status: Arc<Mutex<ModelBootstrapStatusSnapshot>>,
    pub runtime_bootstrap_status: Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    pub statuses: Arc<Mutex<HashMap<String, SeparationStatusSnapshot>>>,
    pub model_cache: Arc<Mutex<ModelCache<LoadedModel>>>,
}

pub fn build_execution_context(state: &AppState) -> CommandResult<SeparationExecutionContext> {
    let app_config = config::load_config(&state.shell.app_data_dir)
        .ok()
        .flatten();
    let model_variant = app_config
        .as_ref()
        .map(|c| c.effective_model_variant())
        .unwrap_or_default()
        .as_str()
        .to_owned();
    let ep_preference = app_config
        .as_ref()
        .map(|c| c.effective_execution_provider())
        .unwrap_or_default();
    let stem_mode = app_config
        .as_ref()
        .map(|c| c.effective_stem_mode())
        .unwrap_or_default();

    Ok(SeparationExecutionContext {
        library_root: state.library_root()?,
        model_variant,
        ep_preference,
        stem_mode,
        app_data_dir: state.shell.app_data_dir.clone(),
        model_bootstrap_status: Arc::clone(&state.shell.model_bootstrap_status),
        runtime_bootstrap_status: Arc::clone(&state.shell.runtime_bootstrap_status),
        statuses: Arc::clone(&state.separation.separation_statuses),
        model_cache: Arc::clone(&state.separation.separator_model_cache),
    })
}

// ---------------------------------------------------------------------------
// Seams — injectable bootstrap / publish for unit tests without full Tauri
// ---------------------------------------------------------------------------

/// Ensure ONNX Runtime then the active model are ready (download/install if needed).
///
/// Seam: production path uses command bootstrap helpers; tests can call
/// `run_job_blocking` with a known model path and skip this entirely.
pub fn ensure_runtime_and_model_blocking<ER, EM>(
    app_data_dir: &Path,
    runtime_status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    model_status: &Arc<Mutex<ModelBootstrapStatusSnapshot>>,
    emit_runtime: &mut ER,
    emit_model: &mut EM,
) -> CommandResult<PathBuf>
where
    ER: FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
    EM: FnMut(&'static str, ModelBootstrapStatusSnapshot),
{
    runtime_bootstrap::ensure_runtime_ready_or_install_blocking(
        app_data_dir,
        runtime_status,
        emit_runtime,
    )?;
    bootstrap::ensure_active_model_ready_or_install_blocking(app_data_dir, model_status, emit_model)
}

/// Publish a completed song to the active remote library when one is bound.
///
/// Seam: `emit_terminal_status` accepts a custom `publish` callback so tests
/// can assert completion without remote I/O.
pub fn publish_on_complete_default<R: Runtime>(app_handle: &AppHandle<R>, song_id: &str) {
    let state = app_handle.state::<AppState>();
    let _ = remote::publish_song_to_active_remote_if_ready(&state, app_handle, song_id);
}

// ---------------------------------------------------------------------------
// Status map helpers
// ---------------------------------------------------------------------------

pub fn get_separation_status_from_map(
    statuses: &Arc<Mutex<HashMap<String, SeparationStatusSnapshot>>>,
    song_id: &str,
) -> CommandResult<SeparationStatusSnapshot> {
    let statuses = statuses
        .lock()
        .map_err(|_| state_lock_error("separation status lock was poisoned"))?;

    Ok(statuses
        .get(song_id)
        .cloned()
        .unwrap_or_else(|| idle_status(song_id)))
}

pub fn reserve_running_status(
    statuses: &Arc<Mutex<HashMap<String, SeparationStatusSnapshot>>>,
    song_id: &str,
    allow_existing_running: bool,
) -> CommandResult<SeparationStatusSnapshot> {
    let mut statuses = statuses
        .lock()
        .map_err(|_| state_lock_error("separation status lock was poisoned"))?;
    if allow_existing_running {
        if let Some(existing) = statuses.get(song_id) {
            if existing.state == SeparationState::Running {
                return Ok(existing.clone());
            }
        }
    }
    let status = running_status(song_id, 0);
    statuses.insert(song_id.to_owned(), status.clone());
    Ok(status)
}

pub fn store_status(
    statuses: &Arc<Mutex<HashMap<String, SeparationStatusSnapshot>>>,
    song_id: &str,
    status: SeparationStatusSnapshot,
) {
    if let Ok(mut statuses) = statuses.lock() {
        statuses.insert(song_id.to_owned(), status);
    }
}

pub fn status_from_job_result(
    song_id: &str,
    result: Result<SeparationArtifacts, CommandError>,
) -> SeparationStatusSnapshot {
    match result {
        Ok(artifacts) => completed_status(
            song_id,
            artifacts.vocals_path,
            artifacts.accomp_path,
            artifacts.cache_hit,
            artifacts.drums_path,
            artifacts.bass_path,
            artifacts.other_path,
        ),
        Err(error) => failed_status(song_id, error),
    }
}

pub fn report_progress_to_status_and_events<R: Runtime>(
    app_handle: &AppHandle<R>,
    statuses: &Arc<Mutex<HashMap<String, SeparationStatusSnapshot>>>,
    song_id: &str,
    percent: u8,
) {
    let snapshot = running_status(song_id, percent);
    store_status(statuses, song_id, snapshot);
    let _ = app_handle.emit(
        SEPARATION_PROGRESS_EVENT,
        SeparationProgressEvent {
            song_id: song_id.to_owned(),
            percent,
        },
    );
}

/// Store terminal status, emit complete/error events, and invoke publish on success.
pub fn emit_terminal_status<R, P>(
    app_handle: &AppHandle<R>,
    statuses: &Arc<Mutex<HashMap<String, SeparationStatusSnapshot>>>,
    status: SeparationStatusSnapshot,
    publish: P,
) where
    R: Runtime,
    P: FnOnce(&str),
{
    let song_id = status.song_id.clone();
    let error = status.error.clone();
    let state = status.state.clone();
    store_status(statuses, &song_id, status.clone());

    match state {
        SeparationState::Completed => {
            let _ = app_handle.emit(
                SEPARATION_COMPLETE_EVENT,
                SeparationCompleteEvent {
                    song_id: song_id.clone(),
                    status: status.clone(),
                },
            );
            publish(&song_id);
        }
        SeparationState::Failed => {
            if let Some(error) = error {
                let _ = app_handle.emit(
                    SEPARATION_ERROR_EVENT,
                    SeparationErrorEvent { song_id, error },
                );
            }
        }
        SeparationState::Idle | SeparationState::Running => {}
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

pub fn ensure_song_can_be_separated(state: &AppState, song_id: &str) -> CommandResult<()> {
    let library_root = state.library_root()?;
    let connection = cache::open_database(&library_root.database_path())
        .map_err(|e| SeparationError::Failed(e.to_string()))?;
    let song = cache::get_song_by_hash(&connection, song_id)
        .map_err(|e| SeparationError::Failed(e.to_string()))?
        .ok_or_else(|| SeparationError::SongNotFound(song_id.to_owned()))?;

    validate_song_can_be_separated(&song, song_id)
}

pub fn validate_song_can_be_separated(
    song: &crate::library::Song,
    song_id: &str,
) -> CommandResult<()> {
    if song.is_media_g() {
        // Media+G songs already carry karaoke graphics and intentionally skip
        // the stem pipeline, which is designed for plain audio assets only.
        return Err(SeparationError::Failed(format!(
            "song {song_id} is a Media+G track and cannot be separated"
        ))
        .into());
    }

    if song.is_instrumental() {
        return Err(SeparationError::Failed(format!(
            "song {song_id} is marked instrumental and cannot be separated"
        ))
        .into());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Job runner (shared deep-job boundary)
// ---------------------------------------------------------------------------

/// Open the library DB and run `separate_song_into_cache` with progress callback.
/// Does not touch bootstrap or remote publish — those are caller responsibilities.
pub fn run_job_blocking(
    library_root: &LibraryRoot,
    model_cache: &Arc<Mutex<ModelCache<LoadedModel>>>,
    model_path: &Path,
    song_id: &str,
    stem_mode: StemMode,
    model_variant: &str,
    ep_preference: ExecutionProviderPreference,
    report_progress: impl FnMut(u8),
) -> CommandResult<SeparationArtifacts> {
    let connection = cache::open_database(&library_root.database_path())
        .map_err(|error| SeparationError::Failed(error.to_string()))?;
    separator::job::separate_song_into_cache(
        &connection,
        library_root,
        model_cache,
        model_path,
        song_id,
        stem_mode,
        model_variant,
        ep_preference,
        report_progress,
    )
    .map_err(|error| SeparationError::Failed(error.to_string()).into())
}

// ---------------------------------------------------------------------------
// Single-song orchestration
// ---------------------------------------------------------------------------

/// Spawn a single-song separation worker: bootstrap → job → terminal status + publish.
///
/// Keep command handlers thin and preserve the event/status contract in one
/// place. Separation orchestration is concurrency-sensitive, so duplicated
/// branches here are easy to drift and regress independently.
pub fn start_job<R: Runtime>(
    app_handle: AppHandle<R>,
    execution_context: SeparationExecutionContext,
    song_id: String,
    stem_mode: StemMode,
) {
    let SeparationExecutionContext {
        library_root,
        model_variant,
        ep_preference,
        app_data_dir,
        model_bootstrap_status,
        runtime_bootstrap_status,
        statuses,
        model_cache,
        stem_mode: _,
    } = execution_context;
    let progress_song_id = song_id.clone();
    let progress_app_handle = app_handle.clone();
    let progress_statuses = Arc::clone(&statuses);

    tauri::async_runtime::spawn(async move {
        let worker_library_root = library_root.clone();
        let worker_song_id = song_id.clone();
        let prerequisite_app_handle = app_handle.clone();
        let prerequisite_app_data_dir = app_data_dir.clone();
        let prerequisite_model_status = Arc::clone(&model_bootstrap_status);
        let prerequisite_runtime_status = Arc::clone(&runtime_bootstrap_status);

        let result = tauri::async_runtime::spawn_blocking(move || -> CommandResult<_> {
            let mut emit_runtime = |event, snapshot| {
                let _ = prerequisite_app_handle.emit(event, snapshot);
            };
            let mut emit_model = |event, snapshot| {
                let _ = prerequisite_app_handle.emit(event, snapshot);
            };
            let worker_model_path = ensure_runtime_and_model_blocking(
                &prerequisite_app_data_dir,
                &prerequisite_runtime_status,
                &prerequisite_model_status,
                &mut emit_runtime,
                &mut emit_model,
            )?;

            run_job_blocking(
                &worker_library_root,
                &model_cache,
                &worker_model_path,
                &worker_song_id,
                stem_mode,
                &model_variant,
                ep_preference,
                |percent| {
                    report_progress_to_status_and_events(
                        &progress_app_handle,
                        &progress_statuses,
                        &progress_song_id,
                        percent,
                    );
                },
            )
        })
        .await;

        let final_status = match result {
            Ok(Ok(artifacts)) => status_from_job_result(&song_id, Ok(artifacts)),
            Ok(Err(error)) => status_from_job_result(&song_id, Err(error)),
            Err(error) => status_from_job_result(
                &song_id,
                Err(SeparationError::Failed(error.to_string()).into()),
            ),
        };

        let publish_handle = app_handle.clone();
        emit_terminal_status(&app_handle, &statuses, final_status, |sid| {
            publish_on_complete_default(&publish_handle, sid);
        });
    });
}

// ---------------------------------------------------------------------------
// Batch planning + orchestration
// ---------------------------------------------------------------------------

pub struct BatchPlan {
    pub to_separate: Vec<String>,
    pub skipped: usize,
}

/// Resolve which song hashes still need separation for the given stem mode.
pub fn plan_batch(
    connection: &rusqlite::Connection,
    library_root: &LibraryRoot,
    song_ids: Vec<String>,
    stem_mode: StemMode,
) -> CommandResult<BatchPlan> {
    let hashes: Vec<String> = if song_ids.is_empty() {
        let songs = cache::list_songs(connection).map_err(|e| database_error(e.to_string()))?;
        songs
            .into_iter()
            .filter(|song| song.is_separable())
            .map(|s| s.hash)
            .collect()
    } else {
        song_ids
            .into_iter()
            .filter(|song_id| {
                cache::get_song_by_hash(connection, song_id)
                    .ok()
                    .flatten()
                    .map(|song| song.is_separable())
                    .unwrap_or(false)
            })
            .collect()
    };

    let mut to_separate = Vec::new();
    let mut skipped: usize = 0;
    for hash in &hashes {
        if let Ok(Some(entry)) = cache::stems::get_cached_stem_entry(connection, hash) {
            let already_done = match stem_mode {
                StemMode::TwoStem => true, // any cached entry is sufficient
                StemMode::FourStem => entry.has_individual_stems(),
            };
            if already_done && cache::stems::cache_entry_files_valid(library_root, &entry) {
                skipped += 1;
                continue;
            }
        }
        to_separate.push(hash.clone());
    }

    Ok(BatchPlan {
        to_separate,
        skipped,
    })
}

/// Spawn sequential batch separation (ONNX Runtime is memory-heavy).
pub fn start_batch_job<R: Runtime>(
    app_handle: AppHandle<R>,
    execution_context: SeparationExecutionContext,
    plan: BatchPlan,
    batch_running: Arc<AtomicBool>,
    batch_cancel: Arc<AtomicBool>,
) {
    let SeparationExecutionContext {
        library_root,
        model_variant,
        ep_preference,
        stem_mode,
        app_data_dir,
        model_bootstrap_status,
        runtime_bootstrap_status,
        statuses: separation_statuses,
        model_cache,
    } = execution_context;

    let total = plan.to_separate.len();
    let skipped = plan.skipped;
    let to_separate = plan.to_separate;

    // Mark batch as running.
    batch_running.store(true, Ordering::Relaxed);
    batch_cancel.store(false, Ordering::Relaxed);

    // Emit initial progress.
    let _ = app_handle.emit(
        BATCH_SEPARATION_PROGRESS_EVENT,
        BatchSeparationProgress {
            total,
            completed: 0,
            skipped,
            failed: 0,
            current_song_id: None,
            current_percent: 0,
        },
    );

    tauri::async_runtime::spawn(async move {
        let mut completed: usize = 0;
        let mut failed_count: usize = 0;

        let prerequisite_result = {
            let app_data_dir = app_data_dir.clone();
            let app_handle = app_handle.clone();
            let runtime_status = Arc::clone(&runtime_bootstrap_status);
            let model_status = Arc::clone(&model_bootstrap_status);
            tauri::async_runtime::spawn_blocking(move || {
                let mut emit_runtime = |event, snapshot| {
                    let _ = app_handle.emit(event, snapshot);
                };
                let mut emit_model = |event, snapshot| {
                    let _ = app_handle.emit(event, snapshot);
                };
                ensure_runtime_and_model_blocking(
                    &app_data_dir,
                    &runtime_status,
                    &model_status,
                    &mut emit_runtime,
                    &mut emit_model,
                )
            })
            .await
        };

        let model_path = match prerequisite_result {
            Ok(Ok(path)) => path,
            Ok(Err(error)) => {
                let _ = app_handle.emit(
                    BATCH_SEPARATION_COMPLETE_EVENT,
                    BatchSeparationProgress {
                        total,
                        completed,
                        skipped,
                        failed: total.saturating_sub(completed + skipped),
                        current_song_id: None,
                        current_percent: 0,
                    },
                );
                batch_running.store(false, Ordering::Relaxed);
                eprintln!("batch separation prerequisites failed: {}", error.message);
                return;
            }
            Err(error) => {
                let _ = app_handle.emit(
                    BATCH_SEPARATION_COMPLETE_EVENT,
                    BatchSeparationProgress {
                        total,
                        completed,
                        skipped,
                        failed: total.saturating_sub(completed + skipped),
                        current_song_id: None,
                        current_percent: 0,
                    },
                );
                batch_running.store(false, Ordering::Relaxed);
                eprintln!("batch separation prerequisites task failed: {error}");
                return;
            }
        };

        for song_id in &to_separate {
            // Check cancellation.
            if batch_cancel.load(Ordering::Relaxed) {
                let _ = app_handle.emit(
                    BATCH_SEPARATION_CANCELLED_EVENT,
                    BatchSeparationProgress {
                        total,
                        completed,
                        skipped,
                        failed: failed_count,
                        current_song_id: None,
                        current_percent: 0,
                    },
                );
                batch_running.store(false, Ordering::Relaxed);
                return;
            }

            // Mark song as running.
            {
                if let Ok(mut statuses) = separation_statuses.lock() {
                    statuses.insert(song_id.clone(), running_status(song_id, 0));
                }
            }

            // Emit batch progress with current song.
            let _ = app_handle.emit(
                BATCH_SEPARATION_PROGRESS_EVENT,
                BatchSeparationProgress {
                    total,
                    completed,
                    skipped,
                    failed: failed_count,
                    current_song_id: Some(song_id.clone()),
                    current_percent: 0,
                },
            );

            let worker_library_root = library_root.clone();
            let worker_model_path = model_path.clone();
            let worker_song_id = song_id.clone();
            let worker_statuses = Arc::clone(&separation_statuses);
            let worker_model_cache = Arc::clone(&model_cache);
            let progress_song_id = song_id.clone();
            let progress_app_handle = app_handle.clone();
            let batch_progress_app_handle = app_handle.clone();
            let batch_total = total;
            let batch_completed = completed;
            let batch_skipped = skipped;
            let batch_failed = failed_count;

            let worker_model_variant = model_variant.clone();
            let result = tauri::async_runtime::spawn_blocking(move || {
                run_job_blocking(
                    &worker_library_root,
                    &worker_model_cache,
                    &worker_model_path,
                    &worker_song_id,
                    stem_mode,
                    &worker_model_variant,
                    ep_preference,
                    |percent| {
                        report_progress_to_status_and_events(
                            &progress_app_handle,
                            &worker_statuses,
                            &progress_song_id,
                            percent,
                        );
                        // Also emit batch progress update with per-song percent.
                        let _ = batch_progress_app_handle.emit(
                            BATCH_SEPARATION_PROGRESS_EVENT,
                            BatchSeparationProgress {
                                total: batch_total,
                                completed: batch_completed,
                                skipped: batch_skipped,
                                failed: batch_failed,
                                current_song_id: Some(progress_song_id.clone()),
                                current_percent: percent,
                            },
                        );
                    },
                )
            })
            .await;

            match result {
                Ok(Ok(artifacts)) => {
                    let status = status_from_job_result(song_id, Ok(artifacts));
                    let publish_handle = app_handle.clone();
                    emit_terminal_status(&app_handle, &separation_statuses, status, |sid| {
                        publish_on_complete_default(&publish_handle, sid);
                    });
                    completed += 1;
                }
                Ok(Err(error)) => {
                    let status = status_from_job_result(song_id, Err(error));
                    emit_terminal_status(&app_handle, &separation_statuses, status, |_| {});
                    failed_count += 1;
                }
                Err(error) => {
                    let cmd_error: CommandError = SeparationError::Failed(error.to_string()).into();
                    let status = status_from_job_result(song_id, Err(cmd_error));
                    emit_terminal_status(&app_handle, &separation_statuses, status, |_| {});
                    failed_count += 1;
                }
            }
        }

        // Batch complete.
        let _ = app_handle.emit(
            BATCH_SEPARATION_COMPLETE_EVENT,
            BatchSeparationProgress {
                total,
                completed,
                skipped,
                failed: failed_count,
                current_song_id: None,
                current_percent: 0,
            },
        );
        batch_running.store(false, Ordering::Relaxed);
    });
}

// ---------------------------------------------------------------------------
// Synchronous command helpers (status query / downgrade / cache clear)
// ---------------------------------------------------------------------------

pub fn get_all_separation_statuses(
    state: &AppState,
) -> CommandResult<Vec<SeparationStatusSnapshot>> {
    let library_root = state.library_root()?;
    let connection = cache::open_database(&library_root.database_path())
        .map_err(|e| database_error(e.to_string()))?;

    let entries = cache::stems::list_all_stem_entries(&connection)
        .map_err(|e| database_error(e.to_string()))?;

    // Also populate the in-memory separation_statuses map so that
    // subsequent get_separation_status calls return the correct state.
    let mut statuses_lock = state
        .separation
        .separation_statuses
        .lock()
        .map_err(|_| state_lock_error("separation status lock was poisoned"))?;

    let mut result = Vec::with_capacity(entries.len());
    for entry in entries {
        // Only report entries whose files still exist on disk
        if cache::stems::cache_entry_files_valid(&library_root, &entry) {
            let status = completed_status_with_model(
                &entry.song_hash,
                &entry.vocals_path,
                &entry.accomp_path,
                true,
                entry.drums_path.clone(),
                entry.bass_path.clone(),
                entry.other_path.clone(),
                entry.model_variant.clone(),
            );
            statuses_lock.insert(entry.song_hash.clone(), status.clone());
            result.push(status);
        }
    }

    Ok(result)
}

pub fn clear_stem_cache_for_song(state: &AppState, song_id: &str) -> CommandResult<()> {
    let library_root = state.library_root()?;
    let connection = cache::open_database(&library_root.database_path())
        .map_err(|e| SeparationError::Failed(e.to_string()))?;
    let _ = cache::stems::delete_stem_cache_entry(&connection, &library_root, song_id);
    Ok(())
}

pub fn clear_in_memory_status(state: &AppState, song_id: &str) -> CommandResult<()> {
    let mut statuses = state
        .separation
        .separation_statuses
        .lock()
        .map_err(|_| state_lock_error("separation status lock was poisoned"))?;
    statuses.remove(song_id);
    Ok(())
}

pub fn try_completed_four_stem_status(
    state: &AppState,
    song_id: &str,
) -> CommandResult<Option<SeparationStatusSnapshot>> {
    let library_root = state.library_root()?;
    let connection = cache::open_database(&library_root.database_path())
        .map_err(|e| SeparationError::Failed(e.to_string()))?;
    if let Ok(Some(entry)) = cache::stems::get_cached_stem_entry(&connection, song_id) {
        if entry.has_individual_stems() {
            return Ok(Some(completed_status(
                song_id,
                entry.vocals_path,
                entry.accomp_path,
                true,
                entry.drums_path,
                entry.bass_path,
                entry.other_path,
            )));
        }
    }
    Ok(None)
}

pub fn downgrade_to_two_stem_and_publish<R: Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_id: &str,
) -> CommandResult<SeparationStatusSnapshot> {
    let library_root = state.library_root()?;
    let connection = cache::open_database(&library_root.database_path())
        .map_err(|e| SeparationError::Failed(e.to_string()))?;

    let (updated_entry, _freed_bytes) =
        cache::stems::downgrade_to_two_stem(&connection, &library_root, song_id)
            .map_err(|e| SeparationError::Failed(e.to_string()))?;

    let completed = completed_status(
        song_id,
        &updated_entry.vocals_path,
        &updated_entry.accomp_path,
        true,
        updated_entry.drums_path,
        updated_entry.bass_path,
        updated_entry.other_path,
    );

    // Update in-memory separation statuses.
    {
        let mut statuses = state
            .separation
            .separation_statuses
            .lock()
            .map_err(|_| state_lock_error("separation status lock was poisoned"))?;
        statuses.insert(song_id.to_owned(), completed.clone());
    }

    // Emit completion event so the frontend updates.
    let _ = app_handle.emit(
        SEPARATION_COMPLETE_EVENT,
        SeparationCompleteEvent {
            song_id: song_id.to_owned(),
            status: completed.clone(),
        },
    );
    remote::publish_song_to_active_remote_if_ready(state, app_handle, song_id)?;

    Ok(completed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lookup_defaults_to_idle_when_song_has_not_started_separation() {
        let statuses = Arc::new(Mutex::new(HashMap::new()));

        let status = get_separation_status_from_map(&statuses, "missing-song")
            .expect("idle lookup should succeed");

        assert_eq!(status, idle_status("missing-song"));
    }

    #[test]
    fn reserve_running_status_reuses_existing_running_entry_when_allowed() {
        let statuses = Arc::new(Mutex::new(HashMap::from([(
            "song-1".to_owned(),
            running_status("song-1", 42),
        )])));

        let status = reserve_running_status(&statuses, "song-1", true)
            .expect("running status reservation should succeed");

        assert_eq!(status, running_status("song-1", 42));
        assert_eq!(
            statuses
                .lock()
                .expect("status map lock should succeed")
                .get("song-1")
                .cloned(),
            Some(running_status("song-1", 42))
        );
    }

    #[test]
    fn status_from_job_result_maps_success_to_completed_status() {
        let status = status_from_job_result(
            "song-1",
            Ok(SeparationArtifacts {
                vocals_path: "vocals.ogg".to_owned(),
                accomp_path: "accomp.ogg".to_owned(),
                cache_hit: true,
                drums_path: Some("drums.ogg".to_owned()),
                bass_path: Some("bass.ogg".to_owned()),
                other_path: Some("other.ogg".to_owned()),
            }),
        );

        assert_eq!(
            status,
            completed_status(
                "song-1",
                "vocals.ogg",
                "accomp.ogg",
                true,
                Some("drums.ogg".to_owned()),
                Some("bass.ogg".to_owned()),
                Some("other.ogg".to_owned()),
            )
        );
    }

    #[test]
    fn status_from_job_result_maps_errors_to_failed_status() {
        let error: CommandError = SeparationError::Failed("boom".to_owned()).into();

        let status = status_from_job_result("song-1", Err(error.clone()));

        assert_eq!(status, failed_status("song-1", error));
    }

    #[test]
    fn validate_song_can_be_separated_rejects_instrumental_songs() {
        let song = crate::library::Song {
            hash: "song-1".to_owned(),
            file_path: Some("media/song-1.mp3".to_owned()),
            cdg_path: None,
            media_g_container: None,
            instrumental: true,
            language: None,
            audio_source_kind: "original".to_owned(),
            title: Some("Song".to_owned()),
            artist: None,
            album: None,
            duration_ms: 1_000,
            cover_art: None,
            has_cover_art: false,
            imported_at: 1,
            original_ext: Some("mp3".to_owned()),
        };

        let error = validate_song_can_be_separated(&song, "song-1")
            .expect_err("instrumental songs should be rejected");

        assert!(error.message.contains("marked instrumental"));
    }

    #[test]
    fn store_status_overwrites_prior_entry() {
        let statuses = Arc::new(Mutex::new(HashMap::from([(
            "song-1".to_owned(),
            running_status("song-1", 10),
        )])));

        store_status(
            &statuses,
            "song-1",
            completed_status("song-1", "v.ogg", "a.ogg", true, None, None, None),
        );

        let stored = statuses
            .lock()
            .expect("status map lock should succeed")
            .get("song-1")
            .cloned()
            .expect("status should exist");
        assert_eq!(stored.state, SeparationState::Completed);
        assert!(stored.cache_hit);
    }
}
