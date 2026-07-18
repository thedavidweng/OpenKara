//! IPC adapter for library integrity audit and cleanup commands.
//!
//! Domain logic lives in `crate::library::integrity`. This module only binds
//! Tauri state, opens the DB, and wraps the domain functions.

use crate::{
    audio::coordinator::PlaybackCommand,
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
///
/// After a successful commit, asks the PlaybackCoordinator to invalidate any
/// current/loading tracks that match deleted hashes (and clears CDG). A failed
/// DB transaction never touches playback state.
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

    // Only reconcile playback after a successful DB mutation. The database
    // commit is authoritative: a stopped coordinator must not report this
    // already-completed destructive action as failed to the caller.
    if !result.deleted_song_hashes.is_empty() {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let command = PlaybackCommand::InvalidateDeletedSongs {
            song_ids: result.deleted_song_hashes.clone(),
            reply: reply_tx,
        };
        if state.playback.command_tx.send(command).is_err() {
            tracing::warn!(
                "library integrity cleanup committed, but playback coordinator is unavailable"
            );
        } else if let Err(error) = reply_rx.await {
            tracing::warn!(
                ?error,
                "library integrity cleanup committed, but playback invalidation reply was lost"
            );
        }
    }

    Ok(result)
}
