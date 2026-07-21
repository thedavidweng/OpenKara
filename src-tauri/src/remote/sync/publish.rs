use crate::{
    cache,
    commands::error::{database_error, CommandError, CommandResult},
    config::RegisteredLibrary,
    library::{artwork, error::LibraryError, Song},
    library_root::LibraryRoot,
    remote::control_db::{
        get_repository_state, upsert_operation, upsert_repository_state, LocalState, OperationKind,
        OperationPayload, OperationRow, OperationState, RepositoryStateRow,
    },
    remote::executor::{
        execute_publish, generate_repository_id, generate_writer_id, PublishContext,
    },
    AppState,
};
use tauri::AppHandle;

use super::super::provider::create_provider;
use super::super::types::{
    load_app_config, load_remote_root, upsert_stem_entry, UploadState, UploadStatusSnapshot,
};

use super::file_ops::{copy_directory_recursive, copy_remote_song_assets};
use super::revision::{prepare_remote_database_for_mutation, resolve_active_remote};
use super::upload_status::{
    emit_upload_complete, emit_upload_error, emit_upload_progress, mark_upload_status,
    mark_upload_status_for_operation, project_upload_status_from_operation,
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

#[allow(dead_code)]
pub(crate) fn maybe_publish_song_to_bound_remote<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_id: &str,
) -> CommandResult<()> {
    maybe_publish_songs_to_bound_remote(state, app_handle, &[song_id.to_owned()], None)
}

/// Publish songs for a bound remote. When `operation_id` is `Some`, that exact
/// durable identity is used for status, assets, and executor — never guessed
/// via library+song. When `None`, a fresh non-terminal operation is created
/// (explicit re-publish). Multi-song ops upload every song then CAS once.
pub(crate) fn maybe_publish_songs_to_bound_remote<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_ids: &[String],
    operation_id: Option<&str>,
) -> CommandResult<()> {
    if song_ids.is_empty() {
        return Ok(());
    }
    let config = load_app_config(&state.shell.app_data_dir)?;
    if resolve_active_remote(&config).is_none() {
        return Ok(());
    }

    let local_root = state.library_root()?;
    let local_connection = cache::open_database(&local_root.database_path())
        .map_err(|error| database_error(error.to_string()))?;

    let mut ready_ids = Vec::new();
    for song_id in song_ids {
        let Some(song) = cache::get_song_by_hash(&local_connection, song_id)
            .map_err(|error| database_error(error.to_string()))?
        else {
            continue;
        };
        if song_ready_for_remote_publish(&local_connection, &local_root, &song)? {
            ready_ids.push(song_id.clone());
        }
    }
    if ready_ids.is_empty() {
        return Ok(());
    }

    // Background publication uses the exact operation identity when known.
    let op_id = operation_id.map(|s| s.to_owned());
    let background_state = state.clone();
    let background_handle = app_handle.clone();
    std::thread::spawn(move || {
        let _ = publish_operation_internal(
            &background_state,
            &background_handle,
            op_id.as_deref(),
            &ready_ids,
        );
    });
    Ok(())
}

/// Publish by exact durable operation identity (or create one for explicit
/// re-publish). All song_ids in the operation are uploaded under one commit
/// lock, then a single candidate freeze + CAS runs. Never reopens terminal
/// operations.
fn publish_operation_internal<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    operation_id: Option<&str>,
    fallback_song_ids: &[String],
) -> CommandResult<UploadStatusSnapshot> {
    // Compatibility path: single-song callers without an operation_id.
    let primary_song = fallback_song_ids.first().map(|s| s.as_str()).unwrap_or("");
    publish_song_internal(
        state,
        app_handle,
        primary_song,
        operation_id,
        fallback_song_ids,
    )
}

fn publish_song_internal<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_id: &str,
    operation_id: Option<&str>,
    batch_song_ids: &[String],
) -> CommandResult<UploadStatusSnapshot> {
    let config = load_app_config(&state.shell.app_data_dir)?;
    let remote_library = resolve_active_remote(&config).ok_or_else(|| {
        CommandError::from(LibraryError::Internal(
            "no bound remote repository is available for publishing".to_string(),
        ))
    })?;
    let remote_library_id = remote_library.id().to_owned();

    // Serialize the FULL publication transaction for this library: remote
    // working-DB mutation, asset staging/upload, candidate freeze, CAS, and
    // local completion. Acquiring the lock only around the executor left a
    // window where two publishers could interleave working-DB writes.
    if state.remote.control_db_degraded {
        return Err(CommandError::from(LibraryError::Internal(
            "remote control database is unavailable; publication is disabled \
             until the control plane is repaired"
                .to_string(),
        )));
    }
    let commit_lock = state.remote.commit_lock(&remote_library_id);
    let _commit_guard = commit_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let remote_library = {
        let control_db_conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        prepare_remote_database_for_mutation(
            &control_db_conn,
            &state.shell.app_data_dir,
            &remote_library,
        )?
    };

    let local_root = state.library_root()?;
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

    // Resolve the exact durable operation identity. Prefer the caller's
    // operation_id; never reopen a terminal row. Multi-song batches keep one
    // identity for upload-all + single CAS.
    let resolved_op = resolve_or_create_publish_operation(
        state,
        &remote_library_id,
        operation_id,
        batch_song_ids,
        song_id,
    )?;
    let operation_id = resolved_op.operation_id.clone();
    let song_ids_to_publish = {
        let payload = OperationPayload::from_json(&resolved_op.payload_json).unwrap_or_default();
        if payload.song_ids.is_empty() {
            if batch_song_ids.is_empty() {
                vec![song_id.to_owned()]
            } else {
                batch_song_ids.to_vec()
            }
        } else {
            payload.song_ids
        }
    };
    let ui_song_id = song_ids_to_publish
        .first()
        .map(|s| s.as_str())
        .unwrap_or(song_id);

    let running = mark_upload_status_for_operation(
        state,
        &operation_id,
        ui_song_id,
        Some(remote_library_id.clone()),
        UploadState::Running,
        0,
        Some("Preparing remote publish".to_owned()),
        None,
    )?;
    emit_upload_progress(app_handle, &running);

    let provider = create_provider(&state.shell.app_data_dir, &remote_library)?;

    // Upload every song in the durable payload under the same commit lock,
    // then freeze/CAS once. Do not split a batch into per-song commits.
    let mut publish_result: CommandResult<()> = Ok(());
    for sid in &song_ids_to_publish {
        if let Err(error) = upload_one_song_assets(
            &local_connection,
            &mut remote_connection,
            &local_root,
            &remote_root,
            &*provider,
            sid,
            same_root,
        ) {
            publish_result = Err(error);
            break;
        }
    }

    if let Err(error) = publish_result {
        // Asset stage failed before the executor. Retryable network faults
        // land as durable RetryWait on this exact operation_id.
        let failure = mark_upload_status_for_operation(
            state,
            &operation_id,
            ui_song_id,
            Some(remote_library_id.clone()),
            UploadState::Failed,
            0,
            None,
            Some(error.clone()),
        )?;
        emit_upload_error(app_handle, &failure, error.clone());
        return Err(error);
    }

    // --- Transactional manifest commit via the executor ---
    //
    // `upload-complete` is emitted ONLY after the manifest CAS succeeds.
    // Failure is persisted by the executor; UI only projects durable state.
    let commit_result = commit_via_executor(
        state,
        &remote_library,
        &remote_library_id,
        &operation_id,
        &remote_root,
    );

    match commit_result {
        Ok(()) => {
            let completed = mark_upload_status_for_operation(
                state,
                &operation_id,
                ui_song_id,
                Some(remote_library_id.clone()),
                UploadState::Completed,
                100,
                None,
                None,
            )?;
            emit_upload_complete(app_handle, &completed);
            // Emit complete for each song in the batch so UI clears all rows.
            for sid in song_ids_to_publish.iter().skip(1) {
                let snap = UploadStatusSnapshot {
                    song_id: sid.clone(),
                    state: UploadState::Completed,
                    percent: 100,
                    remote_library_id: Some(remote_library_id.clone()),
                    detail: None,
                    error: None,
                };
                emit_upload_complete(app_handle, &snap);
            }
            Ok(completed)
        }
        Err(error) => {
            let failure = project_upload_status_from_operation(
                state,
                &operation_id,
                ui_song_id,
                Some(&error),
            )?;
            emit_upload_error(app_handle, &failure, error.clone());
            Err(error)
        }
    }
}

/// Upload assets and update the remote working DB for one song. Shared by
/// single-song and multi-song batch publication under one commit lock.
fn upload_one_song_assets(
    local_connection: &rusqlite::Connection,
    remote_connection: &mut rusqlite::Connection,
    local_root: &LibraryRoot,
    remote_root: &LibraryRoot,
    provider: &dyn super::super::provider::RemoteProvider,
    song_id: &str,
    same_root: bool,
) -> CommandResult<()> {
    let song = cache::get_song_by_hash(local_connection, song_id)
        .map_err(|error| database_error(error.to_string()))?
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(format!(
                "song {song_id} was not found"
            )))
        })?;

    if song.is_separable() {
        let stem_entry = cache::stems::get_cached_stem_entry(local_connection, song_id)
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
        upsert_stem_entry(remote_connection, &stem_entry)?;
        update_remote_song(remote_connection, song.clone(), "stems_remote")?;
        provider.upload_directory(&format!("stems/{song_id}"))?;
        publish_artwork_derivatives(
            local_connection,
            local_root,
            remote_root,
            remote_connection,
            provider,
            song_id,
            same_root,
        )?;
        sync_song_lyrics_to_remote(local_connection, remote_connection, song_id)?;
    } else {
        if let Some(file_path) = song.file_path.as_deref() {
            if !same_root {
                copy_remote_song_assets(local_root, remote_root, file_path, file_path)?;
            }
            provider.upload_file(file_path)?;
        }
        if let Some(cdg_path) = song.cdg_path.as_deref() {
            if !same_root {
                copy_remote_song_assets(local_root, remote_root, cdg_path, cdg_path)?;
            }
            provider.upload_file(cdg_path)?;
        }
        delete_remote_stem_cache_if_present(remote_connection, remote_root, song_id)?;
        update_remote_song(remote_connection, song.clone(), "original_remote")?;
        publish_artwork_derivatives(
            local_connection,
            local_root,
            remote_root,
            remote_connection,
            provider,
            song_id,
            same_root,
        )?;
        sync_song_lyrics_to_remote(local_connection, remote_connection, song_id)?;
    }
    Ok(())
}

/// Commit the remote database via the transactional manifest executor.
///
/// This replaces the legacy `upload_remote_database` call with the 13-step
/// publication protocol: candidate DB copy → integrity check → upload →
/// manifest CAS → verify. The operation row is created or found in the
/// control DB, and the executor drives it through the state machine.
///
/// On a CAS conflict, the repository transitions to `Conflicted` and the
/// error is returned so the caller can emit `upload-error`. The operation is
/// NEVER retried as an unconditional overwrite.
/// Resolve an exact non-terminal operation by id, or create a fresh one for
/// explicit re-publish. Never reopens Completed/Failed/Conflicted/Cancelled.
fn resolve_or_create_publish_operation(
    state: &AppState,
    remote_library_id: &str,
    operation_id: Option<&str>,
    batch_song_ids: &[String],
    fallback_song_id: &str,
) -> CommandResult<OperationRow> {
    let conn =
        state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
    let now = crate::remote::types::current_unix_time_ms();

    if let Some(op_id) = operation_id {
        let op = crate::remote::control_db::get_operation(&conn, op_id)?.ok_or_else(|| {
            CommandError::from(LibraryError::Internal(format!(
                "publish operation {op_id} was not found"
            )))
        })?;
        if op.state.is_terminal() {
            return Err(CommandError::from(LibraryError::Internal(format!(
                "refusing to reopen terminal operation {op_id} ({})",
                op.state.as_str()
            ))));
        }
        return Ok(op);
    }

    // Explicit re-publish without a mutation outbox: mint a fresh identity.
    // Terminal rows are never reused.
    let song_ids = if batch_song_ids.is_empty() {
        vec![fallback_song_id.to_owned()]
    } else {
        batch_song_ids.to_vec()
    };
    let operation_id = uuid::Uuid::new_v4().to_string();
    let repo_state = get_repository_state(&conn, remote_library_id)?;
    let expected_generation = repo_state
        .as_ref()
        .map(|r| r.committed_generation)
        .unwrap_or(0);
    let payload = OperationPayload {
        song_ids,
        percent: 0,
        detail: Some("Publishing to remote".to_owned()),
        ..Default::default()
    };
    let row = OperationRow {
        operation_id: operation_id.clone(),
        library_id: remote_library_id.to_owned(),
        operation_kind: OperationKind::Publish,
        state: OperationState::Pending,
        expected_generation: Some(expected_generation),
        target_generation: None,
        source_db_digest: None,
        candidate_db_digest: None,
        payload_json: payload.to_json()?,
        attempt_count: 0,
        next_attempt_at_ms: None,
        error_code: None,
        error_detail: None,
        created_at_ms: now,
        updated_at_ms: now,
    };
    upsert_operation(&conn, &row)?;
    Ok(row)
}

fn commit_via_executor(
    state: &AppState,
    remote_library: &RegisteredLibrary,
    remote_library_id: &str,
    operation_id: &str,
    remote_root: &LibraryRoot,
) -> CommandResult<()> {
    // Caller already holds the per-library commit lock.
    if state.remote.control_db_degraded {
        return Err(CommandError::from(LibraryError::Internal(
            "remote control database is unavailable; publication is disabled \
             until the control plane is repaired"
                .to_string(),
        )));
    }

    let provider = create_provider(&state.shell.app_data_dir, remote_library)?;

    let (repository_id, writer_id) = {
        let conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        // Refuse terminal operations here as well.
        if let Some(op) = crate::remote::control_db::get_operation(&conn, operation_id)? {
            if op.state.is_terminal() {
                return Err(CommandError::from(LibraryError::Internal(format!(
                    "refusing to execute terminal operation {operation_id}"
                ))));
            }
        }
        let repo_state = get_repository_state(&conn, remote_library_id)?;
        let repository_id = repo_state
            .as_ref()
            .and_then(|r| r.repository_id.clone())
            .unwrap_or_else(generate_repository_id);
        let writer_id = repo_state
            .as_ref()
            .and_then(|r| r.writer_id.clone())
            .unwrap_or_else(generate_writer_id);

        if repo_state
            .as_ref()
            .map(|r| r.repository_id.is_none())
            .unwrap_or(true)
        {
            let mut row = repo_state.unwrap_or(RepositoryStateRow {
                library_id: remote_library_id.to_owned(),
                committed_generation: 0,
                committed_manifest_revision: None,
                local_base_generation: 0,
                local_db_digest: None,
                local_state: LocalState::Publishing,
                active_operation_id: None,
                last_success_at_ms: None,
                last_error_code: None,
                updated_at_ms: crate::remote::types::current_unix_time_ms(),
                repository_id: Some(repository_id.clone()),
                writer_id: Some(writer_id.clone()),
            });
            row.repository_id = Some(repository_id.clone());
            row.writer_id = Some(writer_id.clone());
            row.local_state = LocalState::Publishing;
            upsert_repository_state(&conn, &row)?;
        }

        (repository_id, writer_id)
    };

    let control_db_path = crate::remote::control_db::control_db_path(&state.shell.app_data_dir);
    let exec_conn = crate::remote::control_db::open_control_db(&control_db_path).map_err(|e| {
        crate::commands::error::database_error(format!("failed to open control DB: {e:?}"))
    })?;

    let ctx = PublishContext {
        control_db: &exec_conn,
        provider: provider.as_ref(),
        working_copy_root: remote_root.root(),
        library_id: remote_library_id,
        writer_id: &writer_id,
        repository_id: &repository_id,
    };

    execute_publish(&ctx, operation_id)
}

/// Re-upload song assets for a durable operation after a crash between the
/// local mutation and the executor. Uploads are idempotent (overwrite /
/// re-put). The remote working DB is updated so asset verification and the
/// candidate freeze see the mutation's committed state.
pub(crate) fn reupload_song_assets_for_recovery(
    state: &AppState,
    remote_library: &RegisteredLibrary,
    remote_root: &LibraryRoot,
    song_id: &str,
) -> CommandResult<()> {
    let local_root = state.library_root()?;
    let same_root = local_root.root() == remote_root.root();
    let local_connection = cache::open_database(&local_root.database_path())
        .map_err(|error| database_error(error.to_string()))?;
    let mut remote_connection = cache::open_database(&remote_root.database_path())
        .map_err(|error| database_error(error.to_string()))?;
    let song = cache::get_song_by_hash(&local_connection, song_id)
        .map_err(|error| database_error(error.to_string()))?
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(format!(
                "song {song_id} was not found for publish recovery"
            )))
        })?;
    let provider = create_provider(&state.shell.app_data_dir, remote_library)?;

    if song.is_separable() {
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
            remote_root,
            &remote_connection,
            &*provider,
            song_id,
            same_root,
        )?;
        sync_song_lyrics_to_remote(&local_connection, &remote_connection, song_id)?;
    } else {
        if let Some(file_path) = song.file_path.as_deref() {
            if !same_root {
                copy_remote_song_assets(&local_root, remote_root, file_path, file_path)?;
            }
            provider.upload_file(file_path)?;
        }
        if let Some(cdg_path) = song.cdg_path.as_deref() {
            if !same_root {
                copy_remote_song_assets(&local_root, remote_root, cdg_path, cdg_path)?;
            }
            provider.upload_file(cdg_path)?;
        }
        delete_remote_stem_cache_if_present(&remote_connection, remote_root, song_id)?;
        update_remote_song(&mut remote_connection, song.clone(), "original_remote")?;
        publish_artwork_derivatives(
            &local_connection,
            &local_root,
            remote_root,
            &remote_connection,
            &*provider,
            song_id,
            same_root,
        )?;
        sync_song_lyrics_to_remote(&local_connection, &remote_connection, song_id)?;
    }
    Ok(())
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
    let song_ids = vec![song_id.clone()];
    std::thread::spawn(move || {
        let _ = publish_operation_internal(&background_state, &background_handle, None, &song_ids);
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

    // One background publication for the whole set. Without a pre-bound
    // operation_id this mints a single fresh op covering all song_ids.
    let background_state = state.clone();
    let background_handle = app_handle.clone();
    let song_ids_bg = song_ids.clone();
    std::thread::spawn(move || {
        let _ =
            publish_operation_internal(&background_state, &background_handle, None, &song_ids_bg);
    });

    Ok(snapshots)
}
