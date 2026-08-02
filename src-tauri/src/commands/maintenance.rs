use crate::{
    cache,
    commands::error::{database_error, internal_error, CommandResult},
    remote, AppState,
};
use serde::Serialize;
use tauri::{AppHandle, State};

#[derive(Debug, Serialize)]
pub struct DeleteStemsResult {
    pub deleted_count: usize,
    pub freed_bytes: u64,
}

#[tauri::command]
pub fn delete_all_stems(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> CommandResult<DeleteStemsResult> {
    let library_root = state.library_root()?;

    let freed_bytes = cache::stems::estimate_stems_disk_usage(&library_root)
        .map_err(|e| internal_error(format!("failed to estimate stems disk usage: {e}")))?;

    let publication = remote::PublishChanges::new(&state, &app_handle);
    let applied = publication.apply(remote::Change::new(
        remote::ChangeScope::WholeRepository,
        |connection: &rusqlite::Connection, library: &crate::library_root::LibraryRoot| {
            let deleted_count = cache::stems::delete_all_stem_cache_entries(connection, library)
                .map_err(|e| internal_error(format!("failed to delete all stems: {e}")))?;

            if let Ok(mut statuses) = state.separation.separation_statuses.lock() {
                statuses.clear();
            }

            Ok(deleted_count)
        },
        |_: &usize| remote::ChangeScope::WholeRepository,
    ))?;
    publication.publish(&applied.scope)?;
    let deleted_count = applied.value;

    Ok(DeleteStemsResult {
        deleted_count,
        freed_bytes,
    })
}

#[tauri::command]
pub fn estimate_stems_size(state: State<'_, AppState>) -> CommandResult<u64> {
    let library_root = state.library_root()?;
    cache::stems::estimate_stems_disk_usage(&library_root)
        .map_err(|e| internal_error(format!("failed to estimate stems disk usage: {e}")))
}

#[derive(Debug, Serialize)]
pub struct DowngradeResult {
    pub downgraded_count: usize,
    pub freed_bytes: u64,
}

#[tauri::command]
pub fn downgrade_all_to_two_stem(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> CommandResult<DowngradeResult> {
    let publication = remote::PublishChanges::new(&state, &app_handle);
    let applied = publication.apply(remote::Change::new(
        remote::ChangeScope::WholeRepository,
        |connection: &rusqlite::Connection, library: &crate::library_root::LibraryRoot| {
            let (downgraded_count, freed_bytes) =
                cache::stems::batch_downgrade_to_two_stem(connection, library)
                    .map_err(|e| internal_error(format!("failed to downgrade stems: {e}")))?;

            if let Ok(mut statuses) = state.separation.separation_statuses.lock() {
                for status in statuses.values_mut() {
                    if status.drums_path.is_some() {
                        let accomp_path = format!("stems/{}/accompaniment.ogg", status.song_id);
                        status.accomp_path = Some(accomp_path);
                        status.drums_path = None;
                        status.bass_path = None;
                        status.other_path = None;
                    }
                }
            }
            Ok((downgraded_count, freed_bytes))
        },
        |_: &(usize, u64)| remote::ChangeScope::WholeRepository,
    ))?;
    publication.publish(&applied.scope)?;
    let (downgraded_count, freed_bytes) = applied.value;

    Ok(DowngradeResult {
        downgraded_count,
        freed_bytes,
    })
}

#[tauri::command]
pub fn estimate_downgrade_savings(state: State<'_, AppState>) -> CommandResult<u64> {
    let library_root = state.library_root()?;
    let connection = cache::open_database(&library_root.database_path()).map_err(database_error)?;

    cache::stems::estimate_downgrade_savings(&connection, &library_root)
        .map_err(|e| internal_error(format!("failed to estimate downgrade savings: {e}")))
}

#[tauri::command]
pub fn delete_all_cached_lyrics(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> CommandResult<usize> {
    let publication = remote::PublishChanges::new(&state, &app_handle);
    let applied = publication.apply(remote::Change::new(
        remote::ChangeScope::WholeRepository,
        |connection: &rusqlite::Connection, _library: &crate::library_root::LibraryRoot| {
            cache::lyrics::delete_all_lyrics_cache_entries(connection).map_err(database_error)
        },
        |_: &usize| remote::ChangeScope::WholeRepository,
    ))?;
    publication.publish(&applied.scope)?;
    let deleted = applied.value;
    Ok(deleted)
}
