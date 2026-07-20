//! Thin IPC adapters for the Remote Repository domain.
//!
//! Domain logic lives in `crate::remote`. This module only binds Tauri command
//! signatures and app-data path resolution to those domain entry points.

use crate::{
    commands::error::{internal_error, CommandResult},
    config::RemoteLibraryProvider,
    remote, AppState,
};
use tauri::{AppHandle, Manager, State};

pub use remote::{
    RemoteAuthSession, RemoteAuthStart, RemoteAuthState, RemoteAuthStatus, RemoteLibraryCandidate,
    UploadState, UploadStatusSnapshot,
};

/// Re-export the cache usage snapshot type so the IPC command signature stays
/// stable even if the internal module path changes.
pub use remote::cache_catalog::CacheUsage;

#[tauri::command]
pub fn begin_remote_auth(
    state: State<'_, AppState>,
    provider: RemoteLibraryProvider,
    payload: Option<serde_json::Value>,
) -> CommandResult<RemoteAuthStart> {
    remote::begin_remote_auth(&state, provider, payload)
}

#[tauri::command]
pub fn poll_remote_auth(
    state: State<'_, AppState>,
    session_id: String,
) -> CommandResult<RemoteAuthStatus> {
    remote::poll_remote_auth(&state, session_id)
}

#[tauri::command]
pub fn cancel_remote_auth(state: State<'_, AppState>, session_id: String) -> CommandResult<()> {
    remote::cancel_remote_auth(&state, session_id)
}

#[tauri::command]
pub fn open_external_url(url: String) -> CommandResult<()> {
    remote::open_external_url(url)
}

#[tauri::command]
pub fn list_remote_library_roots(
    state: State<'_, AppState>,
    session_id: String,
) -> CommandResult<Vec<RemoteLibraryCandidate>> {
    remote::list_remote_library_roots(&state, session_id)
}

#[tauri::command]
pub fn create_remote_library(
    state: State<'_, AppState>,
    session_id: String,
    display_name: String,
) -> CommandResult<RemoteLibraryCandidate> {
    remote::create_remote_library(&state, session_id, display_name)
}

#[tauri::command]
pub fn resolve_remote_library_candidate(
    state: State<'_, AppState>,
    session_id: String,
    display_name: String,
) -> CommandResult<RemoteLibraryCandidate> {
    remote::resolve_remote_library_candidate(&state, session_id, display_name)
}

#[tauri::command]
pub fn register_remote_library(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    session_id: String,
    remote_root_locator: String,
    display_name: Option<String>,
) -> CommandResult<crate::commands::library_setup::LibraryRegistrySnapshot> {
    let app_data_dir = app_handle.path().app_data_dir().map_err(internal_error)?;
    remote::register_remote_library(
        &state,
        &app_data_dir,
        session_id,
        remote_root_locator,
        display_name,
    )
}

#[tauri::command]
pub fn reauthorize_remote_library(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    library_id: String,
    session_id: String,
    remote_root_locator: String,
    display_name: String,
    allow_relocation: bool,
) -> CommandResult<crate::commands::library_setup::LibraryRegistrySnapshot> {
    let app_data_dir = app_handle.path().app_data_dir().map_err(internal_error)?;
    remote::reauthorize_remote_library(
        &state,
        &app_data_dir,
        library_id,
        session_id,
        remote_root_locator,
        display_name,
        allow_relocation,
    )
}

#[tauri::command]
pub fn mirror_local_library_to_remote(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    local_library_id: String,
    remote_library_id: String,
) -> CommandResult<()> {
    remote::mirror_local_library_to_remote(
        &state,
        &app_handle,
        &local_library_id,
        &remote_library_id,
    )
}

#[tauri::command]
pub fn sync_active_remote_library(state: State<'_, AppState>) -> CommandResult<()> {
    remote::sync_active_remote_library(&state)
}

#[tauri::command]
pub fn publish_song_to_remote(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_id: String,
) -> CommandResult<UploadStatusSnapshot> {
    remote::publish_song_to_remote(&state, &app_handle, song_id)
}

#[tauri::command]
pub fn publish_songs_to_remote(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_ids: Vec<String>,
) -> CommandResult<Vec<UploadStatusSnapshot>> {
    remote::publish_songs_to_remote(&state, &app_handle, song_ids)
}

#[tauri::command]
pub fn get_all_upload_statuses(
    state: State<'_, AppState>,
) -> CommandResult<Vec<UploadStatusSnapshot>> {
    remote::get_all_upload_statuses(&state)
}

/// Report remote streaming cache usage: total bytes used, the configured byte
/// limit, the number of catalog entries, and how many are currently pinned
/// (in use by playback and exempt from eviction).
#[tauri::command]
pub fn get_remote_cache_usage(state: State<'_, AppState>) -> CommandResult<CacheUsage> {
    let manager = state
        .remote
        .remote_chunk_cache
        .lock()
        .map_err(|_| internal_error("remote chunk cache manager lock was poisoned"))?;
    manager.usage()
}

/// Evict all unpinned remote cache entries. Pinned entries (files in active
/// use by playback) are left in the catalog until playback releases them, at
/// which point a subsequent clear or LRU eviction removes them. Returns the
/// number of entries evicted.
#[tauri::command]
pub fn clear_remote_cache(state: State<'_, AppState>) -> CommandResult<usize> {
    let mut manager = state
        .remote
        .remote_chunk_cache
        .lock()
        .map_err(|_| internal_error("remote chunk cache manager lock was poisoned"))?;
    manager.clear_unpinned()
}
