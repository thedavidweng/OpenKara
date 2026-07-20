use crate::{
    cache,
    commands::error::{database_error, internal_error, CommandError, CommandResult},
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
    let remote_library = {
        let control_db_conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        prepare_remote_database_for_mutation(
            &control_db_conn,
            &state.shell.app_data_dir,
            remote_library,
        )?
    };

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
    // Pre-collect artwork derivative paths (before the transaction deletes the
    // DB rows) so phase 2 can delete the on-disk derivative files. Two songs
    // can share the same cover digest, so deletion checks the DB reference
    // count — but by phase 2 the row is already gone, so we collect paths now
    // and delete unconditionally (the local library is the source of truth and
    // has already decided these songs should not exist remotely).
    let mut artwork_paths_to_delete: Vec<String> = Vec::new();
    for song in &songs_to_delete {
        if song.is_remote_stems()
            || cache::stems::get_cached_stem_entry(&remote_connection, &song.hash)
                .map_err(|error| database_error(error.to_string()))?
                .is_some()
        {
            has_stem_entry.insert(song.hash.clone());
        }
        if let Some(record) = cache::get_artwork_record(&remote_connection, &song.hash)
            .map_err(|error| database_error(error.to_string()))?
        {
            if let Some(p) = record.artwork_thumb_path {
                artwork_paths_to_delete.push(p);
            }
            if let Some(p) = record.artwork_preview_path {
                artwork_paths_to_delete.push(p);
            }
        }
    }

    if !songs_to_delete.is_empty() {
        let tx = remote_connection
            .transaction()
            .map_err(|error| database_error(error.to_string()))?;
        for song in &songs_to_delete {
            crate::library::delete_song_rows_from_database(&tx, &remote_root, &song.hash)
                .map_err(internal_error)?;
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
        let _ = crate::library::delete_song_files_from_working_copy(&remote_root, song);
        let _ = crate::library::delete_stem_files_from_working_copy(&remote_root, &song.hash);
    }

    // Best-effort artwork derivative cleanup. Derivatives are reference-
    // counted against the remote DB (two songs can share the same cover
    // digest), so the on-disk file is removed only when no remaining row
    // references it. The cloud copy is deleted under the same rule: only
    // when the local derivative was actually removed, so a shared derivative
    // belonging to a song that is staying is never deleted from the cloud.
    for path in &artwork_paths_to_delete {
        let deleted = match crate::library::artwork::delete_artwork_derivative_if_unreferenced(
            &remote_connection,
            &remote_root,
            path,
        ) {
            Ok(deleted) => deleted,
            Err(e) => {
                tracing::warn!("failed to clean up artwork derivative {path}: {e}");
                continue;
            }
        };
        if deleted {
            let _ = remote_delete_relative_path(&state.shell.app_data_dir, &remote_library, path);
        }
    }

    let desired_song_ids: Vec<String> = local_songs
        .into_iter()
        .filter_map(|song| desired_kinds.contains_key(&song.hash).then_some(song.hash))
        .collect();
    maybe_publish_songs_to_bound_remote(state, app_handle, &desired_song_ids)?;

    // Commit the remote database via the transactional manifest executor.
    // The mirror sync has already updated the remote working-copy DB with
    // the desired song set; the executor handles the candidate DB copy,
    // integrity check, manifest CAS, and verification.
    let remote_library =
        load_registered_remote_library(&state.shell.app_data_dir, remote_library.id())?;
    commit_mirror_via_executor(state, &remote_library)?;
    Ok(())
}

/// Commit the mirror sync via the transactional manifest executor.
///
/// Similar to `commit_via_executor` in publish.rs but for a mirror operation
/// (whole-library re-sync). Creates a `mirror-<library-id>-<timestamp>`
/// operation row and drives it through the executor.
fn commit_mirror_via_executor(
    state: &AppState,
    remote_library: &RegisteredLibrary,
) -> CommandResult<()> {
    use crate::remote::control_db::{
        get_operation, get_repository_state, upsert_operation, upsert_repository_state, LocalState,
        OperationKind, OperationPayload, OperationRow, OperationState, RepositoryStateRow,
    };
    use crate::remote::executor::{
        execute_publish, generate_repository_id, generate_writer_id, PublishContext,
    };

    let provider = create_provider(&state.shell.app_data_dir, remote_library)?;
    let remote_root = load_remote_root(&state.shell.app_data_dir, remote_library)?;
    let library_id = remote_library.id();
    let now = crate::remote::types::current_unix_time_ms();

    // Resolve or generate stable repository_id and writer_id.
    let (repository_id, writer_id) = {
        let conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        let repo_state = get_repository_state(&conn, library_id)?;
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
                library_id: library_id.to_owned(),
                committed_generation: 0,
                committed_manifest_revision: None,
                local_base_generation: 0,
                local_db_digest: None,
                local_state: LocalState::Publishing,
                active_operation_id: None,
                last_success_at_ms: None,
                last_error_code: None,
                updated_at_ms: now,
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

    let operation_id = format!("mirror-{library_id}-{now}");

    {
        let conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;

        // Check for an existing pending mirror operation.
        if get_operation(&conn, &operation_id)?.is_none() {
            let repo_state = get_repository_state(&conn, library_id)?;
            let expected_generation = repo_state
                .as_ref()
                .map(|r| r.committed_generation)
                .unwrap_or(0);

            let payload = OperationPayload {
                song_ids: Vec::new(),
                percent: 0,
                detail: Some("Mirror sync".to_owned()),
            };

            let row = OperationRow {
                operation_id: operation_id.clone(),
                library_id: library_id.to_owned(),
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
        }
    }

    let conn =
        state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;

    let ctx = PublishContext {
        control_db: &conn,
        provider: provider.as_ref(),
        working_copy_root: remote_root.root(),
        library_id,
        writer_id: &writer_id,
        repository_id: &repository_id,
    };

    execute_publish(&ctx, &operation_id)
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

    let mut restore_config = load_app_config(&state.shell.app_data_dir)?;
    restore_config.active_library_id = original_active_library_id;
    restore_config.pending_mirror_restore = false;
    restore_config.pending_mirror_restore_active_library_id = None;
    persist_app_config(&state.shell.app_data_dir, &restore_config)?;

    sync_result
}
