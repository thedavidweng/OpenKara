use crate::{
    cache,
    commands::error::{database_error, CommandError, CommandResult},
    config::RegisteredLibrary,
    library::{artwork, error::LibraryError, Song},
    library_root::LibraryRoot,
    remote::control_db::{
        get_repository_state, list_operations_for_library, upsert_operation,
        upsert_repository_state, LocalState, OperationKind, OperationPayload, OperationRow,
        OperationState, RepositoryStateRow,
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
use super::revision::{
    load_registered_remote_library, prepare_remote_database_for_mutation, resolve_active_remote,
};
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
/// durable identity is used and **`op.library_id` is the target** — never the
/// currently active library. When `None`, a fresh operation is created against
/// the active remote (explicit re-publish).
pub(crate) fn maybe_publish_songs_to_bound_remote<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_ids: &[String],
    operation_id: Option<&str>,
) -> CommandResult<()> {
    if song_ids.is_empty() {
        return Ok(());
    }

    // Resolve target library: prefer the durable operation's library_id so a
    // post-mutation library switch cannot redirect the publish.
    let target_library_id = if let Some(op_id) = operation_id {
        let conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        let op = crate::remote::control_db::get_operation(&conn, op_id)?.ok_or_else(|| {
            CommandError::from(LibraryError::Internal(format!(
                "publish operation {op_id} was not found"
            )))
        })?;
        if op.state.is_terminal() {
            return Ok(());
        }
        op.library_id
    } else {
        let config = load_app_config(&state.shell.app_data_dir)?;
        let Some(active) = resolve_active_remote(&config) else {
            return Ok(());
        };
        active.id().to_owned()
    };

    let remote_library =
        load_registered_remote_library(&state.shell.app_data_dir, &target_library_id)?;
    let local_root = load_remote_root(&state.shell.app_data_dir, &remote_library)?;
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
    if state.remote.control_db_degraded {
        return Err(CommandError::from(LibraryError::Internal(
            "remote control database is unavailable; publication is disabled \
             until the control plane is repaired"
                .to_string(),
        )));
    }

    // Resolve the durable operation FIRST so the target library is taken from
    // op.library_id — not the currently active library (user may have switched).
    let (resolved_op, remote_library) = {
        let conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
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
            let library =
                load_registered_remote_library(&state.shell.app_data_dir, &op.library_id)?;
            (op, library)
        } else {
            // Explicit re-publish: fall back to active remote.
            drop(conn);
            let config = load_app_config(&state.shell.app_data_dir)?;
            let library = resolve_active_remote(&config).ok_or_else(|| {
                CommandError::from(LibraryError::Internal(
                    "no bound remote repository is available for publishing".to_string(),
                ))
            })?;
            let op = resolve_or_create_publish_operation(
                state,
                library.id(),
                None,
                batch_song_ids,
                song_id,
            )?;
            (op, library)
        }
    };
    let remote_library_id = remote_library.id().to_owned();
    if resolved_op.library_id != remote_library_id {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "operation {} library_id {} does not match target library {}",
            resolved_op.operation_id, resolved_op.library_id, remote_library_id
        ))));
    }

    // Per-library commit lock serializes the full publish (assets + freeze + CAS)
    // and coalesces concurrent pending ops under one generation advance.
    let commit_lock = state.remote.commit_lock(&remote_library_id);
    let _commit_guard = commit_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // If this operation was merged/cancelled by a concurrent publisher, stop.
    {
        let conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        if let Some(op) =
            crate::remote::control_db::get_operation(&conn, &resolved_op.operation_id)?
        {
            if op.state.is_terminal() {
                return Err(CommandError::from(LibraryError::Internal(format!(
                    "operation {} already terminal ({}); likely merged into another publish",
                    op.operation_id,
                    op.state.as_str()
                ))));
            }
        }
    }

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

    // Working-copy root comes from the operation's library, never active.
    let remote_root = load_remote_root(&state.shell.app_data_dir, &remote_library)?;
    let local_root = remote_root.clone();
    let same_root = true; // remote library working copy is the source of truth for publish

    let local_connection = cache::open_database(&local_root.database_path())
        .map_err(|error| database_error(error.to_string()))?;
    let mut remote_connection = cache::open_database(&remote_root.database_path())
        .map_err(|error| database_error(error.to_string()))?;

    // Under the commit lock: atomically merge co-pending publish ops for this
    // library into one durable identity, re-bind expected_generation to the
    // current committed generation, and union song_ids. Prepared ops are never
    // merged (they have not finished local mutation + outbox projection).
    let (operation_id, song_ids_to_publish) = merge_pending_ops_for_publish(
        state,
        &remote_library_id,
        &resolved_op.operation_id,
        batch_song_ids,
        song_id,
        Some(remote_root.root()),
    )?;
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

    // Upload every song in the (merged) durable payload under the commit lock,
    // then freeze/CAS once.
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
/// Only `Pending` / `RetryWait` may be coalesced. `Prepared` is excluded
/// because its local library transaction and outbox projection may not have
/// finished — canceling it would leave "local mutation committed + operation
/// cancelled". In-flight executor states are owned by a lock holder.
fn is_mergeable_publish_state(state: OperationState) -> bool {
    matches!(state, OperationState::Pending | OperationState::RetryWait)
}

/// Under the per-library commit lock: atomically merge every
/// `Pending`/`RetryWait` Publish op for this library into `primary_op_id`.
///
/// All of the following happen inside **one** control-DB SQLite transaction:
/// - read mergeable operations and union song_ids
/// - update primary payload (and invalidate stale candidate identity when the
///   change set grows)
/// - rebind `expected_generation` to the live committed generation
/// - cancel secondary operations
/// - rebind repository `active_operation_id`
/// - clear transfer/upload session rows for invalidated candidates
///
/// Returns `(survivor_op_id, merged_song_ids)`.
pub(crate) fn merge_pending_ops_for_publish(
    state: &AppState,
    library_id: &str,
    primary_op_id: &str,
    batch_song_ids: &[String],
    fallback_song_id: &str,
    working_copy_root: Option<&std::path::Path>,
) -> CommandResult<(String, Vec<String>)> {
    let conn =
        state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
    let now = crate::remote::types::current_unix_time_ms();
    let mut candidate_paths_to_delete: Vec<String> = Vec::new();

    let song_ids = {
        let tx = conn.unchecked_transaction().map_err(|e| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to begin coalesce transaction: {e}"
            )))
        })?;

        let mut primary = crate::remote::control_db::get_operation(&tx, primary_op_id)?
            .ok_or_else(|| {
                CommandError::from(LibraryError::Internal(format!(
                    "publish operation {primary_op_id} was not found"
                )))
            })?;
        if primary.library_id != library_id {
            return Err(CommandError::from(LibraryError::Internal(format!(
                "operation {primary_op_id} library_id mismatch"
            ))));
        }
        if primary.state.is_terminal() {
            return Err(CommandError::from(LibraryError::Internal(format!(
                "operation {primary_op_id} is terminal"
            ))));
        }

        let mut song_ids: Vec<String> = OperationPayload::from_json(&primary.payload_json)
            .map(|p| p.song_ids)
            .unwrap_or_default();
        if song_ids.is_empty() {
            if batch_song_ids.is_empty() {
                if !fallback_song_id.is_empty() {
                    song_ids.push(fallback_song_id.to_owned());
                }
            } else {
                song_ids.extend(batch_song_ids.iter().cloned());
            }
        }
        let original_song_ids = song_ids.clone();

        let mut merged_secondary = false;
        let all_ops = list_operations_for_library(&tx, library_id)?;
        for mut other in all_ops {
            if other.operation_id == primary_op_id {
                continue;
            }
            if other.operation_kind != OperationKind::Publish {
                continue;
            }
            // Never coalesce Prepared (or in-flight executor states).
            if !is_mergeable_publish_state(other.state) {
                continue;
            }
            if let Ok(payload) = OperationPayload::from_json(&other.payload_json) {
                for sid in payload.song_ids {
                    if !song_ids.iter().any(|s| s == &sid) {
                        song_ids.push(sid);
                    }
                }
                if let Some(rel) = payload.candidate_relative_path {
                    candidate_paths_to_delete.push(rel);
                }
            }
            other.state = OperationState::Cancelled;
            other.error_code = Some("merged".to_owned());
            other.error_detail = Some(format!(
                "merged into concurrent publish operation {primary_op_id}"
            ));
            other.updated_at_ms = now;
            upsert_operation(&tx, &other)?;
            // Secondary transfer sessions are abandoned with the cancelled op.
            let _ = crate::remote::control_db::delete_transfer_parts(&tx, &other.operation_id);
            merged_secondary = true;
        }

        let committed = get_repository_state(&tx, library_id)?
            .map(|r| r.committed_generation)
            .unwrap_or(0);

        let mut payload = OperationPayload::from_json(&primary.payload_json).unwrap_or_default();
        let mut sorted_original = original_song_ids;
        let mut sorted_merged = song_ids.clone();
        sorted_original.sort();
        sorted_merged.sort();
        let song_set_changed = sorted_original != sorted_merged;
        // Any new song/change set invalidates a previously frozen candidate —
        // otherwise a RetryWait op would reuse an A-only candidate after
        // merging B into the payload.
        let invalidate_candidate = merged_secondary || song_set_changed;
        if invalidate_candidate {
            if let Some(rel) = payload.candidate_relative_path.take() {
                candidate_paths_to_delete.push(rel);
            }
            payload.candidate_size = None;
            payload.candidate_sha256 = None;
            payload.protocol_step = None;
            primary.candidate_db_digest = None;
            let _ = crate::remote::control_db::delete_transfer_parts(&tx, primary_op_id);
        }

        payload.song_ids = song_ids.clone();
        if payload.detail.is_none() {
            payload.detail = Some("Publishing to remote".to_owned());
        }
        primary.payload_json = payload.to_json()?;
        primary.expected_generation = Some(committed);
        primary.state = OperationState::Pending;
        primary.next_attempt_at_ms = None;
        primary.updated_at_ms = now;
        upsert_operation(&tx, &primary)?;

        // Rebind active_operation_id so status/UI track the survivor.
        if let Some(mut repo) = get_repository_state(&tx, library_id)? {
            repo.active_operation_id = Some(primary_op_id.to_owned());
            if repo.local_state == LocalState::Clean {
                repo.local_state = LocalState::Dirty;
            }
            repo.updated_at_ms = now;
            upsert_repository_state(&tx, &repo)?;
        }

        tx.commit().map_err(|e| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to commit coalesce transaction: {e}"
            )))
        })?;
        song_ids
    };

    // Best-effort: remove abandoned candidate files outside the control TX.
    // Missing files are fine; the identity was already cleared durably.
    if let Some(root) = working_copy_root {
        for rel in candidate_paths_to_delete {
            let path = root.join(rel);
            let _ = std::fs::remove_file(path);
        }
    }

    Ok((primary_op_id.to_owned(), song_ids))
}

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
        if op.library_id != remote_library_id {
            return Err(CommandError::from(LibraryError::Internal(format!(
                "operation {op_id} library_id {} != {remote_library_id}",
                op.library_id
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
/// re-put). Assets are always read from the **operation's library working
/// copy** (`remote_root`), never from the currently active library — the user
/// may have switched libraries since the operation was created.
pub(crate) fn reupload_song_assets_for_recovery(
    state: &AppState,
    remote_library: &RegisteredLibrary,
    remote_root: &LibraryRoot,
    song_id: &str,
) -> CommandResult<()> {
    // Source of truth is the operation library working copy.
    let same_root = true;
    let local_connection = cache::open_database(&remote_root.database_path())
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
        upsert_stem_entry(&remote_connection, &stem_entry)?;
        update_remote_song(&mut remote_connection, song.clone(), "stems_remote")?;
        provider.upload_directory(&format!("stems/{song_id}"))?;
        publish_artwork_derivatives(
            &local_connection,
            remote_root,
            remote_root,
            &remote_connection,
            &*provider,
            song_id,
            same_root,
        )?;
        sync_song_lyrics_to_remote(&local_connection, &remote_connection, song_id)?;
    } else {
        if let Some(file_path) = song.file_path.as_deref() {
            provider.upload_file(file_path)?;
        }
        if let Some(cdg_path) = song.cdg_path.as_deref() {
            provider.upload_file(cdg_path)?;
        }
        delete_remote_stem_cache_if_present(&remote_connection, remote_root, song_id)?;
        update_remote_song(&mut remote_connection, song.clone(), "original_remote")?;
        publish_artwork_derivatives(
            &local_connection,
            remote_root,
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

#[cfg(test)]
mod merge_tests {
    use super::merge_pending_ops_for_publish;
    use crate::remote::control_db::{
        get_operation, open_control_db, upsert_operation, upsert_repository_state, LocalState,
        OperationKind, OperationPayload, OperationRow, OperationState, RepositoryStateRow,
    };
    use crate::AppState;
    use std::sync::{Arc, Mutex};

    fn op(id: &str, library_id: &str, songs: &[&str], state: OperationState) -> OperationRow {
        let payload = OperationPayload {
            song_ids: songs.iter().map(|s| (*s).to_owned()).collect(),
            percent: 0,
            detail: None,
            ..Default::default()
        };
        OperationRow {
            operation_id: id.to_owned(),
            library_id: library_id.to_owned(),
            operation_kind: OperationKind::Publish,
            state,
            expected_generation: Some(10),
            target_generation: None,
            source_db_digest: None,
            candidate_db_digest: None,
            payload_json: payload.to_json().unwrap(),
            attempt_count: 0,
            next_attempt_at_ms: None,
            error_code: None,
            error_detail: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }

    fn op_with_candidate(
        id: &str,
        library_id: &str,
        songs: &[&str],
        state: OperationState,
        candidate_rel: &str,
        candidate_sha: &str,
    ) -> OperationRow {
        let mut row = op(id, library_id, songs, state);
        let mut payload = OperationPayload::from_json(&row.payload_json).unwrap();
        payload.candidate_relative_path = Some(candidate_rel.to_owned());
        payload.candidate_sha256 = Some(candidate_sha.to_owned());
        payload.candidate_size = Some(42);
        payload.protocol_step = Some("candidate_ready".to_owned());
        row.payload_json = payload.to_json().unwrap();
        row.candidate_db_digest = Some(candidate_sha.to_owned());
        row
    }

    fn seed_repo(conn: &rusqlite::Connection) {
        upsert_repository_state(
            conn,
            &RepositoryStateRow {
                library_id: "lib-1".to_owned(),
                committed_generation: 10,
                committed_manifest_revision: None,
                local_base_generation: 0,
                local_db_digest: None,
                local_state: LocalState::Dirty,
                active_operation_id: None,
                last_success_at_ms: None,
                last_error_code: None,
                updated_at_ms: 1,
                repository_id: None,
                writer_id: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn merge_pending_ops_unions_song_ids_and_cancels_others() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remote-state.db");
        let conn = open_control_db(&path).unwrap();
        seed_repo(&conn);
        upsert_operation(
            &conn,
            &op("op-a", "lib-1", &["song-a"], OperationState::Pending),
        )
        .unwrap();
        upsert_operation(
            &conn,
            &op("op-b", "lib-1", &["song-b"], OperationState::Pending),
        )
        .unwrap();

        let mut state = AppState::test_fixture();
        state.remote.control_db = Arc::new(Mutex::new(conn));

        let (survivor, songs) =
            merge_pending_ops_for_publish(&state, "lib-1", "op-a", &[], "song-a", None).unwrap();
        assert_eq!(survivor, "op-a");
        assert!(songs.contains(&"song-a".to_owned()));
        assert!(songs.contains(&"song-b".to_owned()));

        let conn = state.remote.control_db.lock().unwrap();
        let a = get_operation(&conn, "op-a").unwrap().unwrap();
        assert_eq!(a.expected_generation, Some(10));
        let repo = crate::remote::control_db::get_repository_state(&conn, "lib-1")
            .unwrap()
            .unwrap();
        assert_eq!(repo.active_operation_id.as_deref(), Some("op-a"));
        let b = get_operation(&conn, "op-b").unwrap().unwrap();
        assert_eq!(b.state, OperationState::Cancelled);
        assert_eq!(b.error_code.as_deref(), Some("merged"));
    }

    #[test]
    fn merge_does_not_cancel_prepared_operations() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remote-state.db");
        let conn = open_control_db(&path).unwrap();
        seed_repo(&conn);
        upsert_operation(
            &conn,
            &op("op-a", "lib-1", &["song-a"], OperationState::Pending),
        )
        .unwrap();
        // Prepared: local mutation not yet committed — must survive merge.
        upsert_operation(
            &conn,
            &op("op-prepared", "lib-1", &[], OperationState::Prepared),
        )
        .unwrap();

        let mut state = AppState::test_fixture();
        state.remote.control_db = Arc::new(Mutex::new(conn));

        let (survivor, songs) =
            merge_pending_ops_for_publish(&state, "lib-1", "op-a", &[], "song-a", None).unwrap();
        assert_eq!(survivor, "op-a");
        assert_eq!(songs, vec!["song-a".to_owned()]);

        let conn = state.remote.control_db.lock().unwrap();
        let prepared = get_operation(&conn, "op-prepared").unwrap().unwrap();
        assert_eq!(prepared.state, OperationState::Prepared);
        assert!(prepared.error_code.is_none());
    }

    #[test]
    fn merge_invalidates_stale_candidate_when_song_set_grows() {
        let dir = tempfile::TempDir::new().unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir_all(work.join(".openkara/candidates")).unwrap();
        let candidate_rel = ".openkara/candidates/op-a.sqlite";
        let candidate_path = work.join(candidate_rel);
        std::fs::write(&candidate_path, b"stale-candidate").unwrap();

        let path = dir.path().join("remote-state.db");
        let conn = open_control_db(&path).unwrap();
        seed_repo(&conn);
        upsert_operation(
            &conn,
            &op_with_candidate(
                "op-a",
                "lib-1",
                &["song-a"],
                OperationState::RetryWait,
                candidate_rel,
                "deadbeef",
            ),
        )
        .unwrap();
        upsert_operation(
            &conn,
            &op("op-b", "lib-1", &["song-b"], OperationState::Pending),
        )
        .unwrap();

        let mut state = AppState::test_fixture();
        state.remote.control_db = Arc::new(Mutex::new(conn));

        let (_survivor, songs) =
            merge_pending_ops_for_publish(&state, "lib-1", "op-a", &[], "song-a", Some(&work))
                .unwrap();
        assert!(songs.contains(&"song-b".to_owned()));

        let conn = state.remote.control_db.lock().unwrap();
        let a = get_operation(&conn, "op-a").unwrap().unwrap();
        let payload = OperationPayload::from_json(&a.payload_json).unwrap();
        assert!(payload.candidate_relative_path.is_none());
        assert!(payload.candidate_sha256.is_none());
        assert!(payload.candidate_size.is_none());
        assert!(payload.protocol_step.is_none());
        assert!(a.candidate_db_digest.is_none());
        drop(conn);

        assert!(
            !candidate_path.exists(),
            "stale candidate file must be removed after merge"
        );
    }

    #[test]
    fn merge_retry_wait_secondary_into_pending_primary() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("remote-state.db");
        let conn = open_control_db(&path).unwrap();
        seed_repo(&conn);
        upsert_operation(
            &conn,
            &op("op-a", "lib-1", &["song-a"], OperationState::Pending),
        )
        .unwrap();
        upsert_operation(
            &conn,
            &op("op-b", "lib-1", &["song-b"], OperationState::RetryWait),
        )
        .unwrap();

        let mut state = AppState::test_fixture();
        state.remote.control_db = Arc::new(Mutex::new(conn));

        let (_, songs) =
            merge_pending_ops_for_publish(&state, "lib-1", "op-a", &[], "song-a", None).unwrap();
        assert!(songs.contains(&"song-a".to_owned()));
        assert!(songs.contains(&"song-b".to_owned()));

        let conn = state.remote.control_db.lock().unwrap();
        assert_eq!(
            get_operation(&conn, "op-b").unwrap().unwrap().state,
            OperationState::Cancelled
        );
    }
}
