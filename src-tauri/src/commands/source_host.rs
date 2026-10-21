use crate::catalog;
use crate::commands::catalog::{app_data_dir, cancel_authentication, host_error, source};
use crate::commands::error::{internal_error, state_lock_error, CommandResult};
use crate::source_host::{SourceHost, SourceSnapshot};
use crate::AppState;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub fn list_streaming_sources(app_handle: AppHandle) -> Vec<SourceSnapshot> {
    app_handle.state::<SourceHost>().snapshots()
}
#[tauri::command]
pub async fn refresh_streaming_sources(
    app_handle: AppHandle,
) -> CommandResult<Vec<SourceSnapshot>> {
    app_handle
        .state::<SourceHost>()
        .refresh()
        .await
        .map_err(host_error)
}
#[tauri::command]
pub async fn install_streaming_source(
    app_handle: AppHandle,
    source_id: String,
    version: Option<String>,
) -> CommandResult<SourceSnapshot> {
    app_handle
        .state::<SourceHost>()
        .install(&source_id, version.as_deref())
        .await
        .map_err(host_error)
}
#[tauri::command]
pub fn cancel_streaming_source_install(
    app_handle: AppHandle,
    source_id: String,
) -> CommandResult<()> {
    app_handle
        .state::<SourceHost>()
        .cancel_install(&source_id)
        .map_err(host_error)
}
#[tauri::command]
pub fn pin_streaming_source(
    app_handle: AppHandle,
    source_id: String,
    version: Option<String>,
) -> CommandResult<SourceSnapshot> {
    app_handle
        .state::<SourceHost>()
        .set_pin(&source_id, version.as_deref())
        .map_err(host_error)
}
#[tauri::command]
pub async fn set_streaming_source_enabled(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    source_id: String,
    enabled: bool,
    stop_work: Option<bool>,
) -> CommandResult<SourceSnapshot> {
    if !enabled && stop_work.unwrap_or(false) {
        stop_source_work(&state, &app_handle, &source_id).await?;
    }
    app_handle
        .state::<SourceHost>()
        .set_enabled(&source_id, enabled, stop_work.unwrap_or(false))
        .await
        .map_err(host_error)
}
#[tauri::command]
pub async fn uninstall_streaming_source(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    source_id: String,
    stop_work: Option<bool>,
    remove_credentials: bool,
) -> CommandResult<SourceSnapshot> {
    if stop_work.unwrap_or(false) {
        stop_source_work(&state, &app_handle, &source_id).await?;
    }
    let snapshot = app_handle
        .state::<SourceHost>()
        .uninstall(&source_id, stop_work.unwrap_or(false))
        .await
        .map_err(host_error)?;
    if remove_credentials {
        catalog::clear_session(&app_data_dir(&app_handle)?, &source_id)
            .map_err(|_| internal_error("could not remove saved source credentials"))?;
    }
    Ok(snapshot)
}
pub(crate) async fn stop_source_work(
    state: &AppState,
    app: &AppHandle,
    source_id: &str,
) -> CommandResult<()> {
    cancel_authentication(app, source_id).await?;
    stop_import_work(state, app, source_id).await
}
pub(crate) async fn stop_import_work(
    state: &AppState,
    app: &AppHandle,
    source_id: &str,
) -> CommandResult<()> {
    let job = state
        .catalog
        .import_job
        .lock()
        .map_err(|_| state_lock_error("import job lock"))?
        .clone();
    if let Some(job) = job.filter(|job| {
        job.snapshot().source_id == source_id
            && job.snapshot().phase != catalog::StreamingImportJobPhase::Completed
    }) {
        job.cancel();
        let state = state.clone();
        let app = app.clone();
        let id = source_id.to_owned();
        tauri::async_runtime::spawn_blocking(move || -> CommandResult<()> {
            let source = source(&app, &id)?;
            crate::commands::catalog::settle_cancelled_import(&state, &app, &source, &job)
        })
        .await
        .map_err(|_| internal_error("could not cancel paused source import"))??;
    }
    Ok(())
}
