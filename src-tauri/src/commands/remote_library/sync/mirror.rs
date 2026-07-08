use crate::{
    cache,
    commands::error::{database_error, CommandError, CommandResult},
    config::RegisteredLibrary,
    library::error::LibraryError,
    library::Song,
    AppState,
};
use std::collections::HashMap;
use tauri::AppHandle;

use super::super::provider::create_provider;
use super::super::types::{load_app_config, load_remote_root, persist_app_config};

use super::publish::{desired_remote_audio_source_kind, maybe_publish_songs_to_bound_remote};
use super::revision::{
    load_registered_remote_library, prepare_remote_database_for_mutation, resolve_active_remote,
    upload_remote_database,
};

pub(crate) fn remote_delete_relative_path(
    app_data_dir: &std::path::Path,
    library: &RegisteredLibrary,
    relative_path: &str,
) -> CommandResult<()> {
    let provider = create_provider(app_data_dir, library)?;
    provider.delete_path(relative_path)
}

pub(crate) fn sync_bound_remote_for_active_local_library<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
) -> CommandResult<()> {
    let config = load_app_config(&state.shell.app_data_dir)?;
    let Some(remote_library) = resolve_active_remote(&config) else {
        return Ok(());
    };
    sync_bound_remote(state, app_handle, &remote_library)
}

/// Core mirror sync logic. Takes the remote library directly so callers don't
/// need to mutate `active_library_id` in the config — avoiding a crash-window
/// where the config is left pointing at the remote library.
fn sync_bound_remote<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    remote_library: &RegisteredLibrary,
) -> CommandResult<()> {
    let remote_library =
        prepare_remote_database_for_mutation(&state.shell.app_data_dir, remote_library)?;

    let local_root = state.library_root()?;
    let local_connection = cache::open_database(&local_root.database_path())
        .map_err(|error| database_error(error.to_string()))?;
    let remote_root = load_remote_root(&state.shell.app_data_dir, &remote_library)?;
    let mut remote_connection = cache::open_database(&remote_root.database_path())
        .map_err(|error| database_error(error.to_string()))?;
    let local_songs =
        cache::list_songs(&local_connection).map_err(|error| database_error(error.to_string()))?;
    let remote_songs =
        cache::list_songs(&remote_connection).map_err(|error| database_error(error.to_string()))?;

    let mut desired_kinds = HashMap::new();
    for song in &local_songs {
        if let Some(kind) = desired_remote_audio_source_kind(&local_connection, &local_root, song)?
        {
            desired_kinds.insert(song.hash.clone(), kind);
        }
    }

    // Collect songs to delete from the remote mirror. DB deletes are wrapped
    // in a transaction so a mid-loop failure rolls back all prior DB deletes,
    // keeping the remote database consistent. Cloud file deletes happen after
    // the transaction commits — if they fail, the result is orphaned cloud
    // files (wasted storage) rather than DB entries pointing at missing files.
    // We also pre-collect which songs have stem entries so we can delete cloud
    // stem files in phase 2 (the DB row will be gone by then).
    let songs_to_delete: Vec<&Song> = remote_songs
        .iter()
        .filter(|remote_song| match desired_kinds.get(&remote_song.hash) {
            Some(kind) if remote_song.audio_source_kind == *kind => false,
            Some(_) | None => true,
        })
        .collect();

    // Pre-collect which songs have stem cache entries (before the transaction
    // deletes the DB rows). Used in phase 2 to decide whether to delete cloud
    // stem directories.
    let mut has_stem_entry: std::collections::HashSet<String> = std::collections::HashSet::new();
    for song in &songs_to_delete {
        if song.is_remote_stems()
            || cache::stems::get_cached_stem_entry(&remote_connection, &song.hash)
                .map_err(|error| database_error(error.to_string()))?
                .is_some()
        {
            has_stem_entry.insert(song.hash.clone());
        }
    }

    // Phase 1: transactional DB deletes.
    if !songs_to_delete.is_empty() {
        let tx = remote_connection
            .transaction()
            .map_err(|error| database_error(error.to_string()))?;
        for song in &songs_to_delete {
            crate::library::delete_song_rows_from_database(
                &tx,
                &remote_root,
                &song.hash,
            )
            .map_err(|error| CommandError::from(LibraryError::Internal(error.to_string())))?;
        }
        tx.commit()
            .map_err(|error| database_error(error.to_string()))?;
    }

    // Phase 2: best-effort cloud + working-copy file deletes (after DB
    // transaction commits). Failures here leave orphaned files (wasted
    // storage) but don't corrupt the database — the next sync will retry.
    for song in &songs_to_delete {
        if let Some(file_path) = song.file_path.as_deref() {
            let _ =
                remote_delete_relative_path(&state.shell.app_data_dir, &remote_library, file_path);
        }
        if let Some(cdg_path) = song.cdg_path.as_deref() {
            let _ =
                remote_delete_relative_path(&state.shell.app_data_dir, &remote_library, cdg_path);
        }
        // Delete cloud stems if the song had a stem entry (pre-collected
        // before the transaction deleted the DB row).
        if has_stem_entry.contains(&song.hash) {
            let _ = remote_delete_relative_path(
                &state.shell.app_data_dir,
                &remote_library,
                &format!("stems/{}", song.hash),
            );
        }
        // Best-effort working-copy file cleanup (audio, CDG, media_g).
        let _ = crate::library::delete_song_files_from_working_copy(&remote_root, song);
        // Best-effort working-copy stem directory cleanup.
        let _ = crate::library::delete_stem_files_from_working_copy(&remote_root, &song.hash);
    }

    let desired_song_ids: Vec<String> = local_songs
        .into_iter()
        .filter_map(|song| desired_kinds.contains_key(&song.hash).then_some(song.hash))
        .collect();
    maybe_publish_songs_to_bound_remote(state, app_handle, &desired_song_ids)?;
    let remote_library =
        load_registered_remote_library(&state.shell.app_data_dir, remote_library.id())?;
    upload_remote_database(&state.shell.app_data_dir, &remote_library)?;
    Ok(())
}

pub fn mirror_local_library_to_remote<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    local_library_id: &str,
    remote_library_id: &str,
) -> CommandResult<()> {
    let mut config = load_app_config(&state.shell.app_data_dir)?;
    let Some(local_library) = config
        .libraries
        .iter()
        .find(|entry| entry.id() == local_library_id)
    else {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "local library {local_library_id} was not found"
        ))));
    };
    if !matches!(local_library, RegisteredLibrary::Local { .. }) {
        return Err(CommandError::from(LibraryError::Internal(
            "the source library must be a local library".to_owned(),
        )));
    }

    let Some(remote_library) = config
        .libraries
        .iter()
        .find(|entry| entry.id() == remote_library_id)
    else {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "remote repository {remote_library_id} was not found"
        ))));
    };
    if !matches!(remote_library, RegisteredLibrary::Remote { .. }) {
        return Err(CommandError::from(LibraryError::Internal(
            "the target library must be a remote repository".to_owned(),
        )));
    }

    // Temporarily swap active_library_id to the remote library so that
    // resolve_active_remote (used by publish_song_internal) finds it during
    // the sync. Store the original in pending_mirror_restore_active_library_id
    // and set pending_mirror_restore=true so startup can recover if the app
    // crashes mid-sync. The boolean flag is necessary because the original
    // active_library_id may be None (unset), which would be
    // indistinguishable from "no pending operation" without the flag.
    let original_active_library_id = config.active_library_id.clone();
    config.active_library_id = Some(remote_library_id.to_owned());
    config.pending_mirror_restore = true;
    config.pending_mirror_restore_active_library_id = original_active_library_id.clone();
    persist_app_config(&state.shell.app_data_dir, &config)?;

    let sync_result = sync_bound_remote_for_active_local_library(state, app_handle);

    // Restore the original active_library_id and clear the pending marker.
    let mut restore_config = load_app_config(&state.shell.app_data_dir)?;
    restore_config.active_library_id = original_active_library_id;
    restore_config.pending_mirror_restore = false;
    restore_config.pending_mirror_restore_active_library_id = None;
    persist_app_config(&state.shell.app_data_dir, &restore_config)?;

    sync_result
}
