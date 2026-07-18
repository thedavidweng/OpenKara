//! IPC adapter for library integrity audit and cleanup commands.
//!
//! Domain logic lives in `crate::library::integrity`. This module only binds
//! Tauri state, opens the DB, and wraps the domain functions.

use crate::{
    cache,
    commands::error::{database_error, internal_error, CommandResult},
    library::integrity,
    AppState,
};
use tauri::{async_runtime, State};

/// Audit the active managed library for missing/empty referenced files and
/// unreferenced managed files. Returns a deterministic, sorted report.
#[tauri::command]
pub async fn check_library_integrity(
    state: State<'_, AppState>,
) -> CommandResult<integrity::IntegrityReport> {
    let library = state.library_root()?;
    let report =
        async_runtime::spawn_blocking(move || integrity::check_library_integrity(&library))
            .await
            .map_err(|e| internal_error(format!("audit task failed: {e}")))?
            .map_err(|e| internal_error(format!("integrity audit failed: {e}")))?;
    Ok(report)
}

/// Remove database entries for songs whose primary media is missing or empty.
/// Revalidates each song at mutation time in a single transaction.
#[tauri::command]
pub async fn remove_missing_library_entries(
    state: State<'_, AppState>,
    hashes: Vec<String>,
) -> CommandResult<integrity::IntegrityCleanupResult> {
    let library = state.library_root()?;
    let result = async_runtime::spawn_blocking(move || -> anyhow::Result<_> {
        let connection = cache::open_database(&library.database_path())?;
        integrity::remove_missing_library_entries(&connection, &library, hashes)
    })
    .await
    .map_err(|e| internal_error(format!("cleanup task failed: {e}")))?
    .map_err(|e| database_error(format!("integrity cleanup failed: {e}")))?;
    Ok(result)
}
