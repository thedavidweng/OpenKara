use crate::{
    audio::coordinator::PlaybackCommand,
    cache,
    commands::error::{database_error, internal_error, CommandResult},
    library::integrity,
    AppState,
};
use tauri::{async_runtime, State};

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
