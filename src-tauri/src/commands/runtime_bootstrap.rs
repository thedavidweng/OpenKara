use crate::{
    commands::error::{
        internal_error, model_bootstrap_error, runtime_post_download_timeout, CommandError,
        CommandResult,
    },
    separator::catalog::{self, VerifiedCatalog},
    separator::runtime_bootstrap::{self, RuntimeInventory},
    AppState,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, State};

pub const RUNTIME_BOOTSTRAP_PROGRESS_EVENT: &str = "runtime-bootstrap-progress";

/// Process-wide flag preventing concurrent `download_runtime` invocations
/// from racing on the shared artifact directory and slot file.
static RUNTIME_DOWNLOAD_IN_PROGRESS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub const RUNTIME_BOOTSTRAP_READY_EVENT: &str = "runtime-bootstrap-ready";
pub const RUNTIME_BOOTSTRAP_ERROR_EVENT: &str = "runtime-bootstrap-error";

pub const LEGACY_RUNTIME_VERSION: &str = "legacy";
const CPU_FALLBACK_NOTICE: &str = "cpu-runtime-fallback-after-directml-timeout";

const RUNTIME_PARENT_LOAD_TIMEOUT: Duration = Duration::from_secs(120);
const RUNTIME_PARENT_LOAD_IN_PROGRESS_MARKER: &str = "runtime_parent_load_in_progress";
static RUNTIME_PARENT_LOAD_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static RUNTIME_PARENT_LOAD_TIMED_OUT: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBootstrapState {
    Missing,
    Downloading,
    Installing,
    Probing,
    Activating,
    Ready,
    UpdateAvailable,
    DownloadingCandidate,
    CandidateReadyRestartRequired,
    ActivationFailedPreviousRestored,
    Corrupt,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeBootstrapStatusSnapshot {
    pub state: RuntimeBootstrapState,
    pub runtime_path: String,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub version: String,
    pub active_artifact_id: Option<String>,
    pub target_triple: String,
    pub candidate_version: Option<String>,
    pub restart_required: bool,
    pub error: Option<CommandError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_fallback_notice: Option<String>,
}

fn pinned_runtime_version() -> String {
    let catalog = catalog::embedded_catalog();
    catalog::resolve_runtime(
        &catalog.manifest,
        catalog::current_target_triple(),
        crate::config::ExecutionProviderPreference::default_for_current_platform(),
    )
    .map(|runtime| runtime.runtime.version.clone())
    .unwrap_or_else(|_| "unknown".to_owned())
}

pub fn snapshot_from_inventory(inventory: &RuntimeInventory) -> RuntimeBootstrapStatusSnapshot {
    let (state, runtime_path, version, active_artifact_id, error) =
        match (&inventory.active, &inventory.legacy_path) {
            (Some(active), _) => {
                let state = if inventory.last_failure.is_some() {
                    RuntimeBootstrapState::ActivationFailedPreviousRestored
                } else {
                    RuntimeBootstrapState::Ready
                };
                (
                    state,
                    active.library_path.display().to_string(),
                    active.record.upstream_version.clone(),
                    Some(active.record.artifact_id.clone()),
                    inventory
                        .last_failure
                        .as_ref()
                        .map(|failure| bootstrap_command_error_from_message(&failure.error)),
                )
            }
            (None, Some(legacy_path)) => (
                RuntimeBootstrapState::Ready,
                legacy_path.display().to_string(),
                LEGACY_RUNTIME_VERSION.to_owned(),
                None,
                None,
            ),
            (None, None) => {
                let (state, error) = match &inventory.last_failure {
                    Some(failure) => (
                        RuntimeBootstrapState::Failed,
                        Some(bootstrap_command_error_from_message(&failure.error)),
                    ),
                    None => (RuntimeBootstrapState::Missing, None),
                };
                (state, String::new(), pinned_runtime_version(), None, error)
            }
        };

    let candidate_version = inventory
        .candidate
        .as_ref()
        .map(|candidate| candidate.record.upstream_version.clone());
    let restart_required = inventory.candidate.is_some();
    let state = if restart_required {
        RuntimeBootstrapState::CandidateReadyRestartRequired
    } else {
        state
    };

    let cpu_fallback_notice =
        cpu_fallback_notice_for(&catalog::embedded_catalog().manifest, &active_artifact_id);
    RuntimeBootstrapStatusSnapshot {
        state,
        runtime_path,
        downloaded_bytes: None,
        total_bytes: None,
        version,
        active_artifact_id,
        target_triple: catalog::current_target_triple().to_owned(),
        candidate_version,
        restart_required,
        error,
        cpu_fallback_notice,
    }
}

fn cpu_fallback_notice_for(
    manifest: &catalog::ReleaseManifest,
    active_artifact_id: &Option<String>,
) -> Option<String> {
    if !crate::platform_capabilities::directml_disabled_by_timeout() {
        return None;
    }
    let artifact_id = active_artifact_id.as_ref()?;
    let runtime = catalog::runtime_by_artifact_id(manifest, artifact_id)?;
    let is_cpu_only = runtime
        .runtime
        .execution_providers
        .iter()
        .all(|provider| provider.eq_ignore_ascii_case("cpu"));
    if is_cpu_only {
        Some(CPU_FALLBACK_NOTICE.to_owned())
    } else {
        None
    }
}

fn bootstrap_command_error_from_message(message: &str) -> CommandError {
    if message.starts_with(crate::commands::runtime_worker::RUNTIME_POST_DOWNLOAD_TIMEOUT_MARKER) {
        runtime_post_download_timeout(message)
    } else {
        model_bootstrap_error(message)
    }
}

pub fn snapshot_from_disk(app_data_dir: &Path) -> RuntimeBootstrapStatusSnapshot {
    snapshot_from_inventory(&runtime_bootstrap::runtime_inventory(app_data_dir))
}

#[tauri::command]
pub fn get_runtime_bootstrap_status(
    state: State<'_, AppState>,
) -> CommandResult<RuntimeBootstrapStatusSnapshot> {
    get_runtime_bootstrap_status_from_state(&state.shell.runtime_bootstrap_status)
}

pub fn get_runtime_bootstrap_status_from_state(
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
) -> CommandResult<RuntimeBootstrapStatusSnapshot> {
    status.lock().map(|snapshot| snapshot.clone()).map_err(|_| {
        crate::commands::error::state_lock_error("runtime bootstrap status lock was poisoned")
    })
}

pub fn ensure_runtime_ready(
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
) -> CommandResult<()> {
    let snapshot = get_runtime_bootstrap_status_from_state(status)?;

    match snapshot.state {
        RuntimeBootstrapState::Ready
        | RuntimeBootstrapState::UpdateAvailable
        | RuntimeBootstrapState::DownloadingCandidate
        | RuntimeBootstrapState::CandidateReadyRestartRequired
        | RuntimeBootstrapState::ActivationFailedPreviousRestored => Ok(()),
        RuntimeBootstrapState::Missing => Err(model_bootstrap_error(
            "ONNX Runtime is not installed; install it from Settings".to_owned(),
        )),
        RuntimeBootstrapState::Downloading
        | RuntimeBootstrapState::Installing
        | RuntimeBootstrapState::Probing
        | RuntimeBootstrapState::Activating => {
            let description = match snapshot.state {
                RuntimeBootstrapState::Downloading => "downloading",
                RuntimeBootstrapState::Installing => "installing",
                RuntimeBootstrapState::Probing => "checking compatibility",
                RuntimeBootstrapState::Activating => "activating",
                _ => unreachable!(),
            };
            Err(model_bootstrap_error(format!(
                "ONNX Runtime is still {} at {}",
                description, snapshot.runtime_path
            )))
        }
        RuntimeBootstrapState::Corrupt => Err(model_bootstrap_error(format!(
            "ONNX Runtime at {} is corrupt; delete and re-download",
            snapshot.runtime_path
        ))),
        RuntimeBootstrapState::Failed => Err(snapshot.error.unwrap_or_else(|| {
            model_bootstrap_error(format!(
                "ONNX Runtime bootstrap failed for {}",
                snapshot.runtime_path
            ))
        })),
    }
}

fn downloading_snapshot(
    base: &RuntimeBootstrapStatusSnapshot,
    state: RuntimeBootstrapState,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) -> RuntimeBootstrapStatusSnapshot {
    RuntimeBootstrapStatusSnapshot {
        state,
        downloaded_bytes: Some(downloaded_bytes),
        total_bytes,
        error: None,
        ..base.clone()
    }
}

fn store_snapshot(
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    snapshot: RuntimeBootstrapStatusSnapshot,
) {
    if let Ok(mut current) = status.lock() {
        *current = snapshot;
    }
}

/// Store the in-flight download snapshot AND emit it as a progress event so
/// the UI's runtime-download task tracks bytes live. Mirrors the model
/// bootstrap progress pattern (`MODEL_BOOTSTRAP_PROGRESS_EVENT`): without the
/// emit, the separation-triggered first install only ever published its
/// initial 0% snapshot, so the progress bar showed "0%" with no labeled task
/// and looked frozen while the runtime downloaded (#226).
fn report_download_progress(
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
    base: &RuntimeBootstrapStatusSnapshot,
    state: RuntimeBootstrapState,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) {
    let snapshot = downloading_snapshot(base, state, downloaded_bytes, total_bytes);
    store_snapshot(status, snapshot.clone());
    emit(RUNTIME_BOOTSTRAP_PROGRESS_EVENT, snapshot);
}

fn report_post_download_progress(
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
    base: &RuntimeBootstrapStatusSnapshot,
    state: RuntimeBootstrapState,
) {
    let snapshot = RuntimeBootstrapStatusSnapshot {
        state,
        downloaded_bytes: None,
        total_bytes: None,
        error: None,
        ..base.clone()
    };
    store_snapshot(status, snapshot.clone());
    emit(RUNTIME_BOOTSTRAP_PROGRESS_EVENT, snapshot);
}

fn report_worker_progress(
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
    base: &RuntimeBootstrapStatusSnapshot,
    progress: crate::commands::runtime_worker::RuntimeWorkerProgress,
    download_state: RuntimeBootstrapState,
) {
    use crate::commands::runtime_worker::RuntimeWorkerPhase;

    match progress.phase {
        RuntimeWorkerPhase::Downloading => report_download_progress(
            status,
            emit,
            base,
            download_state,
            progress.downloaded_bytes,
            progress.total_bytes,
        ),
        RuntimeWorkerPhase::Installing => {
            report_post_download_progress(status, emit, base, RuntimeBootstrapState::Installing)
        }
        RuntimeWorkerPhase::Probing => {
            report_post_download_progress(status, emit, base, RuntimeBootstrapState::Probing)
        }
        RuntimeWorkerPhase::Activating => {
            report_post_download_progress(status, emit, base, RuntimeBootstrapState::Activating)
        }
    }
}

fn runtime_parent_load_timeout_message() -> String {
    format!(
        "{}: ONNX Runtime load did not finish within {} seconds\n\n{}",
        crate::commands::runtime_worker::RUNTIME_POST_DOWNLOAD_TIMEOUT_MARKER,
        RUNTIME_PARENT_LOAD_TIMEOUT.as_secs(),
        crate::commands::runtime_worker::RUNTIME_POST_DOWNLOAD_TIMEOUT_HINT,
    )
}

/// Load ORT in the application process without allowing a native loader hang
/// to block the command executor forever. A timed-out load poisons this
/// process; a restart is required before another attempt can use ORT.
pub(crate) fn ensure_runtime_loaded_with_watchdog(path: &Path) -> anyhow::Result<()> {
    if RUNTIME_PARENT_LOAD_TIMED_OUT.load(Ordering::SeqCst) {
        anyhow::bail!(
            "{}: ONNX Runtime load already timed out; restart OpenKara before retrying",
            crate::commands::runtime_worker::RUNTIME_POST_DOWNLOAD_TIMEOUT_MARKER
        );
    }

    if let Some(loaded) = crate::separator::model::loaded_runtime_path() {
        anyhow::ensure!(
            loaded == path,
            "a different ONNX Runtime is already loaded from {}; restart to use {}",
            loaded.display(),
            path.display()
        );
        return Ok(());
    }

    if RUNTIME_PARENT_LOAD_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        anyhow::bail!(
            "{}: ONNX Runtime load is already in progress; restart OpenKara before retrying",
            RUNTIME_PARENT_LOAD_IN_PROGRESS_MARKER
        );
    }

    let (sender, receiver) = mpsc::sync_channel(1);
    let runtime_path = path.to_path_buf();
    thread::spawn(move || {
        let result = crate::separator::model::ensure_runtime_loaded_from_path(&runtime_path)
            .map(|_| ())
            .map_err(|error| error.to_string());
        RUNTIME_PARENT_LOAD_IN_PROGRESS.store(false, Ordering::SeqCst);
        let _ = sender.send(result);
    });

    match receiver.recv_timeout(RUNTIME_PARENT_LOAD_TIMEOUT) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(anyhow::anyhow!(error)),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            RUNTIME_PARENT_LOAD_TIMED_OUT.store(true, Ordering::SeqCst);
            anyhow::bail!(runtime_parent_load_timeout_message())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            anyhow::bail!("ONNX Runtime load watchdog exited before reporting a result")
        }
    }
}

fn download_runtime_source(
    catalog_cache: &Arc<Mutex<Option<VerifiedCatalog>>>,
) -> CommandResult<VerifiedCatalog> {
    let embedded = catalog::embedded_catalog();
    let cache = catalog_cache
        .lock()
        .map_err(|_| crate::commands::error::state_lock_error("catalog cache lock was poisoned"))?;
    let catalog = match cache.as_ref() {
        Some(cached) if cached.generation > embedded.generation => cached.clone(),
        _ => embedded.clone(),
    };
    Ok(catalog)
}

pub(crate) fn prepare_runtime_download(
    app_data_dir: &Path,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
) -> bool {
    let inventory = runtime_bootstrap::runtime_inventory(app_data_dir);
    // Loaded runtime is process-final → update (candidate + restart) flow.
    let is_update = inventory.active.is_some()
        || inventory.legacy_path.is_some()
        || crate::separator::model::loaded_runtime_path().is_some();

    let base = snapshot_from_disk(app_data_dir);
    let initial_state = if is_update {
        RuntimeBootstrapState::DownloadingCandidate
    } else {
        RuntimeBootstrapState::Downloading
    };
    let initial = downloading_snapshot(&base, initial_state, 0, None);
    store_snapshot(status, initial.clone());
    emit(RUNTIME_BOOTSTRAP_PROGRESS_EVENT, initial);
    is_update
}

pub fn install_and_load_runtime_blocking(
    app_data_dir: &Path,
    catalog: &VerifiedCatalog,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
) -> anyhow::Result<PathBuf> {
    let runtime = catalog::resolve_runtime(
        &catalog.manifest,
        catalog::current_target_triple(),
        crate::config::effective_execution_provider_from_dir(app_data_dir),
    )?;

    if let Some(path) =
        try_activate_staged_runtime(app_data_dir, &runtime.artifact_id, status, emit)?
    {
        return Ok(path);
    }

    let base = snapshot_from_disk(app_data_dir);

    let installed = crate::commands::runtime_worker::install_runtime_with_worker(
        app_data_dir,
        catalog,
        runtime,
        |progress| {
            report_worker_progress(
                status,
                emit,
                &base,
                progress,
                RuntimeBootstrapState::Downloading,
            );
        },
    )?;

    if let Err(err) = ensure_runtime_loaded_with_watchdog(&installed.library_path) {
        let _ = crate::config::record_directml_unavailable_on_timeout(
            app_data_dir,
            &runtime.runtime.execution_providers,
            &err.to_string(),
        );
        return Err(err);
    }
    runtime_bootstrap::activate_first_install(app_data_dir, &installed.record.artifact_id)?;

    let snapshot = snapshot_from_disk(app_data_dir);
    store_snapshot(status, snapshot.clone());
    emit(RUNTIME_BOOTSTRAP_READY_EVENT, snapshot);
    Ok(installed.library_path)
}

fn try_activate_staged_runtime(
    app_data_dir: &Path,
    artifact_id: &str,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
) -> anyhow::Result<Option<PathBuf>> {
    let inventory = runtime_bootstrap::runtime_inventory(app_data_dir);
    if inventory.active.is_some() {
        return Ok(None);
    }

    let installed = match inventory.candidate {
        Some(candidate) if candidate.record.artifact_id == artifact_id => candidate,
        _ => match runtime_bootstrap::installed_runtime(app_data_dir, artifact_id) {
            Some(existing) if runtime_bootstrap::verify_runtime_files(&existing)? => existing,
            _ => return Ok(None),
        },
    };

    if !runtime_bootstrap::verify_runtime_files(&installed)? {
        return Ok(None);
    }

    let base = snapshot_from_disk(app_data_dir);
    report_post_download_progress(status, emit, &base, RuntimeBootstrapState::Probing);
    if let Err(err) = ensure_runtime_loaded_with_watchdog(&installed.library_path) {
        if let Some(runtime) = catalog::runtime_by_artifact_id(
            &catalog::embedded_catalog().manifest,
            &installed.record.artifact_id,
        ) {
            let _ = crate::config::record_directml_unavailable_on_timeout(
                app_data_dir,
                &runtime.runtime.execution_providers,
                &err.to_string(),
            );
        }
        return Err(err);
    }
    report_post_download_progress(status, emit, &base, RuntimeBootstrapState::Activating);
    runtime_bootstrap::activate_first_install(app_data_dir, &installed.record.artifact_id)?;

    let snapshot = snapshot_from_disk(app_data_dir);
    store_snapshot(status, snapshot.clone());
    emit(RUNTIME_BOOTSTRAP_READY_EVENT, snapshot);
    Ok(Some(installed.library_path))
}

pub fn download_and_stage_candidate_blocking(
    app_data_dir: &Path,
    catalog: &VerifiedCatalog,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
) -> anyhow::Result<()> {
    let runtime = catalog::resolve_runtime(
        &catalog.manifest,
        catalog::current_target_triple(),
        crate::config::effective_execution_provider_from_dir(app_data_dir),
    )?;

    // Fail closed if the staged runtime cannot serve every installed model variant.
    for variant in [
        crate::config::ModelVariant::Htdemucs,
        crate::config::ModelVariant::HtdemucsFt,
    ] {
        let catalog_model = catalog::resolve_model(&catalog.manifest, variant)?;
        if !runtime
            .runtime
            .supported_model_artifact_ids
            .contains(&catalog_model.artifact_id)
        {
            anyhow::bail!(
                "runtime {} does not support model {}; refusing to stage an incompatible candidate",
                runtime.artifact_id,
                catalog_model.artifact_id
            );
        }
    }

    let base = snapshot_from_disk(app_data_dir);

    crate::commands::runtime_worker::install_runtime_with_worker(
        app_data_dir,
        catalog,
        runtime,
        |progress| {
            report_worker_progress(
                status,
                emit,
                &base,
                progress,
                RuntimeBootstrapState::DownloadingCandidate,
            );
        },
    )?;

    let snapshot = snapshot_from_disk(app_data_dir);
    store_snapshot(status, snapshot.clone());
    emit(RUNTIME_BOOTSTRAP_READY_EVENT, snapshot);
    Ok(())
}

pub fn ensure_runtime_ready_or_install_blocking(
    app_data_dir: &Path,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
) -> CommandResult<PathBuf> {
    // Process-committed runtime wins over slot inventory (mapped until restart).
    if let Some(loaded) = crate::separator::model::loaded_runtime_path() {
        return Ok(loaded.to_path_buf());
    }

    let snapshot = get_runtime_bootstrap_status_from_state(status)?;
    let inventory = runtime_bootstrap::runtime_inventory(app_data_dir);

    // Recover a staged candidate left by a killed worker; do not re-download.
    if inventory.active.is_none() && inventory.candidate.is_some() {
        if let Some(plan) = runtime_bootstrap::begin_startup(app_data_dir)
            .map_err(|error| model_bootstrap_error(error.to_string()))?
        {
            let failed_id = plan
                .record
                .as_ref()
                .map(|record| record.artifact_id.clone())
                .unwrap_or_default();
            if let Err(error) = ensure_runtime_loaded_with_watchdog(&plan.library_path) {
                if plan.proving_candidate && !failed_id.is_empty() {
                    let _ = runtime_bootstrap::rollback_failed_activation(
                        app_data_dir,
                        &failed_id,
                        &error.to_string(),
                    );
                }
                let command_error = bootstrap_command_error(error);
                record_runtime_failure(app_data_dir, status, emit, command_error.clone());
                return Err(command_error);
            }
            if plan.proving_candidate {
                runtime_bootstrap::finish_activation_success(app_data_dir)
                    .map_err(|error| model_bootstrap_error(error.to_string()))?;
            }
            let ready = snapshot_from_disk(app_data_dir);
            store_snapshot(status, ready.clone());
            emit(RUNTIME_BOOTSTRAP_READY_EVENT, ready);
            return Ok(plan.library_path);
        }
    }

    let has_loadable = inventory.active.is_some() || inventory.legacy_path.is_some();
    if matches!(
        snapshot.state,
        RuntimeBootstrapState::Downloading | RuntimeBootstrapState::DownloadingCandidate
    ) && !has_loadable
    {
        return Err(model_bootstrap_error(format!(
            "ONNX Runtime is still downloading to {}",
            snapshot.runtime_path
        )));
    }

    if let Some(active) = inventory.active {
        if let Err(error) = ensure_runtime_loaded_with_watchdog(&active.library_path) {
            let command_error = bootstrap_command_error(error);
            record_runtime_failure(app_data_dir, status, emit, command_error.clone());
            return Err(command_error);
        }
        let ready = snapshot_from_disk(app_data_dir);
        store_snapshot(status, ready.clone());
        emit(RUNTIME_BOOTSTRAP_READY_EVENT, ready);
        return Ok(active.library_path);
    }
    if let Some(legacy_path) = inventory.legacy_path {
        if let Err(error) = ensure_runtime_loaded_with_watchdog(&legacy_path) {
            let command_error = bootstrap_command_error(error);
            record_runtime_failure(app_data_dir, status, emit, command_error.clone());
            return Err(command_error);
        }
        let ready = snapshot_from_disk(app_data_dir);
        store_snapshot(status, ready.clone());
        emit(RUNTIME_BOOTSTRAP_READY_EVENT, ready);
        return Ok(legacy_path);
    }

    let catalog = catalog::embedded_catalog();
    ensure_runtime_ready_or_install_with_catalog(app_data_dir, status, catalog, emit)
}

pub fn ensure_runtime_ready_or_install_with_catalog(
    app_data_dir: &Path,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    catalog: &VerifiedCatalog,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
) -> CommandResult<PathBuf> {
    prepare_runtime_download(app_data_dir, status, emit);
    install_and_load_runtime_blocking(app_data_dir, catalog, status, emit).map_err(|error| {
        let command_error = bootstrap_command_error(error);
        record_runtime_failure(app_data_dir, status, emit, command_error.clone());
        command_error
    })
}

pub(crate) fn download_runtime_blocking_with_catalog(
    app_data_dir: &Path,
    catalog: &VerifiedCatalog,
    is_update: bool,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
) -> CommandResult<()> {
    let result = if is_update {
        download_and_stage_candidate_blocking(app_data_dir, catalog, status, emit)
    } else {
        install_and_load_runtime_blocking(app_data_dir, catalog, status, emit).map(|_| ())
    };

    result.map_err(|error| {
        let command_error = bootstrap_command_error(error);
        record_runtime_failure(app_data_dir, status, emit, command_error.clone());
        command_error
    })
}

fn record_runtime_failure(
    app_data_dir: &Path,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
    error: CommandError,
) {
    let mut failed = snapshot_from_disk(app_data_dir);
    failed.state = RuntimeBootstrapState::Failed;
    failed.error = Some(error);
    store_snapshot(status, failed.clone());
    emit(RUNTIME_BOOTSTRAP_ERROR_EVENT, failed);
}

fn bootstrap_command_error(error: anyhow::Error) -> CommandError {
    let message = error.to_string();
    bootstrap_command_error_from_message(&message)
}

#[tauri::command]
pub fn download_runtime(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> CommandResult<RuntimeBootstrapStatusSnapshot> {
    let app_data_dir = state.shell.app_data_dir.clone();
    let status = Arc::clone(&state.shell.runtime_bootstrap_status);
    let catalog = download_runtime_source(&state.shell.catalog_cache)?;

    if RUNTIME_DOWNLOAD_IN_PROGRESS.swap(true, std::sync::atomic::Ordering::SeqCst) {
        // Already running — report current state; do not race a second install.
        return get_runtime_bootstrap_status_from_state(&status);
    }

    let is_update = prepare_runtime_download(&app_data_dir, &status, &mut |event, snapshot| {
        let _ = app_handle.emit(event, snapshot);
    });
    let initial = get_runtime_bootstrap_status_from_state(&status)?;

    tauri::async_runtime::spawn(async move {
        let task_status = Arc::clone(&status);
        let task_app_handle = app_handle.clone();
        let task_app_data_dir = app_data_dir.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let mut emit = |event, snapshot| {
                let _ = task_app_handle.emit(event, snapshot);
            };
            download_runtime_blocking_with_catalog(
                &task_app_data_dir,
                &catalog,
                is_update,
                &task_status,
                &mut emit,
            )
        })
        .await;

        RUNTIME_DOWNLOAD_IN_PROGRESS.store(false, std::sync::atomic::Ordering::SeqCst);

        if let Err(join_error) = result {
            record_runtime_failure(
                &app_data_dir,
                &status,
                &mut |event, snapshot| {
                    let _ = app_handle.emit(event, snapshot);
                },
                internal_error(format!("runtime download task failed: {join_error}")),
            );
        }
    });

    Ok(initial)
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeUpdateReport {
    pub generation: u64,
    pub release_id: String,
    pub target_triple: String,
    pub state: catalog::ModelUpdateState,
    pub installed_version: Option<String>,
    pub available_version: String,
    pub available_bytes: u64,
    pub restart_required: bool,
}

#[tauri::command]
pub async fn check_runtime_updates(
    state: State<'_, AppState>,
) -> CommandResult<RuntimeUpdateReport> {
    let app_data_dir = state.shell.app_data_dir.clone();
    let catalog_cache = Arc::clone(&state.shell.catalog_cache);
    let status = Arc::clone(&state.shell.runtime_bootstrap_status);

    let report =
        tauri::async_runtime::spawn_blocking(move || -> CommandResult<RuntimeUpdateReport> {
            let catalog = catalog::fetch_stable_catalog()
                .map_err(|error| model_bootstrap_error(format!("update check failed: {error}")))?;
            let runtime = catalog::resolve_runtime(
                &catalog.manifest,
                catalog::current_target_triple(),
                crate::config::effective_execution_provider_from_dir(&app_data_dir),
            )
            .map_err(|error| model_bootstrap_error(format!("update check failed: {error}")))?;

            let inventory = runtime_bootstrap::runtime_inventory(&app_data_dir);
            let installed_record = inventory
                .active
                .as_ref()
                .map(|active| active.record.clone());
            let file_exists = inventory.active.is_some() || inventory.legacy_path.is_some();

            let comparison = catalog::compare_installed_artifact(
                installed_record,
                &runtime.artifact_id,
                &runtime.archive_digest,
                &catalog,
                file_exists,
            )
            .map_err(|error| model_bootstrap_error(format!("update check failed: {error}")))?;

            let report = RuntimeUpdateReport {
                generation: catalog.generation,
                release_id: catalog.release_id.clone(),
                target_triple: catalog::current_target_triple().to_owned(),
                state: comparison.state,
                installed_version: comparison
                    .installed
                    .map(|record| record.upstream_version)
                    .or_else(|| {
                        inventory
                            .legacy_path
                            .is_some()
                            .then(|| LEGACY_RUNTIME_VERSION.to_owned())
                    }),
                available_version: runtime.runtime.version.clone(),
                available_bytes: runtime.byte_size,
                restart_required: file_exists,
            };

            if let Ok(mut cache) = catalog_cache.lock() {
                *cache = Some(catalog);
            }

            // Mirror update availability into lifecycle state for UI consumers.
            if matches!(
                report.state,
                catalog::ModelUpdateState::UpdateAvailable
                    | catalog::ModelUpdateState::InstalledWithoutIdentity
            ) {
                if let Ok(mut current) = status.lock() {
                    if current.state == RuntimeBootstrapState::Ready {
                        current.state = RuntimeBootstrapState::UpdateAvailable;
                    }
                }
            }

            Ok(report)
        })
        .await
        .map_err(|error| internal_error(format!("update check task failed: {error}")))??;

    Ok(report)
}

#[tauri::command]
pub fn delete_runtime(state: State<'_, AppState>) -> CommandResult<()> {
    let app_data_dir = state.shell.app_data_dir.clone();
    runtime_bootstrap::delete_runtime(&app_data_dir)
        .map_err(|e| internal_error(format!("failed to delete runtime: {e}")))?;

    let snapshot = snapshot_from_disk(&app_data_dir);
    store_snapshot(&state.shell.runtime_bootstrap_status, snapshot);
    Ok(())
}

pub fn sync_runtime_bootstrap_status(
    app_data_dir: &std::path::Path,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
) -> CommandResult<RuntimeBootstrapStatusSnapshot> {
    let snapshot = snapshot_from_disk(app_data_dir);
    let mut guard = status.lock().map_err(|_| {
        crate::commands::error::state_lock_error("runtime status lock was poisoned")
    })?;
    *guard = snapshot.clone();
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::separator::runtime_bootstrap::RuntimeInventory;

    fn empty_inventory() -> RuntimeInventory {
        RuntimeInventory {
            active: None,
            candidate: None,
            legacy_path: None,
            last_failure: None,
        }
    }

    #[test]
    fn missing_inventory_reports_missing_with_pinned_version() {
        let snapshot = snapshot_from_inventory(&empty_inventory());
        assert_eq!(snapshot.state, RuntimeBootstrapState::Missing);
        assert!(!snapshot.restart_required);
        assert_eq!(snapshot.version, "v1.27.1");
        assert_eq!(
            snapshot.target_triple,
            catalog::current_target_triple().to_owned()
        );
    }

    #[test]
    fn legacy_inventory_reports_ready_with_legacy_version() {
        let mut inventory = empty_inventory();
        inventory.legacy_path = Some(std::path::PathBuf::from("/tmp/runtime/lib"));
        let snapshot = snapshot_from_inventory(&inventory);
        assert_eq!(snapshot.state, RuntimeBootstrapState::Ready);
        assert_eq!(snapshot.version, LEGACY_RUNTIME_VERSION);
        assert_eq!(snapshot.active_artifact_id, None);
    }

    #[test]
    fn failure_without_active_runtime_reports_failed() {
        let mut inventory = empty_inventory();
        inventory.last_failure = Some(crate::separator::runtime_bootstrap::ActivationFailure {
            artifact_id: "rt-x".to_owned(),
            error: "dlopen failed".to_owned(),
            at_unix: 1,
        });
        let snapshot = snapshot_from_inventory(&inventory);
        assert_eq!(snapshot.state, RuntimeBootstrapState::Failed);
        assert!(snapshot.error.is_some());
    }

    #[test]
    fn ensure_runtime_ready_accepts_lifecycle_states_with_active_runtime() {
        for state in [
            RuntimeBootstrapState::Ready,
            RuntimeBootstrapState::UpdateAvailable,
            RuntimeBootstrapState::DownloadingCandidate,
            RuntimeBootstrapState::CandidateReadyRestartRequired,
            RuntimeBootstrapState::ActivationFailedPreviousRestored,
        ] {
            let snapshot = RuntimeBootstrapStatusSnapshot {
                state,
                runtime_path: "/tmp/lib".to_owned(),
                downloaded_bytes: None,
                total_bytes: None,
                version: "v1.27.1".to_owned(),
                active_artifact_id: Some("rt".to_owned()),
                target_triple: catalog::current_target_triple().to_owned(),
                candidate_version: None,
                restart_required: false,
                error: None,
                cpu_fallback_notice: None,
            };
            let status = Arc::new(Mutex::new(snapshot));
            assert!(ensure_runtime_ready(&status).is_ok());
        }
    }

    #[test]
    fn report_download_progress_stores_and_emits_the_snapshot() {
        let base = snapshot_from_inventory(&empty_inventory());
        let status = Arc::new(Mutex::new(base.clone()));
        let mut events: Vec<(&'static str, RuntimeBootstrapStatusSnapshot)> = Vec::new();
        {
            let mut emit = |event, snapshot| events.push((event, snapshot));
            report_download_progress(
                &status,
                &mut emit,
                &base,
                RuntimeBootstrapState::Downloading,
                2_048,
                Some(8_192),
            );
        }

        let stored = status.lock().expect("status lock should succeed").clone();
        assert_eq!(stored.state, RuntimeBootstrapState::Downloading);
        assert_eq!(stored.downloaded_bytes, Some(2_048));
        assert_eq!(stored.total_bytes, Some(8_192));
        assert!(stored.error.is_none());

        assert_eq!(
            events.len(),
            1,
            "exactly one progress event must be emitted"
        );
        assert_eq!(events[0].0, RUNTIME_BOOTSTRAP_PROGRESS_EVENT);
        assert_eq!(
            events[0].1, stored,
            "the emitted snapshot must match the stored snapshot"
        );
    }

    #[test]
    fn download_runtime_source_keeps_a_newer_verified_catalog() {
        let mut refreshed = catalog::embedded_catalog().clone();
        refreshed.generation += 1;
        refreshed.release_id = "refreshed-release".to_owned();
        let cache = Arc::new(Mutex::new(Some(refreshed.clone())));

        let selected = download_runtime_source(&cache).expect("catalog cache should resolve");

        assert_eq!(selected.generation, refreshed.generation);
        assert_eq!(selected.release_id, refreshed.release_id);
    }

    #[test]
    fn post_download_timeout_marker_maps_to_a_structured_error() {
        let error = bootstrap_command_error_from_message(&runtime_parent_load_timeout_message());

        assert_eq!(
            error.code,
            crate::commands::error::ErrorCode::RuntimePostDownloadTimeout
        );
        assert!(
            error
                .message
                .contains(crate::commands::runtime_worker::RUNTIME_POST_DOWNLOAD_TIMEOUT_HINT),
            "timeout error must carry the diagnostic hint for the user"
        );
    }

    #[test]
    fn ensure_runtime_ready_rejects_missing_and_downloading() {
        for state in [
            RuntimeBootstrapState::Missing,
            RuntimeBootstrapState::Downloading,
            RuntimeBootstrapState::Corrupt,
            RuntimeBootstrapState::Failed,
        ] {
            let snapshot = RuntimeBootstrapStatusSnapshot {
                state,
                runtime_path: "/tmp/lib".to_owned(),
                downloaded_bytes: None,
                total_bytes: None,
                version: "v1.27.1".to_owned(),
                active_artifact_id: None,
                target_triple: catalog::current_target_triple().to_owned(),
                candidate_version: None,
                restart_required: false,
                error: None,
                cpu_fallback_notice: None,
            };
            let status = Arc::new(Mutex::new(snapshot));
            assert!(ensure_runtime_ready(&status).is_err());
        }
    }

    #[test]
    fn staging_candidate_rejects_runtime_that_does_not_support_a_model() {
        // Force the compatibility gate: strip supported-model list on the only runtime.
        let mut catalog = catalog::embedded_catalog().clone();
        let mut runtime = catalog.manifest.artifacts.runtimes[0].clone();
        runtime.target_triple = Some(catalog::current_target_triple().to_owned());
        runtime.deprecation = catalog::CatalogDeprecation::default();
        runtime.runtime.supported_model_artifact_ids.clear();
        catalog.manifest.artifacts.runtimes = vec![runtime];

        let status = Arc::new(Mutex::new(snapshot_from_inventory(&empty_inventory())));
        let mut emit = |_event: &'static str, _snapshot: RuntimeBootstrapStatusSnapshot| {};

        let error = download_and_stage_candidate_blocking(
            std::path::Path::new("/nonexistent/openkara-compat-gate"),
            &catalog,
            &status,
            &mut emit,
        )
        .expect_err("an incompatible runtime must be rejected before staging");

        assert!(
            error.to_string().contains("does not support model"),
            "unexpected error: {error}"
        );
    }

    // cpu_fallback_notice_for touches the process-wide DirectML flag, so these
    // tests run under a shared lock and reset the flag at the end of each.
    static DIRECTML_FLAG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn cpu_only_manifest() -> catalog::ReleaseManifest {
        let cpu_runtime = catalog::CatalogRuntime {
            artifact_id: "rt-windows-cpu".to_owned(),
            target_triple: Some("x86_64-pc-windows-msvc".to_owned()),
            filename: "cpu.zip".to_owned(),
            byte_size: 0,
            archive_digest: String::new(),
            download_url: String::new(),
            extracted_file_digests: Default::default(),
            runtime: catalog::CatalogRuntimeMetadata {
                version: "v1".to_owned(),
                ort_c_api_level: "27".to_owned(),
                execution_providers: vec!["cpu".to_owned()],
                supported_model_artifact_ids: vec![],
                companion_files: vec![],
            },
            deprecation: Default::default(),
        };
        let directml_runtime = catalog::CatalogRuntime {
            artifact_id: "rt-windows-dml".to_owned(),
            target_triple: Some("x86_64-pc-windows-msvc".to_owned()),
            filename: "dml.zip".to_owned(),
            byte_size: 0,
            archive_digest: String::new(),
            download_url: String::new(),
            extracted_file_digests: Default::default(),
            runtime: catalog::CatalogRuntimeMetadata {
                version: "v1".to_owned(),
                ort_c_api_level: "27".to_owned(),
                execution_providers: vec!["cpu".to_owned(), "directml".to_owned()],
                supported_model_artifact_ids: vec![],
                companion_files: vec![],
            },
            deprecation: Default::default(),
        };
        catalog::ReleaseManifest {
            schema_version: "openkara.catalog/release-v1".to_owned(),
            generation: 1,
            release_id: "test".to_owned(),
            artifacts: catalog::CatalogArtifacts {
                models: vec![],
                runtimes: vec![cpu_runtime, directml_runtime],
            },
            compatibility: vec![],
        }
    }

    #[test]
    fn cpu_fallback_notice_is_absent_when_directml_timeout_flag_is_clear() {
        let _guard = DIRECTML_FLAG_TEST_LOCK.lock().unwrap();
        crate::platform_capabilities::set_directml_disabled_by_timeout(false);
        let manifest = cpu_only_manifest();
        let artifact_id = Some("rt-windows-cpu".to_owned());
        assert_eq!(cpu_fallback_notice_for(&manifest, &artifact_id), None);
    }

    #[test]
    fn cpu_fallback_notice_is_absent_for_a_directml_runtime() {
        let _guard = DIRECTML_FLAG_TEST_LOCK.lock().unwrap();
        crate::platform_capabilities::set_directml_disabled_by_timeout(true);
        let manifest = cpu_only_manifest();
        let artifact_id = Some("rt-windows-dml".to_owned());
        assert_eq!(cpu_fallback_notice_for(&manifest, &artifact_id), None);
        crate::platform_capabilities::set_directml_disabled_by_timeout(false);
    }

    #[test]
    fn cpu_fallback_notice_is_present_for_a_cpu_only_runtime_when_flagged() {
        let _guard = DIRECTML_FLAG_TEST_LOCK.lock().unwrap();
        crate::platform_capabilities::set_directml_disabled_by_timeout(true);
        let manifest = cpu_only_manifest();
        let artifact_id = Some("rt-windows-cpu".to_owned());
        assert_eq!(
            cpu_fallback_notice_for(&manifest, &artifact_id),
            Some(CPU_FALLBACK_NOTICE.to_owned())
        );
        crate::platform_capabilities::set_directml_disabled_by_timeout(false);
    }
}
