use crate::{
    cache,
    commands::error::{database_error, CommandError, CommandResult},
    config::RegisteredLibrary,
    library::{error::LibraryError, Song},
    library_root::LibraryRoot,
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

fn delete_remote_song_from_mirror(
    app_data_dir: &std::path::Path,
    remote_library: &RegisteredLibrary,
    remote_root: &LibraryRoot,
    remote_connection: &rusqlite::Connection,
    song: &Song,
) -> CommandResult<()> {
    if let Some(file_path) = song.file_path.as_deref() {
        remote_delete_relative_path(app_data_dir, remote_library, file_path)?;
    }
    if let Some(cdg_path) = song.cdg_path.as_deref() {
        remote_delete_relative_path(app_data_dir, remote_library, cdg_path)?;
    }
    if song.is_remote_stems()
        || cache::stems::get_cached_stem_entry(remote_connection, &song.hash)
            .map_err(|error| database_error(error.to_string()))?
            .is_some()
    {
        remote_delete_relative_path(
            app_data_dir,
            remote_library,
            &format!("stems/{}", song.hash),
        )?;
    }
    crate::commands::import::delete::delete_song_from_library(
        remote_connection,
        remote_root,
        &song.hash,
    )
    .map_err(|error| CommandError::from(LibraryError::Internal(error.to_string())))?;
    Ok(())
}

pub(crate) fn sync_bound_remote_for_active_local_library<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
) -> CommandResult<()> {
    let config = load_app_config(&state.shell.app_data_dir)?;
    let Some(remote_library) = resolve_active_remote(&config) else {
        return Ok(());
    };
    let remote_library =
        prepare_remote_database_for_mutation(&state.shell.app_data_dir, &remote_library)?;

    let local_root = state.library_root()?;
    let local_connection = cache::open_database(&local_root.database_path())
        .map_err(|error| database_error(error.to_string()))?;
    let remote_root = load_remote_root(&state.shell.app_data_dir, &remote_library)?;
    let remote_connection = cache::open_database(&remote_root.database_path())
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

    for remote_song in &remote_songs {
        match desired_kinds.get(&remote_song.hash) {
            Some(kind) if remote_song.audio_source_kind == *kind => {}
            Some(_) | None => {
                delete_remote_song_from_mirror(
                    &state.shell.app_data_dir,
                    &remote_library,
                    &remote_root,
                    &remote_connection,
                    remote_song,
                )?;
            }
        }
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

    let original_active_library_id = config.active_library_id.clone();
    config.active_library_id = Some(remote_library_id.to_owned());
    persist_app_config(&state.shell.app_data_dir, &config)?;

    let sync_result = sync_bound_remote_for_active_local_library(state, app_handle);

    let mut restore_config = load_app_config(&state.shell.app_data_dir)?;
    restore_config.active_library_id = original_active_library_id;
    persist_app_config(&state.shell.app_data_dir, &restore_config)?;

    sync_result
}
