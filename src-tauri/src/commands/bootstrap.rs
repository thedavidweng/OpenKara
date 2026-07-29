use crate::{
    commands::error::{
        internal_error, model_bootstrap_error, state_lock_error, CommandError, CommandResult,
    },
    config::{self, ModelVariant},
    separator, AppState,
};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

pub const MODEL_BOOTSTRAP_PROGRESS_EVENT: &str = "model-bootstrap-progress";
pub const MODEL_BOOTSTRAP_READY_EVENT: &str = "model-bootstrap-ready";
pub const MODEL_BOOTSTRAP_ERROR_EVENT: &str = "model-bootstrap-error";

/// Process-wide set of variants currently being downloaded. Prevents
/// concurrent `download_model` calls for the same variant from spawning
/// duplicate download tasks — the second call returns the current
/// downloading status instead of starting a second fetch.
static DOWNLOADS_IN_PROGRESS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn downloads_in_progress() -> &'static Mutex<HashSet<String>> {
    DOWNLOADS_IN_PROGRESS.get_or_init(|| Mutex::new(HashSet::new()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelBootstrapState {
    Pending,
    Downloading,
    Outdated,
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
        ModelBootstrapState::Pending
        | ModelBootstrapState::Downloading
        | ModelBootstrapState::Outdated => MODEL_BOOTSTRAP_PROGRESS_EVENT,
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
        ModelBootstrapState::Outdated => Err(model_bootstrap_error(format!(
            "installed model at {} does not match the current release; open Settings to delete it and download the update",
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
    ensure_active_model_ready_or_install_blocking_with_resolution(app_data_dir, status, emit, true)
}

pub fn ensure_active_managed_model_ready_or_install_blocking(
    app_data_dir: &Path,
    status: &Arc<Mutex<ModelBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, ModelBootstrapStatusSnapshot),
) -> CommandResult<std::path::PathBuf> {
    ensure_active_model_ready_or_install_blocking_with_resolution(app_data_dir, status, emit, false)
}

fn ensure_active_model_ready_or_install_blocking_with_resolution(
    app_data_dir: &Path,
    status: &Arc<Mutex<ModelBootstrapStatusSnapshot>>,
    emit: &mut impl FnMut(&'static str, ModelBootstrapStatusSnapshot),
    allow_development_fallback: bool,
) -> CommandResult<std::path::PathBuf> {
    let active_variant = config::load_config(app_data_dir)
        .ok()
        .flatten()
        .map(|config| config.effective_model_variant())
        .unwrap_or_default();
    let descriptor = separator::bootstrap::descriptor_for(active_variant);
    let managed_path = separator::bootstrap::managed_model_path_for(app_data_dir, descriptor);
    let dev_path = separator::model::default_model_path_for_filename(&descriptor.filename);

    let resolution = if allow_development_fallback {
        separator::bootstrap::resolve_model_installation(
            &managed_path,
            &dev_path,
            &descriptor.file_sha256,
        )
    } else {
        separator::bootstrap::resolve_managed_model_installation(
            &managed_path,
            &descriptor.file_sha256,
        )
    };

    let resolution = resolution
        .map_err(|error| internal_error(format!("failed to inspect model status: {error}")))?;

    match resolution {
        separator::bootstrap::ModelInstallationResolution::Ready(resolved) => {
            let snapshot = ready_status(resolved.path.display().to_string());
            if let Ok(mut current) = status.lock() {
                *current = snapshot.clone();
            }
            emit(MODEL_BOOTSTRAP_READY_EVENT, snapshot);
            Ok(resolved.path)
        }
        separator::bootstrap::ModelInstallationResolution::LegacyManaged(_) => {
            Err(model_bootstrap_error(
                "installed model does not match the pinned release; open Settings to delete it and download the update",
            ))
        }
        separator::bootstrap::ModelInstallationResolution::Absent => {
            let initial = downloading_status(managed_path.display().to_string(), 0, None);
            if let Ok(mut current) = status.lock() {
                *current = initial.clone();
            }
            emit(MODEL_BOOTSTRAP_PROGRESS_EVENT, initial);

            let progress_path = managed_path.display().to_string();
            let download_result = separator::bootstrap::download_and_install_model(
                &managed_path,
                descriptor,
                |downloaded_bytes, total_bytes| {
                    let snapshot = downloading_status(
                        progress_path.clone(),
                        downloaded_bytes,
                        total_bytes,
                    );
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
    let active_variant = config::load_config(app_data_dir)
        .map_err(|error| internal_error(format!("failed to load config: {error}")))?
        .unwrap_or_default()
        .effective_model_variant();
    let descriptor = separator::bootstrap::descriptor_for(active_variant);
    let development_model_path =
        separator::model::default_model_path_for_filename(&descriptor.filename);
    let startup = crate::derive_startup_model_bootstrap(
        app_data_dir,
        &development_model_path,
        active_variant,
        &descriptor.file_sha256,
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

pub fn outdated_status(model_path: impl Into<String>) -> ModelBootstrapStatusSnapshot {
    ModelBootstrapStatusSnapshot {
        state: ModelBootstrapState::Outdated,
        model_path: model_path.into(),
        downloaded_bytes: None,
        total_bytes: None,
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
    pub legacy_install_present: bool,
    pub file_size_bytes: Option<u64>,
    pub installed_version: Option<String>,
    pub pinned_version: String,
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
        &descriptor.file_sha256,
    )
    .map_err(|error| internal_error(format!("failed to inspect model status: {error}")))?;
    let (downloaded, legacy_install_present) = match resolved {
        separator::bootstrap::ModelInstallationResolution::Ready(_) => (true, false),
        separator::bootstrap::ModelInstallationResolution::LegacyManaged(_) => (false, true),
        separator::bootstrap::ModelInstallationResolution::Absent => (false, false),
    };
    let file_size = separator::bootstrap::model_file_size(&app_data_dir, model_variant);
    let installed_version = if downloaded {
        separator::catalog::read_installed_identity(&model_path)
            .map(|identity| identity.upstream_version)
            .or_else(|| Some(descriptor.upstream_tag.clone()))
    } else {
        None
    };
    Ok(ModelStatusSnapshot {
        variant,
        downloaded,
        legacy_install_present,
        file_size_bytes: file_size,
        installed_version,
        pinned_version: descriptor.upstream_tag.clone(),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelUpdateCheckSnapshot {
    pub variant: String,
    pub state: separator::catalog::ModelUpdateState,
    pub installed_version: Option<String>,
    pub available_version: String,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelUpdateReport {
    pub generation: u64,
    pub release_id: String,
    pub models: Vec<ModelUpdateCheckSnapshot>,
}

fn download_descriptor_for(
    catalog_cache: &Arc<Mutex<Option<separator::catalog::VerifiedCatalog>>>,
    variant: ModelVariant,
) -> CommandResult<separator::bootstrap::ModelDescriptor> {
    let embedded = separator::bootstrap::descriptor_for(variant);
    let cache = catalog_cache
        .lock()
        .map_err(|_| state_lock_error("catalog cache lock was poisoned"))?;
    if let Some(catalog) = cache.as_ref() {
        if catalog.generation > separator::catalog::embedded_catalog().generation {
            return separator::bootstrap::descriptor_from_catalog(catalog, variant)
                .map_err(|error| internal_error(format!("failed to resolve model: {error}")));
        }
    }
    Ok(embedded.clone())
}

#[tauri::command]
pub async fn check_model_updates(state: State<'_, AppState>) -> CommandResult<ModelUpdateReport> {
    let app_data_dir = state.shell.app_data_dir.clone();
    let catalog_cache = Arc::clone(&state.shell.catalog_cache);

    let report =
        tauri::async_runtime::spawn_blocking(move || -> CommandResult<ModelUpdateReport> {
            let catalog = separator::catalog::fetch_stable_catalog()
                .map_err(|error| model_bootstrap_error(format!("update check failed: {error}")))?;

            let mut models = Vec::new();
            for variant in [ModelVariant::Htdemucs, ModelVariant::HtdemucsFt] {
                let catalog_model = separator::catalog::resolve_model(&catalog.manifest, variant)
                    .map_err(|error| {
                    model_bootstrap_error(format!("update check failed: {error}"))
                })?;
                let descriptor = separator::bootstrap::descriptor_for(variant);
                let model_path =
                    separator::bootstrap::managed_model_path_for(&app_data_dir, descriptor);
                let installed = separator::catalog::read_installed_identity(&model_path);
                // A file that matches the embedded pin but predates identity
                // records is a known install of the pinned release.
                let installed = installed.or_else(|| {
                    let matches_pin = separator::bootstrap::resolve_existing_model_path(
                        &model_path,
                        &app_data_dir.join("__no_dev_fallback_model__"),
                        &descriptor.file_sha256,
                    )
                    .ok()
                    .flatten()
                    .is_some();
                    matches_pin.then(|| descriptor.identity.clone())
                });
                let comparison = separator::catalog::compare_installed_model(
                    installed,
                    catalog_model,
                    &catalog,
                    model_path.exists(),
                )
                .map_err(|error| model_bootstrap_error(format!("update check failed: {error}")))?;

                models.push(ModelUpdateCheckSnapshot {
                    variant: variant.as_str().to_owned(),
                    state: comparison.state,
                    installed_version: comparison
                        .installed
                        .map(|identity| identity.upstream_version),
                    available_version: catalog_model.upstream.tag.clone(),
                    available_bytes: catalog_model.byte_size,
                });
            }

            let report = ModelUpdateReport {
                generation: catalog.generation,
                release_id: catalog.release_id.clone(),
                models,
            };

            if let Ok(mut cache) = catalog_cache.lock() {
                *cache = Some(catalog);
            }

            Ok(report)
        })
        .await
        .map_err(|error| internal_error(format!("update check task failed: {error}")))??;

    Ok(report)
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
    let descriptor = download_descriptor_for(&state.shell.catalog_cache, model_variant)?;
    let model_path =
        separator::bootstrap::managed_model_path_for(&state.shell.app_data_dir, &descriptor);
    let should_publish_status = is_active_variant(&state.shell.app_data_dir, model_variant);

    // Refuse implicit downgrades: an explicit user download must never
    // replace a verified newer install with an older catalog artifact.
    if let Some(installed) = separator::catalog::read_installed_identity(&model_path) {
        if installed.generation > descriptor.identity.generation {
            return Err(model_bootstrap_error(format!(
                "installed model {} is from catalog generation {}, newer than the available generation {}; downgrades require deleting the model first",
                installed.upstream_version, installed.generation, descriptor.identity.generation
            )));
        }
    }

    // The download targets one exact artifact. "Already installed" means the
    // managed file matches THAT artifact's digest — an identity-verified
    // older install must not short-circuit an update download.
    if separator::bootstrap::model_matches_digest(&model_path, &descriptor.file_sha256)
        .map_err(|error| internal_error(format!("failed to inspect model status: {error}")))?
    {
        return Ok(ready_status(model_path.display().to_string()));
    }

    // Guard against concurrent download calls for the same variant. Without
    // this, a second call that arrives before the first download finishes
    // would pass the `resolve_existing_model_path` check (the file doesn't
    // exist yet) and spawn a duplicate download task.
    {
        let mut in_progress = downloads_in_progress()
            .lock()
            .map_err(|_| state_lock_error("downloads-in-progress lock was poisoned"))?;
        if in_progress.contains(&variant) {
            return Ok(downloading_status(
                model_path.display().to_string(),
                0,
                None,
            ));
        }
        in_progress.insert(variant.clone());
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

    let task_descriptor = descriptor.clone();
    let progress_path = model_path.display().to_string();
    let should_publish_status_for_task = should_publish_status;
    let task_variant = model_variant;
    let task_app_data_dir = state.shell.app_data_dir.clone();
    let task_variant_key = variant.clone();

    tauri::async_runtime::spawn(async move {
        let blocking_status = Arc::clone(&status);
        let blocking_app_handle = app_handle.clone();
        let blocking_model_path = model_path.clone();
        let progress_path = progress_path.clone();
        let blocking_app_data_dir = task_app_data_dir.clone();

        let result = tauri::async_runtime::spawn_blocking(move || {
            separator::bootstrap::download_and_install_model(
                &blocking_model_path,
                &task_descriptor,
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

        if let Ok(mut in_progress) = downloads_in_progress().lock() {
            in_progress.remove(&task_variant_key);
        }

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
                descriptor,
                Arc::clone(&state.shell.model_bootstrap_status),
            );
        }
    }

    Ok(())
}
