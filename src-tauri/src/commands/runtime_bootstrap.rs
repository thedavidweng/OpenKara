use crate::{
    commands::error::{internal_error, model_bootstrap_error, CommandError, CommandResult},
    separator::runtime_bootstrap::{self, RuntimeResolution, RuntimeStatus, RuntimeStatusSnapshot},
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
pub const RUNTIME_BOOTSTRAP_READY_EVENT: &str = "runtime-bootstrap-ready";
pub const RUNTIME_BOOTSTRAP_ERROR_EVENT: &str = "runtime-bootstrap-error";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBootstrapState {
    Missing,
    Downloading,
    Ready,
    Corrupt,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeBootstrapStatusSnapshot {
    pub state: RuntimeBootstrapState,
    pub runtime_path: String,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub version: String,
    pub error: Option<CommandError>,
}

impl From<RuntimeStatusSnapshot> for RuntimeBootstrapStatusSnapshot {
    fn from(snap: RuntimeStatusSnapshot) -> Self {
        let state = match snap.status {
            RuntimeStatus::Missing => RuntimeBootstrapState::Missing,
            RuntimeStatus::Downloading => RuntimeBootstrapState::Downloading,
            RuntimeStatus::Ready => RuntimeBootstrapState::Ready,
            RuntimeStatus::Corrupt => RuntimeBootstrapState::Corrupt,
        };
        Self {
            state,
            runtime_path: snap.runtime_path,
            downloaded_bytes: snap.downloaded_bytes,
            total_bytes: snap.total_bytes,
            version: snap.version,
            error: None,
        }
    }
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
        RuntimeBootstrapState::Ready => Ok(()),
        RuntimeBootstrapState::Missing => Err(model_bootstrap_error(format!(
            "ONNX Runtime is not installed; runtime path: {}",
            snapshot.runtime_path
        ))),
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
    runtime_path: &Path,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) -> RuntimeBootstrapStatusSnapshot {
    RuntimeBootstrapStatusSnapshot {
        state: RuntimeBootstrapState::Downloading,
        runtime_path: runtime_path.display().to_string(),
        downloaded_bytes: Some(downloaded_bytes),
        total_bytes,
        version: runtime_bootstrap::ORT_RUNTIME_VERSION.to_owned(),
        error: None,
    }
}

fn ready_snapshot(runtime_path: &Path) -> RuntimeBootstrapStatusSnapshot {
    RuntimeBootstrapStatusSnapshot {
        state: RuntimeBootstrapState::Ready,
        runtime_path: runtime_path.display().to_string(),
        downloaded_bytes: None,
        total_bytes: None,
        version: runtime_bootstrap::ORT_RUNTIME_VERSION.to_owned(),
        error: None,
    }
}

fn failed_snapshot(runtime_path: &Path, error: CommandError) -> RuntimeBootstrapStatusSnapshot {
    RuntimeBootstrapStatusSnapshot {
        state: RuntimeBootstrapState::Failed,
        runtime_path: runtime_path.display().to_string(),
        downloaded_bytes: None,
        total_bytes: None,
        version: runtime_bootstrap::ORT_RUNTIME_VERSION.to_owned(),
        error: Some(error),
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

pub fn install_and_load_runtime_blocking(
    app_data_dir: &Path,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
) -> anyhow::Result<PathBuf> {
    let runtime_path = runtime_bootstrap::managed_runtime_path(app_data_dir);
    let initial = downloading_snapshot(&runtime_path, 0, None);
    store_snapshot(status, initial.clone());
    emit(RUNTIME_BOOTSTRAP_PROGRESS_EVENT, initial);

    let progress_runtime_path = runtime_path.clone();
    let installed = runtime_bootstrap::download_and_install_runtime(
        app_data_dir,
        |downloaded_bytes, total_bytes| {
            let snapshot =
                downloading_snapshot(&progress_runtime_path, downloaded_bytes, total_bytes);
            store_snapshot(status, snapshot.clone());
            emit(RUNTIME_BOOTSTRAP_PROGRESS_EVENT, snapshot);
        },
    )?;

    crate::separator::model::ensure_runtime_loaded_from_path(&installed)
        .with_context(|| format!("failed to load ONNX Runtime from {}", installed.display()))?;

    let snapshot = ready_snapshot(&installed);
    store_snapshot(status, snapshot.clone());
    emit(RUNTIME_BOOTSTRAP_READY_EVENT, snapshot);
    Ok(installed)
}

fn download_state_blocks_recovery(
    state: &RuntimeBootstrapState,
    resolution: &RuntimeResolution,
) -> bool {
    *state == RuntimeBootstrapState::Downloading
        && !matches!(resolution, RuntimeResolution::Ready(_))
}

pub fn ensure_runtime_ready_or_install_blocking(
    app_data_dir: &Path,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, RuntimeBootstrapStatusSnapshot),
) -> CommandResult<PathBuf> {
    let snapshot = get_runtime_bootstrap_status_from_state(status)?;
    let resolution = runtime_bootstrap::resolve_runtime_installation(app_data_dir)
        .map_err(|error| model_bootstrap_error(error.to_string()))?;

    if download_state_blocks_recovery(&snapshot.state, &resolution) {
        return Err(model_bootstrap_error(format!(
            "ONNX Runtime is still downloading to {}",
            snapshot.runtime_path
        )));
    }

    match resolution {
        RuntimeResolution::Ready(path) => {
            crate::separator::model::ensure_runtime_loaded_from_path(&path)
                .map_err(|error| model_bootstrap_error(error.to_string()))?;
            let ready = ready_snapshot(&path);
            store_snapshot(status, ready.clone());
            emit(RUNTIME_BOOTSTRAP_READY_EVENT, ready);
            Ok(path)
        }
        RuntimeResolution::Corrupt(_) => {
            let _ = runtime_bootstrap::delete_runtime(app_data_dir);
            install_and_load_runtime_blocking(app_data_dir, status, emit).map_err(|error| {
                let command_error = model_bootstrap_error(error.to_string());
                let failed = failed_snapshot(
                    &runtime_bootstrap::managed_runtime_path(app_data_dir),
                    command_error.clone(),
                );
                store_snapshot(status, failed.clone());
                emit(RUNTIME_BOOTSTRAP_ERROR_EVENT, failed);
                command_error
            })
        }
        RuntimeResolution::Absent => {
            install_and_load_runtime_blocking(app_data_dir, status, emit).map_err(|error| {
                let command_error = model_bootstrap_error(error.to_string());
                let failed = failed_snapshot(
                    &runtime_bootstrap::managed_runtime_path(app_data_dir),
                    command_error.clone(),
                );
                store_snapshot(status, failed.clone());
                emit(RUNTIME_BOOTSTRAP_ERROR_EVENT, failed);
                command_error
            })
        }
    }
}

#[tauri::command]
pub fn download_runtime(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> CommandResult<RuntimeBootstrapStatusSnapshot> {
    let app_data_dir = state.shell.app_data_dir.clone();
    let runtime_path = runtime_bootstrap::managed_runtime_path(&app_data_dir);
    let status = Arc::clone(&state.shell.runtime_bootstrap_status);

    let initial = downloading_snapshot(&runtime_path, 0, None);
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
            install_and_load_runtime_blocking(&task_app_data_dir, &task_status, &mut emit)
        })
        .await;

        match result {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                let cmd_error = model_bootstrap_error(error.to_string());
                let snapshot = failed_snapshot(&runtime_path, cmd_error);
                store_snapshot(&status, snapshot.clone());
                let _ = app_handle.emit(RUNTIME_BOOTSTRAP_ERROR_EVENT, snapshot);
            }
            Err(error) => {
                let cmd_error = model_bootstrap_error(error.to_string());
                let snapshot = failed_snapshot(&runtime_path, cmd_error);
                store_snapshot(&status, snapshot.clone());
                let _ = app_handle.emit(RUNTIME_BOOTSTRAP_ERROR_EVENT, snapshot);
            }
        }
    });

    Ok(initial)
}

#[tauri::command]
pub fn delete_runtime(state: State<'_, AppState>) -> CommandResult<()> {
    let app_data_dir = state.shell.app_data_dir.clone();
    runtime_bootstrap::delete_runtime(&app_data_dir)
        .map_err(|e| internal_error(format!("failed to delete runtime: {e}")))?;

    let snapshot = runtime_bootstrap::runtime_status_snapshot(&app_data_dir);
    let bootstrap_snapshot: RuntimeBootstrapStatusSnapshot = snapshot.into();
    if let Ok(mut current) = state.shell.runtime_bootstrap_status.lock() {
        *current = bootstrap_snapshot;
    }

    Ok(())
}

/// Sync the runtime bootstrap status from disk. Called after settings changes
/// or startup to ensure the UI reflects the current state.
pub fn sync_runtime_bootstrap_status(
    app_data_dir: &std::path::Path,
    status: &Arc<Mutex<RuntimeBootstrapStatusSnapshot>>,
) -> CommandResult<RuntimeBootstrapStatusSnapshot> {
    let snapshot = runtime_bootstrap::runtime_status_snapshot(app_data_dir);
    let bootstrap_snapshot: RuntimeBootstrapStatusSnapshot = snapshot.into();
    let mut guard = status.lock().map_err(|_| {
        crate::commands::error::state_lock_error("runtime status lock was poisoned")
    })?;
    *guard = bootstrap_snapshot.clone();
    Ok(bootstrap_snapshot)
}

#[cfg(test)]
mod tests {
    use super::{download_state_blocks_recovery, RuntimeBootstrapState};
    use crate::separator::runtime_bootstrap::RuntimeResolution;
    use std::path::PathBuf;

    #[test]
    fn stale_downloading_state_yields_to_ready_disk_installation() {
        let resolution = RuntimeResolution::Ready(PathBuf::from("managed-runtime"));

        assert!(!download_state_blocks_recovery(
            &RuntimeBootstrapState::Downloading,
            &resolution,
        ));
    }

    #[test]
    fn active_download_remains_blocked_until_disk_installation_is_ready() {
        assert!(download_state_blocks_recovery(
            &RuntimeBootstrapState::Downloading,
            &RuntimeResolution::Absent,
        ));
        assert!(download_state_blocks_recovery(
            &RuntimeBootstrapState::Downloading,
            &RuntimeResolution::Corrupt(PathBuf::from("managed-runtime")),
        ));
    }
}
