//! IPC adapter for library import / song write commands.
//!
//! Domain write path lives in `crate::library`. This module only binds
//! Tauri state, opens the DB, and wraps remote mutation hooks.

// Re-export domain types/functions so existing `commands::import::…` paths
// (tests, smoke) keep working during the transition.
pub use crate::library::import::{
    collect_expandable_import_paths, extract_embedded_cover_art_from_connection,
    get_library_from_connection, import_songs_from_paths, import_songs_from_paths_with_options,
    inspect_import_candidate, DeleteSongsFailure, DeleteSongsResult, ExpandedImportPaths,
    ExtractEmbeddedCoverArtFailure, ExtractEmbeddedCoverArtResult, ImportCandidateDetails,
    ImportSongsOptions, SongProperties,
};
pub use crate::library::songs::{
    delete_songs as delete_songs_in_library, set_songs_instrumental as set_songs_instrumental_in_connection,
    set_songs_language as set_songs_language_in_connection,
    update_song_metadata as update_song_metadata_in_connection,
};

use crate::{
    cache,
    commands::error::{database_error, state_lock_error, CommandError, CommandResult},
    remote,
    library::{error::LibraryError, ImportSongsResult, Song},
    AppState,
};
use tauri::{AppHandle, State};

#[cfg(target_os = "macos")]
use std::ffi::{c_char, CStr, CString};

#[tauri::command]
pub fn import_songs(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    paths: Vec<String>,
    options: Option<ImportSongsOptions>,
) -> CommandResult<ImportSongsResult> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;

    // Remote Pre-Mutation Refresh / Publish Song: run_imported_songs_mutation
    remote::run_imported_songs_mutation(&state, &app_handle, || {
        import_songs_from_paths_with_options(
            &connection,
            &library,
            &paths,
            &options.unwrap_or_default(),
        )
    })
}

#[tauri::command]
pub fn get_import_candidate_details(
    paths: Vec<String>,
) -> CommandResult<Vec<ImportCandidateDetails>> {
    paths
        .into_iter()
        .map(|raw_path| {
            inspect_import_candidate(&raw_path).map_err(|error| {
                CommandError::from(LibraryError::MediaReadFailed(error.to_string()))
            })
        })
        .collect()
}

#[tauri::command]
pub fn expand_import_paths(paths: Vec<String>) -> CommandResult<ExpandedImportPaths> {
    Ok(collect_expandable_import_paths(&paths))
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn openkara_pick_import_paths(
        default_path: *const c_char,
        count_out: *mut usize,
    ) -> *mut *mut c_char;
    fn openkara_free_import_paths(paths: *mut *mut c_char, count: usize);
}

#[tauri::command]
pub fn pick_import_paths(default_path: Option<String>) -> CommandResult<Vec<String>> {
    #[cfg(target_os = "macos")]
    {
        let default_path = default_path
            .as_deref()
            .map(CString::new)
            .transpose()
            .map_err(|error| CommandError::from(LibraryError::Internal(error.to_string())))?;
        let mut count = 0usize;
        let raw_paths = unsafe {
            openkara_pick_import_paths(
                default_path
                    .as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
                &mut count,
            )
        };

        if raw_paths.is_null() || count == 0 {
            return Ok(Vec::new());
        }

        let mut collected_paths = Vec::with_capacity(count);
        for index in 0..count {
            let raw_path = unsafe { *raw_paths.add(index) };
            if raw_path.is_null() {
                continue;
            }

            let path = unsafe { CStr::from_ptr(raw_path) }
                .to_string_lossy()
                .into_owned();
            collected_paths.push(path);
        }

        unsafe {
            openkara_free_import_paths(raw_paths, count);
        }

        Ok(collected_paths)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = default_path;
        Err(CommandError::from(LibraryError::Internal(
            "mixed file and folder selection is only available on macOS".to_string(),
        )))
    }
}

#[tauri::command]
pub fn get_library(state: State<'_, AppState>) -> CommandResult<Vec<Song>> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;

    get_library_from_connection(&connection).map_err(|error| database_error(error.to_string()))
}

#[tauri::command]
pub fn search_library(state: State<'_, AppState>, query: String) -> CommandResult<Vec<Song>> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;

    cache::search_songs(&connection, &query).map_err(|error| database_error(error.to_string()))
}

#[tauri::command]
pub fn get_cover_art(state: State<'_, AppState>, hash: String) -> CommandResult<Option<Vec<u8>>> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;

    cache::get_cover_art(&connection, &hash).map_err(|error| database_error(error.to_string()))
}

#[tauri::command]
pub fn delete_songs(
    state: State<'_, AppState>,
    song_ids: Vec<String>,
) -> CommandResult<DeleteSongsResult> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;
    let current_song_id = {
        let playback = state
            .playback
            .playback
            .lock()
            .map_err(|_| state_lock_error("playback controller lock was poisoned"))?;
        playback.current_song_id().map(|value| value.to_owned())
    };

    let result = delete_songs_in_library(&connection, &library, &song_ids);

    if current_song_id
        .as_deref()
        .is_some_and(|song_id| result.deleted_song_ids.iter().any(|deleted| deleted == song_id))
    {
        {
            let mut playback = state
                .playback
                .playback
                .lock()
                .map_err(|_| state_lock_error("playback controller lock was poisoned"))?;
            playback.clear_track();
        }
        let mut cdg_state = state
            .playback
            .cdg_state
            .lock()
            .map_err(|_| state_lock_error("CDG state lock was poisoned"))?;
        *cdg_state = None;
    }

    Ok(result)
}

#[tauri::command]
pub fn extract_embedded_cover_art(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_ids: Vec<String>,
) -> CommandResult<ExtractEmbeddedCoverArtResult> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;

    remote::run_updated_songs_mutation(
        &state,
        &app_handle,
        || {
            Ok(extract_embedded_cover_art_from_connection(
                &connection,
                &library,
                &song_ids,
            ))
        },
        |result| remote::song_ids_from_songs(&result.updated_songs),
    )
}

#[tauri::command]
pub fn update_song_metadata(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    hash: String,
    title: Option<String>,
    artist: Option<String>,
) -> CommandResult<Song> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;

    remote::run_song_database_mutation(&state, &app_handle, &hash, || {
        update_song_metadata_in_connection(
            &connection,
            &hash,
            title.as_deref(),
            artist.as_deref(),
        )
    })
}

#[tauri::command]
pub fn set_songs_instrumental(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_ids: Vec<String>,
    instrumental: bool,
) -> CommandResult<Vec<Song>> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;

    remote::run_database_then_library_mirror_mutation(&state, &app_handle, || {
        set_songs_instrumental_in_connection(&connection, &song_ids, instrumental)
    })
}

#[tauri::command]
pub fn set_songs_language(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_ids: Vec<String>,
    language: Option<String>,
) -> CommandResult<Vec<Song>> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;

    remote::run_database_then_library_mirror_mutation(&state, &app_handle, || {
        set_songs_language_in_connection(&connection, &song_ids, language.as_deref())
    })
}

#[tauri::command]
pub fn get_song_properties(
    state: State<'_, AppState>,
    song_id: String,
) -> CommandResult<SongProperties> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;

    // Ensure remote working-copy files exist before probing (command-layer only).
    let song = cache::get_song_by_hash(&connection, &song_id)
        .map_err(|e| database_error(e.to_string()))?
        .ok_or_else(|| database_error(format!("song with hash {song_id} not found")))?;

    if song.is_remote() {
        if let Some(song_path) = song.file_path.as_deref() {
            remote::ensure_remote_file_cached(&state.shell.app_data_dir, song_path)?;
        }
        if let Some(cdg_path) = song.cdg_path.as_deref() {
            remote::ensure_remote_file_cached(&state.shell.app_data_dir, cdg_path)?;
        }
    }

    crate::library::songs::get_song_properties(&connection, &library, &song_id)
}
