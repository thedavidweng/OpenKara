use crate::{
    cache,
    commands::error::{database_error, CommandError, CommandResult},
    library::{artwork, error::LibraryError, Song},
    library_root::LibraryRoot,
    AppState,
};
use tauri::AppHandle;

use super::super::provider::create_provider;
use super::super::types::{
    load_app_config, load_remote_root, upsert_stem_entry, UploadState, UploadStatusSnapshot,
};

use super::file_ops::{copy_directory_recursive, copy_remote_song_assets};
use super::revision::{
    prepare_remote_database_for_mutation, resolve_active_remote, upload_remote_database,
};
use super::upload_status::{
    emit_upload_complete, emit_upload_error, emit_upload_progress, mark_upload_status,
};

/// Best-effort: missing/invalid derivatives are regenerated from the local
/// cover art bytes. Failures are logged but do not abort the publish.
fn publish_artwork_derivatives(
    local_connection: &rusqlite::Connection,
    local_root: &LibraryRoot,
    remote_root: &LibraryRoot,
    remote_connection: &rusqlite::Connection,
    provider: &dyn super::super::provider::RemoteProvider,
    song_id: &str,
    same_root: bool,
) -> CommandResult<()> {
    let record = cache::get_artwork_record(local_connection, song_id)
        .map_err(|error| database_error(error.to_string()))?;
    let Some(record) = record else {
        return Ok(());
    };
    let Some(cover_art) = record.cover_art.as_deref() else {
        // The original BLOB is the source of truth for a derivative. Do not
        // propagate DB paths that cannot be tied to an authoritative cover.
        return Ok(());
    };

    let digest = artwork::cover_sha256(cover_art);
    let expected_thumb = artwork::derivative_relative_path(artwork::ArtworkSize::Thumb, &digest);
    let expected_preview =
        artwork::derivative_relative_path(artwork::ArtworkSize::Preview, &digest);
    let recorded_paths_match_cover = record.artwork_thumb_path.as_deref()
        == Some(expected_thumb.as_str())
        && record.artwork_preview_path.as_deref() == Some(expected_preview.as_str());
    let derivatives_are_usable = recorded_paths_match_cover
        && artwork::read_artwork_derivative(local_root, &expected_thumb, artwork::THUMB_SIZE)
            .map(|bytes| bytes.is_some())
            .unwrap_or(false)
        && artwork::read_artwork_derivative(local_root, &expected_preview, artwork::PREVIEW_SIZE)
            .map(|bytes| bytes.is_some())
            .unwrap_or(false);

    // Regenerate if a path is missing, malformed, belongs to different cover
    // bytes, or points at an invalid WebP. The conditional database update
    // prevents a stale publisher from overwriting derivatives after concurrent
    // cover-art replacement.
    let (thumb_path, preview_path) = if derivatives_are_usable {
        (expected_thumb, expected_preview)
    } else {
        let derivatives = match artwork::generate_artwork_derivatives(local_root, cover_art) {
            Ok(derivatives) => derivatives,
            Err(e) => {
                tracing::warn!("artwork derivative generation failed for {song_id}: {e}");
                return Ok(());
            }
        };
        match cache::update_artwork_derivative_paths_if_cover_matches(
            local_connection,
            song_id,
            Some(&derivatives.thumb_path),
            Some(&derivatives.preview_path),
            cover_art,
        ) {
            Ok(true) => (derivatives.thumb_path, derivatives.preview_path),
            Ok(false) => {
                // The just-generated deterministic files may have no row left
                // referencing them. Delete only when reference counting proves
                // they are not shared by another song.
                for path in [&derivatives.thumb_path, &derivatives.preview_path] {
                    let _ = artwork::delete_artwork_derivative_if_unreferenced(
                        local_connection,
                        local_root,
                        path,
                    );
                }
                tracing::debug!("cover art changed while publishing derivatives for {song_id}");
                return Ok(());
            }
            Err(error) => {
                for path in [&derivatives.thumb_path, &derivatives.preview_path] {
                    let _ = artwork::delete_artwork_derivative_if_unreferenced(
                        local_connection,
                        local_root,
                        path,
                    );
                }
                tracing::warn!(
                    "failed to persist regenerated artwork derivatives for {song_id}: {error}"
                );
                return Ok(());
            }
        }
    };

    // Copy derivative files to the remote working copy (unless same
    // root) and upload to cloud storage. Persist the remote DB paths only
    // after both files are present remotely — never commit DB paths that
    // reference files omitted from the same publish operation.
    for (path, expected_size) in [
        (&thumb_path, artwork::THUMB_SIZE),
        (&preview_path, artwork::PREVIEW_SIZE),
    ] {
        if !same_root {
            if let Err(e) =
                artwork::copy_artwork_derivative(local_root, remote_root, path, expected_size)
            {
                tracing::warn!(
                    "failed to copy validated artwork derivative {path} to remote working copy: {e}"
                );
                return Ok(());
            }
        } else if artwork::read_artwork_derivative(local_root, path, expected_size)
            .map(|bytes| bytes.is_none())
            .unwrap_or(true)
        {
            tracing::warn!("artwork derivative source missing or invalid locally: {path}");
            return Ok(());
        }
        if let Err(e) = provider.upload_file(path) {
            tracing::warn!("failed to upload artwork derivative {path}: {}", e.message);
            return Ok(());
        }
    }

    // Both derivative files are present in the remote working copy and cloud —
    // safe to persist the paths in the remote DB.
    if let Err(error) = cache::update_artwork_derivative_paths(
        remote_connection,
        song_id,
        Some(&thumb_path),
        Some(&preview_path),
    ) {
        tracing::warn!("failed to persist remote artwork derivatives for {song_id}: {error}");
    }

    Ok(())
}

fn delete_remote_stem_cache_if_present(
    remote_connection: &rusqlite::Connection,
    remote_root: &LibraryRoot,
    song_id: &str,
) -> CommandResult<()> {
    if cache::stems::get_cached_stem_entry(remote_connection, song_id)
        .map_err(|error| database_error(error.to_string()))?
        .is_some()
    {
        cache::stems::delete_stem_cache_entry(remote_connection, remote_root, song_id)
            .map_err(|error| database_error(error.to_string()))?;
    }
    Ok(())
}

fn song_ready_for_remote_publish(
    connection: &rusqlite::Connection,
    library_root: &LibraryRoot,
    song: &Song,
) -> CommandResult<bool> {
    if !song.is_separable() {
        return Ok(true);
    }

    let Some(entry) = cache::stems::get_cached_stem_entry(connection, &song.hash)
        .map_err(|error| database_error(error.to_string()))?
    else {
        return Ok(false);
    };

    Ok(cache::stems::cache_entry_files_valid(library_root, &entry))
}

pub(crate) fn desired_remote_audio_source_kind(
    connection: &rusqlite::Connection,
    library_root: &LibraryRoot,
    song: &Song,
) -> CommandResult<Option<&'static str>> {
    if !song_ready_for_remote_publish(connection, library_root, song)? {
        return Ok(None);
    }

    Ok(Some(if song.is_separable() {
        "stems_remote"
    } else {
        "original_remote"
    }))
}

pub(crate) fn sync_song_lyrics_to_remote(
    local_connection: &rusqlite::Connection,
    remote_connection: &rusqlite::Connection,
    song_id: &str,
) -> CommandResult<()> {
    if let Some(entry) = cache::lyrics::get_lyrics_cache_entry(local_connection, song_id)
        .map_err(|error| database_error(error.to_string()))?
    {
        cache::lyrics::upsert_lyrics_cache_entry(remote_connection, &entry)
            .map_err(|error| database_error(error.to_string()))?;
    } else {
        remote_connection
            .execute("DELETE FROM lyrics WHERE song_hash = ?1", [song_id])
            .map_err(|error| database_error(error.to_string()))?;
    }
    Ok(())
}

pub(crate) fn update_remote_song(
    connection: &mut rusqlite::Connection,
    mut song: Song,
    remote_mode: &str,
) -> CommandResult<()> {
    let existing_cover = cache::get_artwork_record(connection, &song.hash)
        .map_err(|error| database_error(error.to_string()))?
        .and_then(|record| record.cover_art);
    let cover_changed = existing_cover.as_deref() != song.cover_art.as_deref();

    song.audio_source_kind = remote_mode.to_owned();
    if remote_mode == "stems_remote" {
        song.file_path = None;
    }
    // Updating cover_art without updating its paired derivative paths would
    // leave the remote DB pointing at stale artwork if a later upload fails.
    // Commit the song update and derivative-path invalidation together; the
    // paths are repopulated only after both derivative uploads succeed.
    let transaction = connection
        .transaction()
        .map_err(|error| database_error(error.to_string()))?;
    cache::upsert_song(&transaction, &song).map_err(|error| database_error(error.to_string()))?;
    if cover_changed {
        cache::update_artwork_derivative_paths(&transaction, &song.hash, None, None)
            .map_err(|error| database_error(error.to_string()))?;
    }
    transaction
        .commit()
        .map_err(|error| database_error(error.to_string()))?;
    Ok(())
}

pub(crate) fn maybe_publish_song_to_bound_remote<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_id: &str,
) -> CommandResult<()> {
    let config = load_app_config(&state.shell.app_data_dir)?;
    if resolve_active_remote(&config).is_none() {
        return Ok(());
    }

    let local_root = state.library_root()?;
    let local_connection = cache::open_database(&local_root.database_path())
        .map_err(|error| database_error(error.to_string()))?;
    let Some(song) = cache::get_song_by_hash(&local_connection, song_id)
        .map_err(|error| database_error(error.to_string()))?
    else {
        return Ok(());
    };

    if song_ready_for_remote_publish(&local_connection, &local_root, &song)? {
        let _ = publish_song_internal(state, app_handle, song_id)?;
    }

    Ok(())
}

pub(crate) fn maybe_publish_songs_to_bound_remote<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_ids: &[String],
) -> CommandResult<()> {
    for song_id in song_ids {
        maybe_publish_song_to_bound_remote(state, app_handle, song_id)?;
    }
    Ok(())
}

fn publish_song_internal<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_id: &str,
) -> CommandResult<UploadStatusSnapshot> {
    let config = load_app_config(&state.shell.app_data_dir)?;
    let remote_library = resolve_active_remote(&config).ok_or_else(|| {
        CommandError::from(LibraryError::Internal(
            "no bound remote repository is available for publishing".to_string(),
        ))
    })?;
    let remote_library =
        prepare_remote_database_for_mutation(&state.shell.app_data_dir, &remote_library)?;

    let local_root = state.library_root()?;
    let remote_library_id = remote_library.id().to_owned();
    let remote_root = load_remote_root(&state.shell.app_data_dir, &remote_library)?;

    // When the active library IS the remote repository (user is directly working
    // in a remote repository), local_root and remote_root point to the same
    // directory.  In that case the "copy to remote" step must be skipped —
    // `copy_directory_recursive` would delete the source before reading it,
    // destroying stems and media files.  The cloud upload reads from the
    // working copy via `RegisteredLibrary::working_copy_root()`, so it works
    // correctly regardless.
    let same_root = local_root.root() == remote_root.root();

    let local_connection = cache::open_database(&local_root.database_path())
        .map_err(|error| database_error(error.to_string()))?;
    let mut remote_connection = cache::open_database(&remote_root.database_path())
        .map_err(|error| database_error(error.to_string()))?;

    let song = cache::get_song_by_hash(&local_connection, song_id)
        .map_err(|error| database_error(error.to_string()))?
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(format!(
                "song {song_id} was not found"
            )))
        })?;

    let running = mark_upload_status(
        state,
        song_id,
        Some(remote_library_id.clone()),
        UploadState::Running,
        0,
        Some("Preparing remote publish".to_owned()),
        None,
    )?;
    emit_upload_progress(app_handle, &running);

    let provider = create_provider(&state.shell.app_data_dir, &remote_library)?;

    let publish_result = if song.is_separable() {
        let stem_entry = cache::stems::get_cached_stem_entry(&local_connection, song_id)
            .map_err(|error| database_error(error.to_string()))?
            .ok_or_else(|| {
                CommandError::from(LibraryError::Internal(format!(
                    "song {song_id} must have cached stems before publishing to a remote repository"
                )))
            })?;
        if !same_root {
            let source_stems_dir = local_root.resolve(&format!("stems/{song_id}"));
            let destination_stems_dir = remote_root.resolve(&format!("stems/{song_id}"));
            copy_directory_recursive(&source_stems_dir, &destination_stems_dir)?;
        }
        upsert_stem_entry(&remote_connection, &stem_entry)?;

        update_remote_song(&mut remote_connection, song.clone(), "stems_remote")?;
        provider.upload_directory(&format!("stems/{song_id}"))?;
        publish_artwork_derivatives(
            &local_connection,
            &local_root,
            &remote_root,
            &remote_connection,
            &*provider,
            song_id,
            same_root,
        )?;
        sync_song_lyrics_to_remote(&local_connection, &remote_connection, song_id)?;
        Ok::<_, CommandError>(())
    } else {
        if let Some(file_path) = song.file_path.as_deref() {
            if !same_root {
                copy_remote_song_assets(&local_root, &remote_root, file_path, file_path)?;
            }
            provider.upload_file(file_path)?;
        }
        if let Some(cdg_path) = song.cdg_path.as_deref() {
            if !same_root {
                copy_remote_song_assets(&local_root, &remote_root, cdg_path, cdg_path)?;
            }
            provider.upload_file(cdg_path)?;
        }

        delete_remote_stem_cache_if_present(&remote_connection, &remote_root, song_id)?;

        update_remote_song(&mut remote_connection, song.clone(), "original_remote")?;
        publish_artwork_derivatives(
            &local_connection,
            &local_root,
            &remote_root,
            &remote_connection,
            &*provider,
            song_id,
            same_root,
        )?;
        sync_song_lyrics_to_remote(&local_connection, &remote_connection, song_id)?;
        Ok(())
    };

    if let Err(error) = publish_result {
        let failure = mark_upload_status(
            state,
            song_id,
            Some(remote_library_id.clone()),
            UploadState::Failed,
            0,
            None,
            Some(error.clone()),
        )?;
        emit_upload_error(app_handle, &failure, error.clone());
        return Err(error);
    }

    let completed = mark_upload_status(
        state,
        song_id,
        Some(remote_library_id.clone()),
        UploadState::Completed,
        100,
        None,
        None,
    )?;
    emit_upload_complete(app_handle, &completed);

    upload_remote_database(&state.shell.app_data_dir, &remote_library)?;

    Ok(completed)
}

pub(crate) fn publish_song_to_remote<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_id: String,
) -> CommandResult<UploadStatusSnapshot> {
    let snapshot = mark_upload_status(
        state,
        &song_id,
        None,
        UploadState::Running,
        0,
        Some("Preparing remote publish".to_owned()),
        None,
    )?;
    emit_upload_progress(app_handle, &snapshot);

    let background_state = state.clone();
    let background_handle = app_handle.clone();
    let song_id = song_id.clone();
    std::thread::spawn(move || {
        let _ = publish_song_internal(&background_state, &background_handle, &song_id);
    });

    Ok(snapshot)
}

pub(crate) fn publish_songs_to_remote<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_ids: Vec<String>,
) -> CommandResult<Vec<UploadStatusSnapshot>> {
    let mut snapshots = Vec::with_capacity(song_ids.len());
    for song_id in &song_ids {
        let snapshot = mark_upload_status(
            state,
            song_id,
            None,
            UploadState::Running,
            0,
            Some("Preparing remote publish".to_owned()),
            None,
        )?;
        emit_upload_progress(app_handle, &snapshot);
        snapshots.push(snapshot);
    }

    let background_state = state.clone();
    let background_handle = app_handle.clone();
    let song_ids = song_ids.to_vec();
    std::thread::spawn(move || {
        for song_id in song_ids {
            let _ = publish_song_internal(&background_state, &background_handle, &song_id);
        }
    });

    Ok(snapshots)
}
