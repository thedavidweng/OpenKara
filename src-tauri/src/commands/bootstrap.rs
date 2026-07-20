use crate::{
    commands::error::{
        internal_error, model_bootstrap_error, state_lock_error, CommandError, CommandResult,
    },
    config::{self, ModelVariant},
    separator, AppState,
};
use serde::Serialize;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

pub const MODEL_BOOTSTRAP_PROGRESS_EVENT: &str = "model-bootstrap-progress";
pub const MODEL_BOOTSTRAP_READY_EVENT: &str = "model-bootstrap-ready";
pub const MODEL_BOOTSTRAP_ERROR_EVENT: &str = "model-bootstrap-error";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelBootstrapState {
    Pending,
    Downloading,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelBootstrapStatusSnapshot {
    pub state: ModelBootstrapState,
    pub model_path: String,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub error: Option<CommandError>,
}

#[tauri::command]
pub fn get_model_bootstrap_status(
    state: State<'_, AppState>,
) -> CommandResult<ModelBootstrapStatusSnapshot> {
    get_model_bootstrap_status_from_state(&state.shell.model_bootstrap_status)
}

pub fn get_model_bootstrap_status_from_state(
    status: &Arc<Mutex<ModelBootstrapStatusSnapshot>>,
) -> CommandResult<ModelBootstrapStatusSnapshot> {
    status
        .lock()
        .map(|snapshot| snapshot.clone())
        .map_err(|_| state_lock_error("model bootstrap status lock was poisoned"))
}

pub fn emit_model_bootstrap_snapshot<R: Runtime>(
    app: &AppHandle<R>,
    snapshot: &ModelBootstrapStatusSnapshot,
) {
    let event = match snapshot.state {
        ModelBootstrapState::Ready => MODEL_BOOTSTRAP_READY_EVENT,
        ModelBootstrapState::Failed => MODEL_BOOTSTRAP_ERROR_EVENT,
        ModelBootstrapState::Pending | ModelBootstrapState::Downloading => {
            MODEL_BOOTSTRAP_PROGRESS_EVENT
        }
    };
    let _ = app.emit(event, snapshot);
}

pub fn ensure_model_ready(status: &Arc<Mutex<ModelBootstrapStatusSnapshot>>) -> CommandResult<()> {
    let snapshot = get_model_bootstrap_status_from_state(status)?;

    match snapshot.state {
        ModelBootstrapState::Ready => Ok(()),
        ModelBootstrapState::Pending => Err(model_bootstrap_error(format!(
            "model bootstrap has not started for {}",
            snapshot.model_path
        ))),
        ModelBootstrapState::Downloading => Err(model_bootstrap_error(format!(
            "model bootstrap is still downloading to {}",
            snapshot.model_path
        ))),
        ModelBootstrapState::Failed => Err(snapshot.error.unwrap_or_else(|| {
            model_bootstrap_error(format!(
                "model bootstrap failed for {}",
                snapshot.model_path
            ))
        })),
    }
}

pub fn ensure_active_model_ready_or_install_blocking(
    app_data_dir: &Path,
    status: &Arc<Mutex<ModelBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, ModelBootstrapStatusSnapshot),
) -> CommandResult<std::path::PathBuf> {
    let active_variant = config::load_config(app_data_dir)
        .ok()
        .flatten()
        .map(|config| config.effective_model_variant())
        .unwrap_or_default();
    let descriptor = separator::bootstrap::descriptor_for(active_variant);
    let managed_path = separator::bootstrap::managed_model_path_for(app_data_dir, descriptor);
    let dev_path = separator::model::default_model_path_for_filename(descriptor.filename);

    match separator::bootstrap::resolve_model_installation(&managed_path, &dev_path)
        .map_err(|error| internal_error(format!("failed to inspect model status: {error}")))?
    {
        separator::bootstrap::ModelInstallationResolution::Ready(resolved) => {
            let snapshot = ready_status(resolved.path.display().to_string());
            if let Ok(mut current) = status.lock() {
                *current = snapshot.clone();
            }
            emit(MODEL_BOOTSTRAP_READY_EVENT, snapshot);
            Ok(resolved.path)
        }
        separator::bootstrap::ModelInstallationResolution::Absent => {
            let initial = downloading_status(managed_path.display().to_string(), 0, None);
            if let Ok(mut current) = status.lock() {
                *current = initial.clone();
            }
            emit(MODEL_BOOTSTRAP_PROGRESS_EVENT, initial);

            let progress_path = managed_path.display().to_string();
            let manifest = separator::upstream::fetch_upstream_manifest().map_err(|error| {
                internal_error(format!("failed to resolve latest model: {error}"))
            })?;
            let latest = separator::upstream::latest_for_variant(&manifest, active_variant);

            let download_result = separator::bootstrap::download_and_install_model(
                &managed_path,
                &latest.url,
                &latest.sha256,
                Some(&latest.tag),
                |downloaded_bytes, total_bytes| {
                    let snapshot =
                        downloading_status(progress_path.clone(), downloaded_bytes, total_bytes);
                    if let Ok(mut current) = status.lock() {
                        *current = snapshot.clone();
                    }
                    emit(MODEL_BOOTSTRAP_PROGRESS_EVENT, snapshot);
                },
            );

            match download_result {
                Ok(()) => {
                    let snapshot = ready_status(managed_path.display().to_string());
                    if let Ok(mut current) = status.lock() {
                        *current = snapshot.clone();
                    }
                    emit(MODEL_BOOTSTRAP_READY_EVENT, snapshot);
                    Ok(managed_path)
                }
                Err(error) => {
                    let command_error = model_bootstrap_error(error.to_string());
                    let snapshot =
                        failed_status(managed_path.display().to_string(), command_error.clone());
                    if let Ok(mut current) = status.lock() {
                        *current = snapshot.clone();
                    }
                    emit(MODEL_BOOTSTRAP_ERROR_EVENT, snapshot);
                    Err(command_error)
                }
            }
        }
    }
}

pub fn sync_active_model_bootstrap_status(
    app_data_dir: &Path,
    status: &Arc<Mutex<ModelBootstrapStatusSnapshot>>,
) -> CommandResult<ModelBootstrapStatusSnapshot> {
    // Recompute bootstrap state from the active variant whenever settings change
    // so the UI reflects the model that separation will actually use next.
    let active_variant = config::load_config(app_data_dir)
        .map_err(|error| internal_error(format!("failed to load config: {error}")))?
        .unwrap_or_default()
        .effective_model_variant();
    let descriptor = separator::bootstrap::descriptor_for(active_variant);
    let development_model_path =
        separator::model::default_model_path_for_filename(descriptor.filename);
    let startup = crate::derive_startup_model_bootstrap(
        app_data_dir,
        &development_model_path,
        active_variant,
    )
    .map_err(|error| internal_error(format!("failed to derive bootstrap status: {error}")))?;

    let snapshot = startup.status;
    let mut guard = status
        .lock()
        .map_err(|_| state_lock_error("model bootstrap status lock was poisoned"))?;
    *guard = snapshot.clone();
    Ok(snapshot)
}

fn is_active_variant(app_data_dir: &Path, variant: ModelVariant) -> bool {
    config::load_config(app_data_dir)
        .ok()
        .flatten()
        .unwrap_or_default()
        .effective_model_variant()
        == variant
}

pub fn pending_status(model_path: impl Into<String>) -> ModelBootstrapStatusSnapshot {
    ModelBootstrapStatusSnapshot {
        state: ModelBootstrapState::Pending,
        model_path: model_path.into(),
        downloaded_bytes: None,
        total_bytes: None,
        error: None,
    }
}

pub fn downloading_status(
    model_path: impl Into<String>,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) -> ModelBootstrapStatusSnapshot {
    ModelBootstrapStatusSnapshot {
        state: ModelBootstrapState::Downloading,
        model_path: model_path.into(),
        downloaded_bytes: Some(downloaded_bytes),
        total_bytes,
        error: None,
    }
}

pub fn ready_status(model_path: impl Into<String>) -> ModelBootstrapStatusSnapshot {
    ModelBootstrapStatusSnapshot {
        state: ModelBootstrapState::Ready,
        model_path: model_path.into(),
        downloaded_bytes: None,
        total_bytes: None,
        error: None,
    }
}

pub fn failed_status(
    model_path: impl Into<String>,
    error: CommandError,
) -> ModelBootstrapStatusSnapshot {
    ModelBootstrapStatusSnapshot {
        state: ModelBootstrapState::Failed,
        model_path: model_path.into(),
        downloaded_bytes: None,
        total_bytes: None,
        error: Some(error),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatusSnapshot {
    pub variant: String,
    pub downloaded: bool,
    /// Release tag recorded in the verification manifest, if a verified
    /// model is installed. `None` when no manifest exists or the manifest
    /// predates the release-tag field.
    pub installed_tag: Option<String>,
    pub file_size: Option<u64>,
}

#[tauri::command]
pub fn get_model_status(
    app_handle: AppHandle,
    variant: String,
) -> CommandResult<ModelStatusSnapshot> {
    let model_variant = ModelVariant::parse(&variant)
        .ok_or_else(|| internal_error(format!("invalid model variant: {variant}")))?;
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let descriptor = separator::bootstrap::descriptor_for(model_variant);
    let model_path = separator::bootstrap::managed_model_path_for(&app_data_dir, descriptor);
    let resolved = separator::bootstrap::resolve_model_installation(
        &model_path,
        &app_data_dir.join("__no_dev_fallback_model__"),
    )
    .map_err(|error| internal_error(format!("failed to inspect model status: {error}")))?;
    let downloaded = matches!(
        resolved,
        separator::bootstrap::ModelInstallationResolution::Ready(_)
    );
    let installed_tag = separator::bootstrap::installed_release_tag(&model_path)
        .map_err(|error| internal_error(format!("failed to read model manifest: {error}")))?;
    let file_size = separator::bootstrap::model_file_size(&app_data_dir, model_variant);
    Ok(ModelStatusSnapshot {
        variant,
        downloaded,
        installed_tag,
        file_size,
    })
}

/// Result of comparing the installed model against the upstream latest
/// release. The frontend uses this to show an update prompt and trigger
/// a download-and-replace flow.
#[derive(Debug, Clone, Serialize)]
pub struct ModelUpdateInfo {
    pub variant: String,
    /// Release tag of the currently installed model, or `None` if no
    /// verified model is installed or the manifest predates the tag field.
    pub installed_tag: Option<String>,
    /// Release tag of the newest upstream release for this variant.
    pub latest_tag: String,
    /// Disk size of the latest release asset in bytes.
    pub latest_size: u64,
    /// `true` when the installed checksum differs from the upstream latest
    /// checksum (or no verified model is installed).
    pub update_available: bool,
}

#[tauri::command]
pub fn check_model_update(
    app_handle: AppHandle,
    variant: String,
) -> CommandResult<ModelUpdateInfo> {
    let model_variant = ModelVariant::parse(&variant)
        .ok_or_else(|| internal_error(format!("invalid model variant: {variant}")))?;
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let descriptor = separator::bootstrap::descriptor_for(model_variant);
    let model_path = separator::bootstrap::managed_model_path_for(&app_data_dir, descriptor);

    // Read the installed manifest to get the recorded checksum and release tag.
    let installed_manifest = separator::verified_manifest::read_verified_manifest(&model_path)
        .map_err(|error| internal_error(format!("failed to read model manifest: {error}")))?;
    let installed_tag = installed_manifest
        .as_ref()
        .and_then(|m| m.release_tag.clone());
    let installed_sha256 = installed_manifest.as_ref().map(|m| m.sha256.as_str());

    // Fetch the upstream latest release for this variant.
    let manifest = separator::upstream::fetch_upstream_manifest()
        .map_err(|error| internal_error(format!("failed to fetch upstream manifest: {error}")))?;
    let latest = separator::upstream::latest_for_variant(&manifest, model_variant);

    // An update is available when the installed checksum differs from the
    // upstream latest, or when no verified model is installed at all.
    let update_available = installed_sha256 != Some(latest.sha256.as_str());

    Ok(ModelUpdateInfo {
        variant,
        installed_tag,
        latest_tag: latest.tag.clone(),
        latest_size: latest.size,
        update_available,
    })
}

#[tauri::command]
pub fn download_model(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    variant: String,
) -> CommandResult<ModelBootstrapStatusSnapshot> {
    crate::commands::runtime_bootstrap::ensure_runtime_ready(
        &state.shell.runtime_bootstrap_status,
    )?;

    let model_variant = ModelVariant::parse(&variant)
        .ok_or_else(|| internal_error(format!("invalid model variant: {variant}")))?;
    let descriptor = separator::bootstrap::descriptor_for(model_variant);
    let model_path =
        separator::bootstrap::managed_model_path_for(&state.shell.app_data_dir, descriptor);
    let should_publish_status = is_active_variant(&state.shell.app_data_dir, model_variant);

    if separator::bootstrap::resolve_existing_model_path(
        &model_path,
        &state.shell.app_data_dir.join("__no_dev_fallback_model__"),
    )
    .map_err(|error| internal_error(format!("failed to inspect model status: {error}")))?
    .is_some()
    {
        // An explicit download button is about installing the managed copy for
        // this variant. A verified dev fallback is enough to run locally, but it
        // should not make the managed install appear already downloaded.
        return Ok(ready_status(model_path.display().to_string()));
    }

    let status = Arc::clone(&state.shell.model_bootstrap_status);
    let initial = downloading_status(model_path.display().to_string(), 0, None);
    if should_publish_status {
        // Bootstrap status is single-slot state for the currently active model.
        // Downloads for inactive variants must not overwrite what the active
        // playback path reports as ready/pending/downloading.
        if let Ok(mut current) = status.lock() {
            *current = initial.clone();
        }
    }

    let progress_path = model_path.display().to_string();
    let should_publish_status_for_task = should_publish_status;
    let task_variant = model_variant;
    let task_app_data_dir = state.shell.app_data_dir.clone();

    tauri::async_runtime::spawn(async move {
        let blocking_status = Arc::clone(&status);
        let blocking_app_handle = app_handle.clone();
        let blocking_model_path = model_path.clone();
        let progress_path = progress_path.clone();
        let blocking_app_data_dir = task_app_data_dir.clone();

        let result = tauri::async_runtime::spawn_blocking(move || {
            // Resolve the latest release at download time so the app always
            // fetches the newest model without a code change.
            let manifest = separator::upstream::fetch_upstream_manifest()?;
            let latest = separator::upstream::latest_for_variant(&manifest, task_variant);
            separator::bootstrap::download_and_install_model(
                &blocking_model_path,
                &latest.url,
                &latest.sha256,
                Some(&latest.tag),
                |downloaded_bytes, total_bytes| {
                    if should_publish_status_for_task
                        && is_active_variant(&blocking_app_data_dir, task_variant)
                    {
                        let snapshot = downloading_status(
                            progress_path.clone(),
                            downloaded_bytes,
                            total_bytes,
                        );
                        if let Ok(mut current) = blocking_status.lock() {
                            *current = snapshot.clone();
                        }
                        let _ = blocking_app_handle.emit(MODEL_BOOTSTRAP_PROGRESS_EVENT, snapshot);
                    }
                },
            )
        })
        .await;

        if !should_publish_status_for_task || !is_active_variant(&task_app_data_dir, task_variant) {
            return;
        }

        match result {
            Ok(Ok(())) => {
                let snapshot = ready_status(model_path.display().to_string());
                if let Ok(mut current) = status.lock() {
                    *current = snapshot.clone();
                }
                let _ = app_handle.emit(MODEL_BOOTSTRAP_READY_EVENT, snapshot);
            }
            Ok(Err(error)) => {
                let command_error = model_bootstrap_error(error.to_string());
                let snapshot = failed_status(model_path.display().to_string(), command_error);
                if let Ok(mut current) = status.lock() {
                    *current = snapshot.clone();
                }
                let _ = app_handle.emit(MODEL_BOOTSTRAP_ERROR_EVENT, snapshot);
            }
            Err(error) => {
                let command_error = model_bootstrap_error(error.to_string());
                let snapshot = failed_status(model_path.display().to_string(), command_error);
                if let Ok(mut current) = status.lock() {
                    *current = snapshot.clone();
                }
                let _ = app_handle.emit(MODEL_BOOTSTRAP_ERROR_EVENT, snapshot);
            }
        }
    });

    Ok(initial)
}

#[tauri::command]
pub fn delete_model(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    variant: String,
) -> CommandResult<()> {
    let model_variant = ModelVariant::parse(&variant)
        .ok_or_else(|| internal_error(format!("invalid model variant: {variant}")))?;
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    separator::bootstrap::delete_model_file(&app_data_dir, model_variant)
        .map_err(|e| internal_error(format!("failed to delete model: {e}")))?;

    let snapshot =
        sync_active_model_bootstrap_status(&app_data_dir, &state.shell.model_bootstrap_status)?;
    emit_model_bootstrap_snapshot(&app_handle, &snapshot);

    if matches!(snapshot.state, ModelBootstrapState::Pending) {
        let active = config::load_config(&app_data_dir)
            .ok()
            .flatten()
            .unwrap_or_default()
            .effective_model_variant();
        if model_variant == active {
            let descriptor = separator::bootstrap::descriptor_for(active);
            let managed = separator::bootstrap::managed_model_path_for(&app_data_dir, descriptor);
            crate::app_runtime::spawn_model_bootstrap_worker(
                app_handle.clone(),
                managed,
                active,
                Arc::clone(&state.shell.model_bootstrap_status),
            );
        }
    }

    Ok(())
}
