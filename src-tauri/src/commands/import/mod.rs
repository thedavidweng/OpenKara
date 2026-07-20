//! IPC adapter for library import / song write commands.
//!
//! Domain write path lives in `crate::library`. This module only binds
//! Tauri state, opens the DB, and wraps remote mutation hooks.

pub use crate::library::import::{
    collect_expandable_import_paths, extract_embedded_cover_art_from_connection,
    get_library_from_connection, import_songs_from_paths, import_songs_from_paths_with_options,
    inspect_import_candidate, DeleteSongsFailure, DeleteSongsResult, ExpandedImportPaths,
    ExtractEmbeddedCoverArtFailure, ExtractEmbeddedCoverArtResult, ImportCandidateDetails,
    ImportSongsOptions, SongProperties,
};
pub use crate::library::songs::{
    delete_songs as delete_songs_in_library,
    set_songs_instrumental as set_songs_instrumental_in_connection,
    set_songs_language as set_songs_language_in_connection,
    update_song_metadata as update_song_metadata_in_connection,
};

use crate::{
    cache,
    commands::error::{database_error, state_lock_error, CommandError, CommandResult},
    library::{artwork, error::LibraryError, ImportSongsResult, Song},
    remote, AppState,
};
use tauri::{AppHandle, State};

#[cfg(target_os = "macos")]
use crate::commands::error::internal_error;

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
            .map_err(internal_error)?;
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

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CoverArtSize {
    Thumb,
    Preview,
    Original,
}

/// Remove a newly generated pair when the conditional DB write did not take
/// effect. Content-addressed paths can be shared, so removal is always gated
/// by a fresh reference count rather than by assuming the current request owns
/// the files.
fn discard_unpersisted_artwork_derivatives(
    connection: &rusqlite::Connection,
    library: &crate::library_root::LibraryRoot,
    derivatives: &artwork::ArtworkDerivatives,
) {
    for path in [&derivatives.thumb_path, &derivatives.preview_path] {
        let _ = artwork::delete_artwork_derivative_if_unreferenced(connection, library, path);
    }
}

#[tauri::command]
pub async fn get_cover_art(
    state: State<'_, AppState>,
    hash: String,
    size: Option<CoverArtSize>,
) -> CommandResult<Option<Vec<u8>>> {
    let library = state.library_root()?;
    let database_path = library.database_path();
    let size = size.unwrap_or(CoverArtSize::Original);

    // Disk/decode work runs off the async runtime thread with a freshly
    // opened library DB connection so the IPC thread is never blocked.
    let bytes = tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<Option<Vec<u8>>> {
        let connection = cache::open_database(&database_path)?;
        let Some(record) = cache::get_artwork_record(&connection, &hash)? else {
            return Ok(None);
        };

        match size {
            CoverArtSize::Original => {
                // Lazy repair: regenerate missing derivatives when the
                // original cover art is read. Non-fatal — the original is
                // authoritative and returned regardless.
                if let Some(cover_art) = record.cover_art.as_deref() {
                    let needs_repair = record.artwork_thumb_path.is_none()
                        || record.artwork_preview_path.is_none();
                    if needs_repair {
                        if let Ok(derivatives) =
                            artwork::generate_artwork_derivatives(&library, cover_art)
                        {
                            match cache::update_artwork_derivative_paths_if_cover_matches(
                                &connection,
                                &hash,
                                Some(&derivatives.thumb_path),
                                Some(&derivatives.preview_path),
                                cover_art,
                            ) {
                                Ok(true) => {}
                                Ok(false) => discard_unpersisted_artwork_derivatives(
                                    &connection,
                                    &library,
                                    &derivatives,
                                ),
                                Err(error) => {
                                    discard_unpersisted_artwork_derivatives(
                                        &connection,
                                        &library,
                                        &derivatives,
                                    );
                                    tracing::warn!(
                                        "failed to persist lazily repaired artwork for {hash}: {error}"
                                    );
                                }
                            }
                        }
                    }
                }
                Ok(cache::get_cover_art(&connection, &hash)?)
            }
            CoverArtSize::Thumb | CoverArtSize::Preview => {
                let (expected_size, recorded_path) = match size {
                    CoverArtSize::Thumb => {
                        (artwork::THUMB_SIZE, record.artwork_thumb_path.as_deref())
                    }
                    CoverArtSize::Preview => (
                        artwork::PREVIEW_SIZE,
                        record.artwork_preview_path.as_deref(),
                    ),
                    _ => unreachable!(),
                };

                if let Some(path) = recorded_path {
                    if let Ok(Some(bytes)) =
                        artwork::read_artwork_derivative(&library, path, expected_size)
                    {
                        return Ok(Some(bytes));
                    }
                }

                // Lazy repair: regenerate both derivatives from the
                // original bytes, then update paths only if the cover art
                // BLOB still matches (concurrent replacement safe).
                let Some(cover_art) = record.cover_art.as_deref() else {
                    return Ok(None);
                };
                match artwork::generate_artwork_derivatives(&library, cover_art) {
                    Ok(derivatives) => {
                        let persisted = cache::update_artwork_derivative_paths_if_cover_matches(
                            &connection,
                            &hash,
                            Some(&derivatives.thumb_path),
                            Some(&derivatives.preview_path),
                            cover_art,
                        );
                        match persisted {
                            Ok(true) => {}
                            Ok(false) => discard_unpersisted_artwork_derivatives(
                                &connection,
                                &library,
                                &derivatives,
                            ),
                            Err(error) => {
                                discard_unpersisted_artwork_derivatives(
                                    &connection,
                                    &library,
                                    &derivatives,
                                );
                                tracing::warn!(
                                    "failed to persist lazily repaired artwork for {hash}: {error}"
                                );
                            }
                        }
                        let target = match size {
                            CoverArtSize::Thumb => &derivatives.thumb_path,
                            CoverArtSize::Preview => &derivatives.preview_path,
                            _ => unreachable!(),
                        };
                        if let Ok(Some(bytes)) =
                            artwork::read_artwork_derivative(&library, target, expected_size)
                        {
                            return Ok(Some(bytes));
                        }
                    }
                    Err(e) => {
                        tracing::warn!("artwork derivative generation failed for {hash}: {e}");
                    }
                }

                Ok(cache::get_cover_art(&connection, &hash)?)
            }
        }
    })
    .await
    .map_err(|e| {
        CommandError::from(LibraryError::Internal(format!(
            "cover art task failed: {e}"
        )))
    })?
    .map_err(|e| {
        CommandError::from(LibraryError::Internal(format!(
            "cover art task failed: {e}"
        )))
    })?;

    Ok(bytes)
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

    if current_song_id.as_deref().is_some_and(|song_id| {
        result
            .deleted_song_ids
            .iter()
            .any(|deleted| deleted == song_id)
    }) {
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
        update_song_metadata_in_connection(&connection, &hash, title.as_deref(), artist.as_deref())
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
        .map_err(database_error)?
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
