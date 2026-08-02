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
    library_root::LibraryRoot,
    remote, AppState,
};
use tauri::{AppHandle, Manager, State};

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
    let publication = remote::PublishChanges::new(&state, &app_handle);
    let applied = publication.apply(remote::Change::new(
        remote::ChangeScope::None,
        |connection: &rusqlite::Connection, library: &LibraryRoot| {
            Ok(import_songs_from_paths_with_options(
                connection,
                library,
                &paths,
                &options.unwrap_or_default(),
            ))
        },
        |result: &ImportSongsResult| {
            remote::ChangeScope::Songs(remote::song_ids_from_songs(&result.imported))
        },
    ))?;
    publication.publish(&applied.scope)?;
    let mut result = applied.value;
    absolutize_thumbnail_paths(&app_handle, &mut result.imported, &state.library_root()?);
    Ok(result)
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
        // SAFETY: the optional default path is a CString that outlives the
        // call, and `count` is a live local the picker writes the result length
        // into. Ownership of the returned array transfers to us, and is handed
        // back through openkara_free_import_paths below.
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
            // SAFETY: the picker reported `count` entries and the array was
            // null-checked above, so every index below `count` is in bounds.
            let raw_path = unsafe { *raw_paths.add(index) };
            if raw_path.is_null() {
                continue;
            }

            // SAFETY: null-checked above, and the picker returns
            // NUL-terminated strings that stay alive until the free call below.
            let path = unsafe { CStr::from_ptr(raw_path) }
                .to_string_lossy()
                .into_owned();
            collected_paths.push(path);
        }

        // SAFETY: frees the array the picker handed us, once, with the length
        // it reported. Nothing borrows it past this point - the paths were
        // copied into owned Strings above.
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

/// Rewrite the library-relative thumbnail path stored in the database into an
/// absolute one, and make sure the asset protocol may serve it.
///
/// This is the seam named on `Song::artwork_thumb_path`: the frontend feeds the
/// value to `convertFileSrc`, which only accepts absolute paths, while the
/// backend keeps paths relative so a library stays portable across machines.
///
/// Every command that hands a `Song` to the frontend must run this, not just
/// the two that feed the grid: a song returned by an edit command with a
/// still-relative path renders a failed image request before falling back.
///
/// The scope grant lives here rather than at library activation because this is
/// the one place the app promises "this path is loadable" — an activation path
/// added later cannot forget it. The grant is a `HashSet` insert, so repeating
/// it per call costs nothing. A failed grant is not fatal: the `<img>` request
/// is denied and `CoverArtThumbnail` falls back to reading the bytes over IPC.
fn absolutize_thumbnail_paths(app_handle: &AppHandle, songs: &mut [Song], library: &LibraryRoot) {
    let artwork_dir = library.artwork_dir();
    if let Err(error) = app_handle
        .asset_protocol_scope()
        .allow_directory(&artwork_dir, false)
    {
        tracing::warn!(
            "failed to grant asset protocol access to {}: {error}",
            artwork_dir.display()
        );
    }

    resolve_thumbnail_paths(songs, library);
}

fn resolve_thumbnail_paths(songs: &mut [Song], library: &LibraryRoot) {
    for song in songs {
        song.artwork_thumb_path = song
            .artwork_thumb_path
            .as_deref()
            .map(|relative| library.resolve(relative).to_string_lossy().into_owned());
    }
}

#[tauri::command]
pub fn get_library(app_handle: AppHandle, state: State<'_, AppState>) -> CommandResult<Vec<Song>> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;

    let mut songs = get_library_from_connection(&connection)
        .map_err(|error| database_error(error.to_string()))?;
    absolutize_thumbnail_paths(&app_handle, &mut songs, &library);
    Ok(songs)
}

#[tauri::command]
pub fn search_library(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    query: String,
) -> CommandResult<Vec<Song>> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;

    let mut songs = cache::search_songs(&connection, &query)
        .map_err(|error| database_error(error.to_string()))?;
    absolutize_thumbnail_paths(&app_handle, &mut songs, &library);
    Ok(songs)
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
    library: &LibraryRoot,
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
    let publication = remote::PublishChanges::new(&state, &app_handle);
    let applied = publication.apply(remote::Change::new(
        remote::ChangeScope::Songs(song_ids.clone()),
        |connection: &rusqlite::Connection, library: &LibraryRoot| {
            Ok(extract_embedded_cover_art_from_connection(
                connection, library, &song_ids,
            ))
        },
        |result: &ExtractEmbeddedCoverArtResult| {
            remote::ChangeScope::Songs(remote::song_ids_from_songs(&result.updated_songs))
        },
    ))?;
    publication.publish(&applied.scope)?;
    let mut result = applied.value;
    let library = state.library_root()?;
    absolutize_thumbnail_paths(&app_handle, &mut result.updated_songs, &library);
    Ok(result)
}

#[tauri::command]
pub fn update_song_metadata(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    hash: String,
    title: Option<String>,
    artist: Option<String>,
) -> CommandResult<Song> {
    let publication = remote::PublishChanges::new(&state, &app_handle);
    let applied = publication.apply(remote::Change::new(
        remote::ChangeScope::Songs(vec![hash.clone()]),
        |connection: &rusqlite::Connection, _library: &LibraryRoot| {
            update_song_metadata_in_connection(
                connection,
                &hash,
                title.as_deref(),
                artist.as_deref(),
            )
        },
        |song: &Song| remote::ChangeScope::Songs(vec![song.hash.clone()]),
    ))?;
    publication.publish(&applied.scope)?;
    let mut song = applied.value;
    absolutize_thumbnail_paths(
        &app_handle,
        std::slice::from_mut(&mut song),
        &state.library_root()?,
    );
    Ok(song)
}

#[tauri::command]
pub fn set_songs_instrumental(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_ids: Vec<String>,
    instrumental: bool,
) -> CommandResult<Vec<Song>> {
    let publication = remote::PublishChanges::new(&state, &app_handle);
    let applied = publication.apply(remote::Change::new(
        remote::ChangeScope::Songs(song_ids.clone()),
        |connection: &rusqlite::Connection, _library: &LibraryRoot| {
            set_songs_instrumental_in_connection(connection, &song_ids, instrumental)
        },
        |songs: &Vec<Song>| remote::ChangeScope::Songs(remote::song_ids_from_songs(songs)),
    ))?;
    publication.publish(&applied.scope)?;
    let mut songs = applied.value;
    let library = state.library_root()?;
    absolutize_thumbnail_paths(&app_handle, &mut songs, &library);
    Ok(songs)
}

#[tauri::command]
pub fn set_songs_language(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_ids: Vec<String>,
    language: Option<String>,
) -> CommandResult<Vec<Song>> {
    let publication = remote::PublishChanges::new(&state, &app_handle);
    let applied = publication.apply(remote::Change::new(
        remote::ChangeScope::Songs(song_ids.clone()),
        |connection: &rusqlite::Connection, _library: &LibraryRoot| {
            set_songs_language_in_connection(connection, &song_ids, language.as_deref())
        },
        |songs: &Vec<Song>| remote::ChangeScope::Songs(remote::song_ids_from_songs(songs)),
    ))?;
    publication.publish(&applied.scope)?;
    let mut songs = applied.value;
    let library = state.library_root()?;
    absolutize_thumbnail_paths(&app_handle, &mut songs, &library);
    Ok(songs)
}

#[tauri::command]
pub fn get_song_properties(
    state: State<'_, AppState>,
    song_id: String,
) -> CommandResult<SongProperties> {
    let library = state.library_root()?;
    let connection = cache::open_database(&library.database_path()).map_err(database_error)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn song_with_thumb(thumb: Option<&str>) -> Song {
        Song {
            hash: "song-1".to_owned(),
            file_path: Some("media/song-1.mp3".to_owned()),
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: "original".to_owned(),
            title: None,
            artist: None,
            album: None,
            duration_ms: 0,
            cover_art: None,
            has_cover_art: thumb.is_some(),
            artwork_thumb_path: thumb.map(str::to_owned),
            imported_at: 0,
            original_ext: Some("mp3".to_owned()),
        }
    }

    #[test]
    fn rewrites_stored_relative_thumbnails_to_absolute_paths() {
        let temp = tempfile::tempdir().expect("temp dir");
        let library = LibraryRoot::create(temp.path()).expect("library");
        let mut songs = vec![song_with_thumb(Some("artwork/thumb_abc_80.webp"))];

        resolve_thumbnail_paths(&mut songs, &library);

        let resolved = songs[0].artwork_thumb_path.as_deref().expect("path");
        assert_eq!(
            std::path::Path::new(resolved),
            library.resolve("artwork/thumb_abc_80.webp"),
        );
        assert!(std::path::Path::new(resolved).is_absolute());
    }

    #[test]
    fn leaves_songs_without_a_derivative_untouched() {
        // A song with no recorded derivative must stay `None`. Mapping it onto
        // the library root instead would hand the frontend a path to a
        // directory, and every such row would render a broken image before
        // falling back.
        let temp = tempfile::tempdir().expect("temp dir");
        let library = LibraryRoot::create(temp.path()).expect("library");
        let mut songs = vec![song_with_thumb(None)];

        resolve_thumbnail_paths(&mut songs, &library);

        assert!(songs[0].artwork_thumb_path.is_none());
    }
}
