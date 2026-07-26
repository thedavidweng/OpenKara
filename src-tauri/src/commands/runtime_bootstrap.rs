use crate::{
    commands::error::{internal_error, model_bootstrap_error, CommandError, CommandResult},
    separator::catalog::{self, VerifiedCatalog},
    separator::runtime_bootstrap::{self, RuntimeInventory},
    AppState,
};
use anyhow::Context;
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Emitter, State};

pub const RUNTIME_BOOTSTRAP_PROGRESS_EVENT: &str = "runtime-bootstrap-progress";

/// Process-wide flag preventing concurrent `download_runtime` invocations
/// from racing on the shared artifact directory and slot file.
static RUNTIME_DOWNLOAD_IN_PROGRESS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub const RUNTIME_BOOTSTRAP_READY_EVENT: &str = "runtime-bootstrap-ready";
pub const RUNTIME_BOOTSTRAP_ERROR_EVENT: &str = "runtime-bootstrap-error";

/// Reported when a pre-slot legacy install is active. The legacy library's
/// exact version is unknown by design — its pinned constants are gone.
pub const LEGACY_RUNTIME_VERSION: &str = "legacy";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBootstrapState {
    Missing,
    Downloading,
    Ready,
    UpdateAvailable,
    DownloadingCandidate,
    CandidateReadyRestartRequired,
    ActivationFailedPreviousRestored,
    Corrupt,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeBootstrapStatusSnapshot {
    pub state: RuntimeBootstrapState,
    pub runtime_path: String,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    /// Upstream version of the ACTIVE runtime (`v1.27.1`), `legacy` for a
    /// pre-slot install, or the pinned catalog version when nothing is
    /// installed yet. Never a single global constant: each platform reports
    /// the artifact it actually has.
    pub version: String,
    pub active_artifact_id: Option<String>,
    pub target_triple: String,
    pub candidate_version: Option<String>,
    pub restart_required: bool,
    pub error: Option<CommandError>,
}

fn pinned_runtime_version() -> String {
    let catalog = catalog::embedded_catalog();
    catalog::resolve_runtime(&catalog.manifest, catalog::current_target_triple())
        .map(|runtime| runtime.runtime.version.clone())
        .unwrap_or_else(|_| "unknown".to_owned())
}

/// Derive the disk-truth snapshot from the runtime inventory. Transient
/// states (downloading, update-available) are layered on by the flows that
/// own them.
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
                        .map(|failure| model_bootstrap_error(failure.error.clone())),
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
                        Some(model_bootstrap_error(failure.error.clone())),
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
        // States with a loaded, working active runtime.
        RuntimeBootstrapState::Ready
        | RuntimeBootstrapState::UpdateAvailable
        | RuntimeBootstrapState::DownloadingCandidate
        | RuntimeBootstrapState::CandidateReadyRestartRequired
        | RuntimeBootstrapState::ActivationFailedPreviousRestored => Ok(()),
        RuntimeBootstrapState::Missing => Err(model_bootstrap_error(
            "ONNX Runtime is not installed; install it from Settings".to_owned(),
        )),
        RuntimeBootstrapState::Downloading => Err(model_bootstrap_error(format!(
            "ONNX Runtime is still downloading to {}",
            snapshot.runtime_path
        ))),
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

/// Resolve the catalog runtime a download should install: the freshest
/// verified catalog when `check_*_updates` cached one newer than the
/// embedded snapshot, otherwise the embedded pin.
fn download_runtime_source(
    catalog_cache: &Arc<Mutex<Option<VerifiedCatalog>>>,
) -> CommandResult<(VerifiedCatalog, ())> {
    let embedded = catalog::embedded_catalog();
    let cache = catalog_cache
        .lock()
        .map_err(|_| crate::commands::error::state_lock_error("catalog cache lock was poisoned"))?;
    let catalog = match cache.as_ref() {
        Some(cached) if cached.generation > embedded.generation => cached.clone(),
        _ => embedded.clone(),
    };
    Ok((catalog, ()))
}

/// First-install path: download, verify, activate, and load the runtime in
/// this process. Only valid while no runtime is loaded — updates go through
/// the candidate flow instead.
pub fn install_and_load_runtime_blocking(
    app_data_dir: &Path,
    catalog: &VerifiedCatalog,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
) -> anyhow::Result<PathBuf> {
    let runtime = catalog::resolve_runtime(&catalog.manifest, catalog::current_target_triple())?;

    let base = snapshot_from_disk(app_data_dir);
    let initial = downloading_snapshot(&base, RuntimeBootstrapState::Downloading, 0, None);
    store_snapshot(status, initial.clone());
    emit(RUNTIME_BOOTSTRAP_PROGRESS_EVENT, initial);

    let installed = runtime_bootstrap::install_runtime_artifact(
        app_data_dir,
        runtime,
        catalog,
        |downloaded_bytes, total_bytes| {
            report_download_progress(
                status,
                emit,
                &base,
                RuntimeBootstrapState::Downloading,
                downloaded_bytes,
                total_bytes,
            );
        },
    )?;

    // Prove the dynamic load BEFORE persisting activation: a runtime whose
    // files verify but whose library cannot load must never become the
    // recorded active runtime (it would report Ready while separation
    // fails, with no recovery path).
    crate::separator::model::ensure_runtime_loaded_from_path(&installed.library_path)
        .with_context(|| {
            format!(
                "failed to load ONNX Runtime from {}",
                installed.library_path.display()
            )
        })?;
    runtime_bootstrap::activate_first_install(app_data_dir, &installed.record.artifact_id)?;

    let snapshot = snapshot_from_disk(app_data_dir);
    store_snapshot(status, snapshot.clone());
    emit(RUNTIME_BOOTSTRAP_READY_EVENT, snapshot);
    Ok(installed.library_path)
}

/// Update path: download and verify the runtime into its artifact directory
/// and stage it as the next-launch candidate. The running process keeps its
/// loaded runtime — activation happens at the next startup.
pub fn download_and_stage_candidate_blocking(
    app_data_dir: &Path,
    catalog: &VerifiedCatalog,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
) -> anyhow::Result<()> {
    let runtime = catalog::resolve_runtime(&catalog.manifest, catalog::current_target_triple())?;

    // Runtime/model compatibility gate: the staged runtime must support a
    // catalog model for every installed variant. The manifest validator
    // guarantees same-generation reciprocity, so this check exists to fail
    // loudly if that invariant is ever violated rather than staging an
    // incompatible candidate.
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
    let initial = downloading_snapshot(&base, RuntimeBootstrapState::DownloadingCandidate, 0, None);
    store_snapshot(status, initial.clone());
    emit(RUNTIME_BOOTSTRAP_PROGRESS_EVENT, initial);

    let installed = runtime_bootstrap::install_runtime_artifact(
        app_data_dir,
        runtime,
        catalog,
        |downloaded_bytes, total_bytes| {
            report_download_progress(
                status,
                emit,
                &base,
                RuntimeBootstrapState::DownloadingCandidate,
                downloaded_bytes,
                total_bytes,
            );
        },
    )?;

    runtime_bootstrap::stage_candidate(app_data_dir, &installed.record.artifact_id)?;

    let snapshot = snapshot_from_disk(app_data_dir);
    store_snapshot(status, snapshot.clone());
    emit(RUNTIME_BOOTSTRAP_READY_EVENT, snapshot);
    Ok(())
}

/// Separation gate: make a runtime loadable now, installing it first when
/// nothing is on disk. Used by the blocking separation path.
pub fn ensure_runtime_ready_or_install_blocking(
    app_data_dir: &Path,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
) -> CommandResult<PathBuf> {
    // A runtime committed into this process is what separation will use,
    // regardless of what the slots say (e.g. after delete + reinstall the
    // old library stays mapped until restart).
    if let Some(loaded) = crate::separator::model::loaded_runtime_path() {
        return Ok(loaded.to_path_buf());
    }

    let snapshot = get_runtime_bootstrap_status_from_state(status)?;
    let inventory = runtime_bootstrap::runtime_inventory(app_data_dir);

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
        crate::separator::model::ensure_runtime_loaded_from_path(&active.library_path)
            .map_err(|error| model_bootstrap_error(error.to_string()))?;
        let ready = snapshot_from_disk(app_data_dir);
        store_snapshot(status, ready.clone());
        emit(RUNTIME_BOOTSTRAP_READY_EVENT, ready);
        return Ok(active.library_path);
    }
    if let Some(legacy_path) = inventory.legacy_path {
        crate::separator::model::ensure_runtime_loaded_from_path(&legacy_path)
            .map_err(|error| model_bootstrap_error(error.to_string()))?;
        let ready = snapshot_from_disk(app_data_dir);
        store_snapshot(status, ready.clone());
        emit(RUNTIME_BOOTSTRAP_READY_EVENT, ready);
        return Ok(legacy_path);
    }

    let catalog = catalog::embedded_catalog();
    install_and_load_runtime_blocking(app_data_dir, catalog, status, emit).map_err(|error| {
        let command_error = model_bootstrap_error(error.to_string());
        let mut failed = snapshot_from_disk(app_data_dir);
        failed.state = RuntimeBootstrapState::Failed;
        failed.error = Some(command_error.clone());
        store_snapshot(status, failed.clone());
        emit(RUNTIME_BOOTSTRAP_ERROR_EVENT, failed);
        command_error
    })
}

/// Install the runtime. First install downloads and loads immediately; when
/// a runtime is already active (or a legacy install is loaded), the download
/// is staged as a next-launch candidate instead — a loaded runtime is never
/// replaced in place.
#[tauri::command]
pub fn download_runtime(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> CommandResult<RuntimeBootstrapStatusSnapshot> {
    let app_data_dir = state.shell.app_data_dir.clone();
    let status = Arc::clone(&state.shell.runtime_bootstrap_status);
    let (catalog, ()) = download_runtime_source(&state.shell.catalog_cache)?;

    if RUNTIME_DOWNLOAD_IN_PROGRESS.swap(true, std::sync::atomic::Ordering::SeqCst) {
        // A download is already running; report its current state instead
        // of spawning a second install racing on the same directories.
        return get_runtime_bootstrap_status_from_state(&status);
    }

    let inventory = runtime_bootstrap::runtime_inventory(&app_data_dir);
    // A runtime loaded into this process can never be replaced in place,
    // even when the slots were just deleted — treat any loaded runtime as
    // the update (candidate + restart) flow.
    let is_update = inventory.active.is_some()
        || inventory.legacy_path.is_some()
        || crate::separator::model::loaded_runtime_path().is_some();

    let base = snapshot_from_disk(&app_data_dir);
    let initial_state = if is_update {
        RuntimeBootstrapState::DownloadingCandidate
    } else {
        RuntimeBootstrapState::Downloading
    };
    let initial = downloading_snapshot(&base, initial_state, 0, None);
    store_snapshot(&status, initial.clone());
    let _ = app_handle.emit(RUNTIME_BOOTSTRAP_PROGRESS_EVENT, initial.clone());

    tauri::async_runtime::spawn(async move {
        let task_status = Arc::clone(&status);
        let task_app_handle = app_handle.clone();
        let task_app_data_dir = app_data_dir.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let mut emit = |event, snapshot| {
                let _ = task_app_handle.emit(event, snapshot);
            };
            if is_update {
                download_and_stage_candidate_blocking(
                    &task_app_data_dir,
                    &catalog,
                    &task_status,
                    &mut emit,
                )
            } else {
                install_and_load_runtime_blocking(
                    &task_app_data_dir,
                    &catalog,
                    &task_status,
                    &mut emit,
                )
                .map(|_| ())
            }
        })
        .await;

        RUNTIME_DOWNLOAD_IN_PROGRESS.store(false, std::sync::atomic::Ordering::SeqCst);

        let flattened = match result {
            Ok(inner) => inner,
            Err(join_error) => Err(anyhow::anyhow!(join_error.to_string())),
        };
        if let Err(error) = flattened {
            let command_error = model_bootstrap_error(error.to_string());
            let mut failed = snapshot_from_disk(&app_data_dir);
            failed.state = RuntimeBootstrapState::Failed;
            failed.error = Some(command_error);
            store_snapshot(&status, failed.clone());
            let _ = app_handle.emit(RUNTIME_BOOTSTRAP_ERROR_EVENT, failed);
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

/// Check the stable catalog for a runtime update for this target. Mirrors
/// the model update semantics and shares the same verified-catalog cache. A
/// failed check never affects the readiness of the installed runtime.
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
            let runtime =
                catalog::resolve_runtime(&catalog.manifest, catalog::current_target_triple())
                    .map_err(|error| {
                        model_bootstrap_error(format!("update check failed: {error}"))
                    })?;

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

            // Surface update availability through the lifecycle state so the UI
            // and status consumers see it without re-deriving from the report.
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

/// Sync the runtime bootstrap status from disk. Called after settings changes
/// or startup to ensure the UI reflects the current state.
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
            };
            let status = Arc::new(Mutex::new(snapshot));
            assert!(ensure_runtime_ready(&status).is_ok());
        }
    }

    #[test]
    fn report_download_progress_stores_and_emits_the_snapshot() {
        // The separation-triggered first install used to only store the
        // in-flight snapshot without emitting it, leaving the UI stuck at "0%".
        // This guards that progress now both persists AND reaches the frontend.
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
            };
            let status = Arc::new(Mutex::new(snapshot));
            assert!(ensure_runtime_ready(&status).is_err());
        }
    }

    #[test]
    fn staging_candidate_rejects_runtime_that_does_not_support_a_model() {
        // The manifest validator guarantees same-generation runtime/model
        // reciprocity, so this defensive gate should never fire in practice.
        // It must still bail loudly — rather than stage a runtime that cannot
        // serve an installed model — if that invariant is ever violated. We
        // reduce the catalog to a single active runtime for the current target
        // and strip its supported-model list so the gate is the only reachable
        // outcome; the bail happens before any download, so no I/O is needed.
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
}
