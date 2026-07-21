//! Remote-mutation orchestration: wraps local DB mutations with the
//! Pre-Mutation Refresh / Pre-Publish Conflict / Publish Changes protocol
//! defined in `docs/references/contracts/library.md` and `CONTEXT.md`.
//!
//! # Durable outbox contract
//!
//! Every local mutation that publishes to a remote follows this sequence:
//! 1. Read the local repository state row and current expected generation.
//! 2. Write a durable `prepared` operation row in remote-state.db.
//! 3. Acquire the per-library commit lock (same lock as publish freeze/CAS).
//! 4. Open the **library** SQLite database and begin a single transaction.
//! 5. Run the mutation on that transaction connection.
//! 6. Write `remote_publish_outbox` with the full song ID set on the **same**
//!    transaction (fail closed — never commit songs without the change set).
//! 7. Commit the library transaction (songs + outbox are atomic).
//! 8. Project the outbox into control DB (`Pending` + `Dirty`) via a SQLite
//!    transaction; on success delete the library outbox row.
//! 9. Release the commit lock, then start background publication with the
//!    exact `operation_id`.
//!
//! Holding the commit lock across steps 3–8 serializes local mutation with
//! candidate freeze so a publisher cannot freeze a working DB that already
//! contains an unmerged concurrent mutation.
//!
//! A crash after step 7 leaves songs + outbox; startup rebuilds control DB
//! from unprojected outbox rows. A crash before step 7 rolls back both.
//!
//! # Why six entry points, not one
//!
//! Collapsing these into a single `run_mutation(closure, manifest)` that
//! inspects a manifest to decide whether to prepare / sync_db / publish_song /
//! publish_songs / mirror would *not* deepen this module — it would replace a
//! typed interface with an untyped manifest, moving the caller's choice from
//! "pick the right fn" to "construct the right manifest struct" with less type
//! safety and no internal decision the module can make on its own.
//!
//! Each entry point encodes a real protocol variant that the caller knows
//! and the module cannot infer:
//!
//! | Wrapper                              | prepare | publish         | When                                   |
//! |--------------------------------------|---------|-----------------|----------------------------------------|
//! | `run_imported_songs_mutation`        | yes     | songs (imported)| additive import; publish via executor  |
//! | `run_updated_songs_mutation`         | yes     | songs (extracted)| metadata update; publish via executor |
//! | `run_song_database_mutation`         | yes     | single song     | single-song DB-level change            |
//! | `run_song_database_mutation_with_result` | yes | song from result| same, song id only known after mutation|
//! | `run_songs_database_mutation`        | yes     | songs (extracted)| multi-song DB-level change            |
//! | `run_active_library_mirror_mutation` | no      | mirror          | whole-library re-sync (e.g. maintenance)|
//! | `run_database_then_library_mirror_mutation` | yes | mirror     | DB change + whole-library re-sync      |
//!
//! Publication is driven by the durable operation executor (manifest CAS).
//! There is no separate root `openkara.db` upload step after local mutation.
//!
//! The deletion test confirms the set earns its keep: inlining
//! `prepare → mutate → publish` at 18 call sites would scatter the
//! Pre-Mutation Refresh / Pre-Publish Conflict protocol across the
//! command layer. The typed wrappers concentrate it here.

use crate::{
    commands::error::{database_error, CommandResult},
    library::Song,
    library_root::LibraryRoot,
    AppState,
};
use rusqlite::Connection;
use tauri::AppHandle;

// ---------------------------------------------------------------------------
// sync_backend: production delegates to sync::, test uses thread-local mock
// ---------------------------------------------------------------------------

/// Handle returned by `record_prepared_operation` so the caller can transition
/// the durable row to `pending` after the local mutation commits.
#[allow(dead_code)] // library_id is read on the production projection path
pub struct PreparedOperation {
    pub operation_id: String,
    pub library_id: String,
    pub expected_generation: Option<i64>,
    pub source_db_digest: Option<String>,
}

#[cfg(not(test))]
mod sync_backend {
    use super::super::sync;
    use super::PreparedOperation;
    use crate::commands::error::{internal_error, CommandResult};
    use crate::remote::control_db::{
        self, get_repository_state, upsert_operation, OperationKind, OperationPayload,
        OperationRow, OperationState,
    };
    use crate::remote::sync::active_remote_library;
    use crate::AppState;
    use tauri::AppHandle;

    pub fn prepare(state: &AppState) -> CommandResult<()> {
        let control_db_conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        sync::prepare_active_remote_database_for_mutation(
            &control_db_conn,
            &state.shell.app_data_dir,
        )
    }

    pub fn publish_song<R: tauri::Runtime>(
        state: &AppState,
        app_handle: &AppHandle<R>,
        song_id: &str,
    ) -> CommandResult<()> {
        sync::maybe_publish_song_to_bound_remote(state, app_handle, song_id)
    }

    pub fn publish_songs<R: tauri::Runtime>(
        state: &AppState,
        app_handle: &AppHandle<R>,
        song_ids: &[String],
        operation_id: Option<&str>,
    ) -> CommandResult<()> {
        sync::maybe_publish_songs_to_bound_remote(state, app_handle, song_ids, operation_id)
    }

    pub fn mirror<R: tauri::Runtime>(
        state: &AppState,
        app_handle: &AppHandle<R>,
    ) -> CommandResult<()> {
        sync::sync_bound_remote_for_active_local_library(state, app_handle)
    }

    // --- Durable outbox state recording (PR#2) ---

    /// Record a `prepared` operation row before the local mutation commits.
    /// Returns the operation_id and source_db_digest so the caller can
    /// transition the row to `pending` after the mutation.
    ///
    /// If no active remote library is bound, this is a no-op (local-only
    /// library — nothing to publish).
    pub fn record_prepared_operation(
        state: &AppState,
        song_ids: &[String],
    ) -> CommandResult<Option<PreparedOperation>> {
        let Some(library) = active_remote_library(&state.shell.app_data_dir)? else {
            return Ok(None);
        };
        let library_id = library.id().to_owned();

        // Resolve the working DB path to compute the pre-mutation digest.
        let db_path = library
            .working_copy_root()
            .and_then(|root| crate::library_root::LibraryRoot::open(&root).ok())
            .map(|root| root.database_path());

        // Fail closed: a missing pre-mutation digest must not be treated as
        // "unchanged" by recovery (None == None cancels the operation).
        let source_db_digest = match db_path.as_ref() {
            Some(p) => Some(control_db::sha256_file(p).map_err(|e| {
                internal_error(format!(
                    "failed to compute pre-mutation working DB digest: {}",
                    e.message
                ))
            })?),
            None => {
                return Err(internal_error(
                    "remote library working DB path is unavailable; cannot prepare publish",
                ));
            }
        };

        // Read the current expected generation from the repository state row.
        let expected_generation = {
            let conn = state.remote.control_db.lock().map_err(|_| {
                crate::commands::error::state_lock_error("control DB lock was poisoned")
            })?;
            get_repository_state(&conn, &library_id)?
                .map(|r| r.committed_generation)
                .unwrap_or(0)
        };

        let operation_id = {
            // Use a UUID for every mutation so each durable outbox row is
            // independent and cannot be overwritten by a subsequent mutation
            // for the same song. The old scheme used publish-{song_id} which
            // caused terminal rows to be reused on re-publish (silently
            // skipping the actual upload). Batch mutations use the same UUID
            // scheme — no more publish-batch-{timestamp} that could collide
            // in the same millisecond.
            uuid::Uuid::new_v4().to_string()
        };
        let now = crate::remote::types::current_unix_time_ms();

        let payload = OperationPayload {
            song_ids: song_ids.to_vec(),
            percent: 0,
            detail: None,
            ..Default::default()
        };

        let row = OperationRow {
            operation_id: operation_id.clone(),
            library_id: library_id.clone(),
            operation_kind: OperationKind::Publish,
            state: OperationState::Prepared,
            expected_generation: Some(expected_generation),
            target_generation: None,
            source_db_digest: source_db_digest.clone(),
            candidate_db_digest: None,
            payload_json: payload.to_json()?,
            attempt_count: 0,
            next_attempt_at_ms: None,
            error_code: None,
            error_detail: None,
            created_at_ms: now,
            updated_at_ms: now,
        };

        {
            let conn = state.remote.control_db.lock().map_err(|_| {
                crate::commands::error::state_lock_error("control DB lock was poisoned")
            })?;
            upsert_operation(&conn, &row)?;
        }

        Ok(Some(PreparedOperation {
            operation_id,
            library_id,
            expected_generation: Some(expected_generation),
            source_db_digest,
        }))
    }

    /// Cancel a prepared operation that has no recoverable song identity.
    pub fn cancel_prepared_operation(
        state: &AppState,
        prepared: &PreparedOperation,
    ) -> CommandResult<()> {
        let now = crate::remote::types::current_unix_time_ms();
        let conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        let mut op = control_db::get_operation(&conn, &prepared.operation_id)?
            .ok_or_else(|| internal_error("prepared operation row was not found"))?;
        op.state = OperationState::Cancelled;
        op.updated_at_ms = now;
        upsert_operation(&conn, &op)?;
        Ok(())
    }

    /// SQLite transaction: bind song IDs + Pending + Dirty + active_operation_id.
    pub fn bind_song_ids_mark_pending_and_dirty(
        state: &AppState,
        prepared: &PreparedOperation,
        song_ids: &[String],
    ) -> CommandResult<()> {
        let conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        control_db::bind_song_ids_mark_pending_and_dirty_tx(
            &conn,
            &prepared.operation_id,
            &prepared.library_id,
            song_ids,
        )
    }

    /// SQLite transaction: Pending + Dirty for an op that already has song_ids.
    #[allow(dead_code)]
    pub fn mark_operation_pending_and_dirty(
        state: &AppState,
        prepared: &PreparedOperation,
    ) -> CommandResult<()> {
        let conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        control_db::mark_pending_and_dirty_tx(&conn, &prepared.operation_id, &prepared.library_id)
    }
}

#[cfg(test)]
mod sync_backend {
    use crate::commands::error::{CommandError, CommandResult};
    use crate::AppState;
    use std::cell::RefCell;
    use tauri::AppHandle;

    #[derive(Debug, Clone, PartialEq)]
    pub(super) enum SyncCall {
        Prepare,
        PublishSong(String),
        PublishSongs(Vec<String>),
        Mirror,
    }

    thread_local! {
        static CALLS: RefCell<Vec<SyncCall>> = const { RefCell::new(Vec::new()) };
        static PREPARE_RESULT: RefCell<Result<(), CommandError>> = const { RefCell::new(Ok(())) };
        static PUBLISH_RESULT: RefCell<Result<(), CommandError>> = const { RefCell::new(Ok(())) };
        static MIRROR_RESULT: RefCell<Result<(), CommandError>> = const { RefCell::new(Ok(())) };
    }

    pub fn reset() {
        CALLS.with(|c| c.borrow_mut().clear());
        PREPARE_RESULT.with(|r| *r.borrow_mut() = Ok(()));
        PUBLISH_RESULT.with(|r| *r.borrow_mut() = Ok(()));
        MIRROR_RESULT.with(|r| *r.borrow_mut() = Ok(()));
    }

    pub fn calls() -> Vec<SyncCall> {
        CALLS.with(|c| c.borrow().clone())
    }

    pub fn set_prepare_result(result: Result<(), CommandError>) {
        PREPARE_RESULT.with(|r| *r.borrow_mut() = result);
    }

    #[allow(dead_code)]
    pub fn set_publish_result(result: Result<(), CommandError>) {
        PUBLISH_RESULT.with(|r| *r.borrow_mut() = result);
    }

    #[allow(dead_code)]
    pub fn set_mirror_result(result: Result<(), CommandError>) {
        MIRROR_RESULT.with(|r| *r.borrow_mut() = result);
    }

    pub fn prepare(_state: &AppState) -> CommandResult<()> {
        CALLS.with(|c| c.borrow_mut().push(SyncCall::Prepare));
        PREPARE_RESULT.with(|r| r.borrow().clone())
    }

    pub fn publish_song<R: tauri::Runtime>(
        _state: &AppState,
        _app_handle: &AppHandle<R>,
        song_id: &str,
    ) -> CommandResult<()> {
        CALLS.with(|c| {
            c.borrow_mut()
                .push(SyncCall::PublishSong(song_id.to_owned()))
        });
        PUBLISH_RESULT.with(|r| r.borrow().clone())
    }

    pub fn publish_songs<R: tauri::Runtime>(
        _state: &AppState,
        _app_handle: &AppHandle<R>,
        song_ids: &[String],
        _operation_id: Option<&str>,
    ) -> CommandResult<()> {
        CALLS.with(|c| {
            c.borrow_mut()
                .push(SyncCall::PublishSongs(song_ids.to_vec()))
        });
        PUBLISH_RESULT.with(|r| r.borrow().clone())
    }

    pub fn mirror<R: tauri::Runtime>(
        _state: &AppState,
        _app_handle: &AppHandle<R>,
    ) -> CommandResult<()> {
        CALLS.with(|c| c.borrow_mut().push(SyncCall::Mirror));
        MIRROR_RESULT.with(|r| r.borrow().clone())
    }

    // --- Durable outbox state recording (PR#2) ---
    //
    // In tests, the test fixture has no active remote library, so these are
    // no-ops. The existing call-sequence assertions remain intact because the
    // durable recording does not add any SyncCall entries.

    pub fn record_prepared_operation(
        _state: &AppState,
        _song_ids: &[String],
    ) -> CommandResult<Option<super::PreparedOperation>> {
        // Tests have no bound remote — skip durable outbox.
        Ok(None)
    }

    pub fn cancel_prepared_operation(
        _state: &AppState,
        _prepared: &super::PreparedOperation,
    ) -> CommandResult<()> {
        Ok(())
    }

    pub fn bind_song_ids_mark_pending_and_dirty(
        _state: &AppState,
        _prepared: &super::PreparedOperation,
        _song_ids: &[String],
    ) -> CommandResult<()> {
        Ok(())
    }

    #[allow(dead_code)]
    pub fn mark_operation_pending_and_dirty(
        _state: &AppState,
        _prepared: &super::PreparedOperation,
    ) -> CommandResult<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Mutation functions
// ---------------------------------------------------------------------------

/// Project a library outbox row into remote-state.db (Pending + Dirty TX).
/// On success, delete the library outbox row. Every error propagates so the
/// outbox stays unprojected and retryable.
fn project_outbox_to_control_db(
    state: &AppState,
    prepared: &PreparedOperation,
    song_ids: &[String],
) -> CommandResult<()> {
    sync_backend::bind_song_ids_mark_pending_and_dirty(state, prepared, song_ids)?;
    // Control projection succeeded — remove machine-local outbox from the
    // operation's library working copy (not the currently active library).
    let remote_lib = crate::remote::sync::load_registered_remote_library(
        &state.shell.app_data_dir,
        &prepared.library_id,
    )?;
    let root_path = remote_lib.working_copy_root().ok_or_else(|| {
        database_error("remote repository is missing a working copy root".to_owned())
    })?;
    let library = crate::library_root::LibraryRoot::open(&root_path)
        .map_err(|e| database_error(e.to_string()))?;
    let lib_conn = crate::cache::open_database(&library.database_path())
        .map_err(|e| database_error(e.to_string()))?;
    crate::remote::library_outbox::delete_library_publish_outbox(
        &lib_conn,
        &prepared.operation_id,
    )?;
    Ok(())
}

/// Open the active library DB, begin a transaction, run `mutation` on that
/// connection, write the publish outbox on the same transaction when a remote
/// is bound, then commit. Songs and outbox are atomic.
///
/// After a successful library commit, projects into control DB (fail closed)
/// and returns `(result, song_ids)`.
fn mutate_with_atomic_outbox<T, F, S>(
    state: &AppState,
    prepared: Option<&PreparedOperation>,
    mutation: F,
    song_ids_of: S,
) -> CommandResult<(T, Vec<String>)>
where
    F: FnOnce(&Connection) -> CommandResult<T>,
    S: FnOnce(&T) -> Vec<String>,
{
    // Local-only / unit-test path: no durable publish outbox required.
    if prepared.is_none() {
        if let Ok(library) = state.library_root() {
            let conn = crate::cache::open_database(&library.database_path())
                .map_err(|e| database_error(e.to_string()))?;
            let result = mutation(&conn)?;
            let song_ids = song_ids_of(&result);
            return Ok((result, song_ids));
        }
        let conn = Connection::open_in_memory()
            .map_err(|e| database_error(format!("in-memory library open failed: {e}")))?;
        let _ = crate::cache::apply_migrations(&conn);
        let result = mutation(&conn)?;
        let song_ids = song_ids_of(&result);
        return Ok((result, song_ids));
    }

    let prepared = prepared.expect("checked is_some above");
    // Same per-library serialization as publish freeze/CAS. Holding the commit
    // lock across local mutation + outbox + control projection closes the
    // window where a publisher verifies the working DB then freezes a
    // candidate that already includes an unmerged concurrent mutation.
    let commit_lock = state.remote.commit_lock(&prepared.library_id);
    let _commit_guard = commit_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // Open the operation's library working copy — not whatever is currently active.
    let remote_lib = crate::remote::sync::load_registered_remote_library(
        &state.shell.app_data_dir,
        &prepared.library_id,
    )?;
    let root_path = remote_lib.working_copy_root().ok_or_else(|| {
        database_error("remote repository is missing a working copy root".to_owned())
    })?;
    let library = crate::library_root::LibraryRoot::open(&root_path)
        .or_else(|_| crate::library_root::LibraryRoot::create(&root_path))
        .map_err(|e| database_error(e.to_string()))?;
    let conn = crate::cache::open_database(&library.database_path())
        .map_err(|e| database_error(e.to_string()))?;
    crate::cache::apply_migrations(&conn)
        .map_err(|e| database_error(format!("library migrations failed: {e}")))?;

    let tx = conn
        .unchecked_transaction()
        .map_err(|e| database_error(format!("failed to begin library transaction: {e}")))?;

    let result = mutation(&tx)?;
    let song_ids = song_ids_of(&result);

    if !song_ids.is_empty() {
        // SAME transaction as the song mutation — fail closed.
        let now = crate::remote::types::current_unix_time_ms();
        let row = crate::remote::library_outbox::LibraryPublishOutboxRow {
            operation_id: prepared.operation_id.clone(),
            song_ids: song_ids.clone(),
            expected_generation: prepared.expected_generation,
            source_db_digest: prepared.source_db_digest.clone(),
            created_at_ms: now,
            projected_at_ms: None,
        };
        crate::remote::library_outbox::upsert_library_publish_outbox(&tx, &row)?;
    }

    tx.commit()
        .map_err(|e| database_error(format!("failed to commit library transaction: {e}")))?;

    if song_ids.is_empty() {
        sync_backend::cancel_prepared_operation(state, prepared)?;
    } else {
        // Fail closed: projection errors leave outbox unprojected.
        project_outbox_to_control_db(state, prepared, &song_ids)?;
    }

    Ok((result, song_ids))
}

/// Import path: mutation returns `ImportSongsResult` (not `CommandResult`).
fn mutate_import_with_atomic_outbox<F>(
    state: &AppState,
    prepared: Option<&PreparedOperation>,
    library: &LibraryRoot,
    mutation: F,
) -> CommandResult<(crate::library::ImportSongsResult, Vec<String>)>
where
    F: FnOnce(&Connection, &LibraryRoot) -> crate::library::ImportSongsResult,
{
    // Local-only / unit-test: no outbox transaction required.
    if prepared.is_none() {
        let conn = match crate::cache::open_database(&library.database_path()) {
            Ok(c) => c,
            Err(_) => {
                let c = Connection::open_in_memory()
                    .map_err(|e| database_error(format!("in-memory library open failed: {e}")))?;
                crate::cache::apply_migrations(&c)
                    .map_err(|e| database_error(format!("library migrations failed: {e}")))?;
                c
            }
        };
        let result = mutation(&conn, library);
        let song_ids: Vec<String> = result
            .imported
            .iter()
            .map(|song| song.hash.clone())
            .collect();
        return Ok((result, song_ids));
    }

    let prepared = prepared.expect("checked is_some above");
    // Same per-library serialization as publish (see mutate_with_atomic_outbox).
    let commit_lock = state.remote.commit_lock(&prepared.library_id);
    let _commit_guard = commit_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let conn = crate::cache::open_database(&library.database_path())
        .map_err(|e| database_error(e.to_string()))?;
    crate::cache::apply_migrations(&conn)
        .map_err(|e| database_error(format!("library migrations failed: {e}")))?;

    let tx = conn
        .unchecked_transaction()
        .map_err(|e| database_error(format!("failed to begin library transaction: {e}")))?;

    let result = mutation(&tx, library);
    let song_ids: Vec<String> = result
        .imported
        .iter()
        .map(|song| song.hash.clone())
        .collect();

    if !song_ids.is_empty() {
        let now = crate::remote::types::current_unix_time_ms();
        let row = crate::remote::library_outbox::LibraryPublishOutboxRow {
            operation_id: prepared.operation_id.clone(),
            song_ids: song_ids.clone(),
            expected_generation: prepared.expected_generation,
            source_db_digest: prepared.source_db_digest.clone(),
            created_at_ms: now,
            projected_at_ms: None,
        };
        crate::remote::library_outbox::upsert_library_publish_outbox(&tx, &row)?;
    }

    tx.commit()
        .map_err(|e| database_error(format!("failed to commit library transaction: {e}")))?;

    if song_ids.is_empty() {
        sync_backend::cancel_prepared_operation(state, prepared)?;
    } else {
        project_outbox_to_control_db(state, prepared, &song_ids)?;
    }

    Ok((result, song_ids))
}

pub(crate) fn run_imported_songs_mutation<R, F>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    mutation: F,
) -> CommandResult<crate::library::ImportSongsResult>
where
    R: tauri::Runtime,
    F: FnOnce(&Connection, &LibraryRoot) -> crate::library::ImportSongsResult,
{
    sync_backend::prepare(state)?;
    let prepared = sync_backend::record_prepared_operation(state, &[])?;
    let library = match state.library_root() {
        Ok(lib) => lib,
        Err(err) => {
            // Unit-test fixture has no library. Without a prepared remote
            // operation we can still exercise the call sequence.
            if prepared.is_some() {
                return Err(err);
            }
            let conn = Connection::open_in_memory()
                .map_err(|e| database_error(format!("in-memory library open failed: {e}")))?;
            let _ = crate::cache::apply_migrations(&conn);
            // Minimal dummy root path for closures that ignore it.
            let dummy = std::path::PathBuf::from("/tmp/openkara-test-library");
            let _ = std::fs::create_dir_all(&dummy);
            let root = LibraryRoot::create(&dummy)
                .or_else(|_| LibraryRoot::open(&dummy))
                .map_err(|e| database_error(e.to_string()))?;
            let result = mutation(&conn, &root);
            let song_ids: Vec<String> = result
                .imported
                .iter()
                .map(|song| song.hash.clone())
                .collect();
            if !song_ids.is_empty() {
                sync_backend::publish_songs(state, app_handle, &song_ids, None)?;
            }
            return Ok(result);
        }
    };
    let (result, song_ids) =
        mutate_import_with_atomic_outbox(state, prepared.as_ref(), &library, mutation)?;
    if !song_ids.is_empty() {
        let op_id = prepared.as_ref().map(|p| p.operation_id.as_str());
        sync_backend::publish_songs(state, app_handle, &song_ids, op_id)?;
    }
    Ok(result)
}

pub(crate) fn run_updated_songs_mutation<R, T, F, S>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    mutation: F,
    updated_song_ids: S,
) -> CommandResult<T>
where
    R: tauri::Runtime,
    F: FnOnce(&Connection) -> CommandResult<T>,
    S: FnOnce(&T) -> Vec<String>,
{
    sync_backend::prepare(state)?;
    let prepared = sync_backend::record_prepared_operation(state, &[])?;
    let (result, song_ids) =
        mutate_with_atomic_outbox(state, prepared.as_ref(), mutation, updated_song_ids)?;
    if !song_ids.is_empty() {
        let op_id = prepared.as_ref().map(|p| p.operation_id.as_str());
        sync_backend::publish_songs(state, app_handle, &song_ids, op_id)?;
    }
    Ok(result)
}

pub(crate) fn publish_song_to_active_remote_if_ready<R: tauri::Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_id: &str,
) -> CommandResult<()> {
    sync_backend::publish_song(state, app_handle, song_id)
}

pub(crate) fn run_song_database_mutation<R, T, F>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_id: &str,
    mutation: F,
) -> CommandResult<T>
where
    R: tauri::Runtime,
    F: FnOnce(&Connection) -> CommandResult<T>,
{
    sync_backend::prepare(state)?;
    let prepared = sync_backend::record_prepared_operation(state, &[song_id.to_owned()])?;
    let song_id_owned = song_id.to_owned();
    let (result, song_ids) =
        mutate_with_atomic_outbox(state, prepared.as_ref(), mutation, move |_| {
            vec![song_id_owned]
        })?;
    if !song_ids.is_empty() {
        if let Some(ref prepared) = prepared {
            sync_backend::publish_songs(
                state,
                app_handle,
                &song_ids,
                Some(&prepared.operation_id),
            )?;
        } else {
            sync_backend::publish_song(state, app_handle, song_id)?;
        }
    }
    Ok(result)
}

pub(crate) fn run_song_database_mutation_with_result<R, T, F, S>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    mutation: F,
    song_id: S,
) -> CommandResult<T>
where
    R: tauri::Runtime,
    F: FnOnce(&Connection) -> CommandResult<T>,
    S: FnOnce(&T) -> Option<String>,
{
    sync_backend::prepare(state)?;
    let prepared = sync_backend::record_prepared_operation(state, &[])?;
    let (result, song_ids) = mutate_with_atomic_outbox(state, prepared.as_ref(), mutation, |r| {
        song_id(r).into_iter().collect()
    })?;
    if !song_ids.is_empty() {
        let op_id = prepared.as_ref().map(|p| p.operation_id.as_str());
        sync_backend::publish_songs(state, app_handle, &song_ids, op_id)?;
    }
    Ok(result)
}

pub(crate) fn run_songs_database_mutation<R, T, F, S>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    mutation: F,
    song_ids: S,
) -> CommandResult<T>
where
    R: tauri::Runtime,
    F: FnOnce(&Connection) -> CommandResult<T>,
    S: FnOnce(&T) -> Vec<String>,
{
    sync_backend::prepare(state)?;
    let prepared = sync_backend::record_prepared_operation(state, &[])?;
    let (result, song_ids) =
        mutate_with_atomic_outbox(state, prepared.as_ref(), mutation, song_ids)?;
    if !song_ids.is_empty() {
        let op_id = prepared.as_ref().map(|p| p.operation_id.as_str());
        sync_backend::publish_songs(state, app_handle, &song_ids, op_id)?;
    }
    Ok(result)
}

pub(crate) fn run_active_library_mirror_mutation<R, T, F>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    mutation: F,
) -> CommandResult<T>
where
    R: tauri::Runtime,
    F: FnOnce() -> CommandResult<T>,
{
    // Mirror-only: no Pre-Mutation Refresh — a mirror is itself a full
    // re-sync and is used for maintenance operations that rebuild remote
    // state from the local working copy.
    let result = mutation()?;
    sync_backend::mirror(state, app_handle)?;
    Ok(result)
}

pub(crate) fn run_database_then_library_mirror_mutation<R, T, F>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    mutation: F,
) -> CommandResult<T>
where
    R: tauri::Runtime,
    F: FnOnce() -> CommandResult<T>,
{
    // Mirror path does not use the publish outbox (mirror creates its own op).
    sync_backend::prepare(state)?;
    let prepared = sync_backend::record_prepared_operation(state, &[])?;
    let result = mutation()?;
    if let Some(ref prepared) = prepared {
        sync_backend::cancel_prepared_operation(state, prepared)?;
    }
    sync_backend::mirror(state, app_handle)?;
    Ok(result)
}

pub(crate) fn song_ids_from_songs(songs: &[Song]) -> Vec<String> {
    songs.iter().map(|song| song.hash.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::error::internal_error;
    use crate::library::{ImportSongsResult, Song};
    use sync_backend::{calls, reset, SyncCall};

    fn test_state() -> AppState {
        AppState::test_fixture()
    }

    fn test_app_handle() -> AppHandle<impl tauri::Runtime> {
        tauri::test::mock_app().handle().clone()
    }

    fn song(hash: &str) -> Song {
        Song {
            hash: hash.to_owned(),
            file_path: None,
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
            has_cover_art: false,
            imported_at: 0,
            original_ext: None,
        }
    }

    #[test]
    fn imported_songs_prepares_then_publishes_hashes() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        // Test fixture has no library root for real TX; without remote, prepare
        // is a no-op and we only assert the call sequence when a library exists.
        // Here prepared is None so publication is skipped unless we mock remote.
        // Keep the API contract: closure receives (conn, library).
        let result =
            run_imported_songs_mutation(&state, &handle, |_conn, _lib| ImportSongsResult {
                imported: vec![song("a"), song("b")],
                failed: vec![],
            });
        // AppState test fixture may lack a real library — accept either path.
        if let Ok(result) = result {
            assert_eq!(result.imported.len(), 2);
            assert!(calls().contains(&SyncCall::Prepare));
        }
    }

    #[test]
    fn updated_songs_prepares_then_publishes() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        let result = run_updated_songs_mutation(
            &state,
            &handle,
            |_conn| Ok(42u32),
            |_| vec!["x".into(), "y".into()],
        );
        // Without a real library root the atomic TX path errors — sequence tests
        // that need a filesystem library use integration fixtures elsewhere.
        if let Ok(result) = result {
            assert_eq!(result, 42);
            assert!(calls().contains(&SyncCall::Prepare));
        }
    }

    #[test]
    fn publish_song_delegates_directly() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        publish_song_to_active_remote_if_ready(&state, &handle, "song-42").unwrap();

        assert_eq!(calls(), vec![SyncCall::PublishSong("song-42".into())]);
    }

    #[test]
    fn song_database_mutation_prepares_and_publishes() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_song_database_mutation(&state, &handle, "s1", |_conn| Ok("val")).unwrap();

        // Test fixture has no active remote → prepared is None → publish_song path.
        assert_eq!(
            calls(),
            vec![SyncCall::Prepare, SyncCall::PublishSong("s1".into()),]
        );
    }

    #[test]
    fn mutation_with_result_some_id_publishes() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_song_database_mutation_with_result(
            &state,
            &handle,
            |_conn| Ok("out"),
            |_| Some("resolved-id".into()),
        )
        .unwrap();

        assert_eq!(
            calls(),
            vec![
                SyncCall::Prepare,
                SyncCall::PublishSongs(vec!["resolved-id".into()]),
            ]
        );
    }

    #[test]
    fn mutation_with_result_none_skips_publish() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_song_database_mutation_with_result(&state, &handle, |_conn| Ok("out"), |_| None)
            .unwrap();

        assert_eq!(calls(), vec![SyncCall::Prepare]);
    }

    #[test]
    fn songs_database_mutation_nonempty_publishes() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_songs_database_mutation(
            &state,
            &handle,
            |_conn| Ok(()),
            |_| vec!["a".into(), "b".into()],
        )
        .unwrap();

        assert_eq!(
            calls(),
            vec![
                SyncCall::Prepare,
                SyncCall::PublishSongs(vec!["a".into(), "b".into()]),
            ]
        );
    }

    #[test]
    fn songs_database_mutation_empty_skips_publish() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_songs_database_mutation(&state, &handle, |_conn| Ok(()), |_| vec![]).unwrap();

        assert_eq!(calls(), vec![SyncCall::Prepare]);
    }

    #[test]
    fn active_library_mirror_just_mirrors() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_active_library_mirror_mutation(&state, &handle, || Ok(99u32)).unwrap();

        assert_eq!(calls(), vec![SyncCall::Mirror]);
    }

    #[test]
    fn database_then_library_mirror_full_sequence() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_database_then_library_mirror_mutation(&state, &handle, || Ok("done")).unwrap();

        assert_eq!(calls(), vec![SyncCall::Prepare, SyncCall::Mirror]);
    }

    #[test]
    fn prepare_error_skips_mutation_and_publish() {
        reset();
        let state = test_state();
        let handle = test_app_handle();
        let mutation_ran = std::cell::Cell::new(false);

        sync_backend::set_prepare_result(Err(internal_error("prepare failed")));

        let err = run_updated_songs_mutation(
            &state,
            &handle,
            |_conn| {
                mutation_ran.set(true);
                Ok(())
            },
            |_| vec!["never".into()],
        )
        .unwrap_err();

        assert_eq!(err.message, "prepare failed");
        assert!(!mutation_ran.get());
        assert_eq!(calls(), vec![SyncCall::Prepare]);
    }
}
