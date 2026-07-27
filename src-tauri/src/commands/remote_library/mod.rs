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

/// Diagnostic snapshot of the remote repository state for the active remote
/// library. Returned by `get_remote_diagnostics` so the frontend (PR #8) can
/// surface repository health, generation, conflict status, and recent
/// operation outcomes in the settings diagnostics panel.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RemoteDiagnostics {
    /// `true` when an active remote library is registered.
    pub has_active_remote: bool,
    /// Stable repository UUID from the manifest protocol (PR #4). `None` for
    /// legacy repositories or local libraries.
    pub repository_id: Option<String>,
    /// Stable writer (installation) UUID. For diagnostics only.
    pub writer_id: Option<String>,
    /// Committed remote generation (monotonically increasing). 0 when no
    /// publication has completed yet.
    pub committed_generation: i64,
    /// Local base generation — the generation the local working copy was last
    /// synced from.
    pub local_base_generation: i64,
    /// Local repository cleanliness state. `Clean`, `Dirty`, or `Conflicted`.
    pub local_state: String,
    /// SHA-256 digest of the local working database, if known.
    pub local_db_digest: Option<String>,
    /// Active operation ID, if a publish/GC is in progress.
    pub active_operation_id: Option<String>,
    /// Wall-clock ms of the last successful publication.
    pub last_success_at_ms: Option<i64>,
    /// Last error code (e.g. `remote_conflict`, `network_unavailable`).
    pub last_error_code: Option<String>,
    /// Recent operations (most recent first, capped at 20).
    pub recent_operations: Vec<RemoteOperationDiagnostic>,
}

/// Diagnostic view of a single remote operation row.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RemoteOperationDiagnostic {
    pub operation_id: String,
    pub operation_kind: String,
    pub state: String,
    pub expected_generation: Option<i64>,
    pub target_generation: Option<i64>,
    pub attempt_count: i64,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

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

/// Take one of the two exits from a Pre-Publish Conflict.
///
/// `keep_local` rebases the pending local changes onto the winning remote
/// generation and republishes them; the executor refuses it when both sides
/// touched the same songs, because an automatic merge there would silently
/// pick a winner. `use_remote` discards the pending operation and adopts the
/// remote database.
#[tauri::command]
pub fn resolve_remote_conflict(
    state: State<'_, AppState>,
    resolution: remote::ConflictResolution,
) -> CommandResult<()> {
    remote::resolve_active_remote_conflict(&state, resolution)
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

/// Return a diagnostic snapshot of the remote repository state for the active
/// remote library. When no remote library is active, returns a snapshot with
/// `has_active_remote: false` and all other fields zeroed/empty. The frontend
/// (PR #8) renders this in the settings diagnostics panel so the user can
/// inspect repository health, generation, conflict status, and recent
/// operation outcomes.
#[tauri::command]
pub fn get_remote_diagnostics(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> CommandResult<RemoteDiagnostics> {
    use crate::remote::active_remote_library;
    use crate::remote::control_db;

    let app_data_dir = app_handle.path().app_data_dir().map_err(internal_error)?;

    let active = active_remote_library(&app_data_dir)?;
    let library_id = match &active {
        Some(lib) => lib.id().to_owned(),
        None => {
            return Ok(RemoteDiagnostics {
                has_active_remote: false,
                repository_id: None,
                writer_id: None,
                committed_generation: 0,
                local_base_generation: 0,
                local_state: "clean".to_owned(),
                local_db_digest: None,
                active_operation_id: None,
                last_success_at_ms: None,
                last_error_code: None,
                recent_operations: Vec::new(),
            });
        }
    };

    let conn = state
        .remote
        .control_db
        .lock()
        .map_err(|_| internal_error("control DB lock was poisoned"))?;

    let repo_state = control_db::get_repository_state(&conn, &library_id)?;

    let (
        committed_generation,
        local_base_generation,
        local_state,
        local_db_digest,
        repository_id,
        writer_id,
        active_operation_id,
        last_success_at_ms,
        last_error_code,
    ) = match repo_state {
        Some(row) => (
            row.committed_generation,
            row.local_base_generation,
            row.local_state.as_str().to_owned(),
            row.local_db_digest,
            row.repository_id,
            row.writer_id,
            row.active_operation_id,
            row.last_success_at_ms,
            row.last_error_code,
        ),
        None => (0, 0, "clean".to_owned(), None, None, None, None, None, None),
    };

    // Load recent operations for this library (most recent first, capped at 20).
    let mut ops = control_db::list_operations_for_library(&conn, &library_id)?;
    // Sort by updated_at_ms descending (newest first).
    ops.sort_by_key(|op| std::cmp::Reverse(op.updated_at_ms));
    ops.truncate(20);
    let recent_operations = ops
        .into_iter()
        .map(|op| RemoteOperationDiagnostic {
            operation_id: op.operation_id,
            operation_kind: op.operation_kind.as_str().to_owned(),
            state: op.state.as_str().to_owned(),
            expected_generation: op.expected_generation,
            target_generation: op.target_generation,
            attempt_count: op.attempt_count,
            error_code: op.error_code,
            error_detail: op.error_detail,
            created_at_ms: op.created_at_ms,
            updated_at_ms: op.updated_at_ms,
        })
        .collect();

    Ok(RemoteDiagnostics {
        has_active_remote: true,
        repository_id,
        writer_id,
        committed_generation,
        local_base_generation,
        local_state,
        local_db_digest,
        active_operation_id,
        last_success_at_ms,
        last_error_code,
        recent_operations,
    })
}
