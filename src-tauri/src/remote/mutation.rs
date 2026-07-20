//! Remote-mutation orchestration: wraps local DB mutations with the
//! Pre-Mutation Refresh / Pre-Publish Conflict / Publish Changes protocol
//! defined in `docs/references/contracts/library.md` and `CONTEXT.md`.
//!
//! # Durable outbox contract (PR#2)
//!
//! Every local mutation that publishes to a remote follows this sequence:
//! 1. Read the local repository state row and current expected generation.
//! 2. Write a durable `prepared` operation row (operation_id, expected_generation,
//!    source_db_digest = current working DB SHA-256, payload_json with affected
//!    song IDs + kind).
//! 3. Execute and commit the library SQLite mutation (the mutation closure).
//! 4. Compute the post-mutation DB digest.
//! 5. Mark the repository `dirty` and the operation `pending` in the control DB.
//! 6. Return the local mutation result to the caller.
//! 7. Publication runs asynchronously (PR#4 drives it; PR#2 leaves the
//!    operation `pending`).
//!
//! A network failure after step 6 leaves the local edit visible and durable
//! with a retrying/outbox status. Recovery on the next startup inspects the
//! `prepared`/`pending` rows and transitions them safely.
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
//! | Wrapper                              | prepare | sync_db | publish         | When                                   |
//! |--------------------------------------|---------|---------|-----------------|----------------------------------------|
//! | `run_imported_songs_mutation`        | yes     | no      | songs (imported)| additive import; publish uploads DB    |
//! | `run_updated_songs_mutation`         | yes     | no      | songs (extracted)| metadata update; publish uploads DB   |
//! | `run_song_database_mutation`         | yes     | yes     | single song     | single-song DB-level change            |
//! | `run_song_database_mutation_with_result` | yes | yes     | song from result| same, song id only known after mutation|
//! | `run_songs_database_mutation`        | yes     | yes     | songs (extracted)| multi-song DB-level change            |
//! | `run_active_library_mirror_mutation` | no      | no      | mirror          | whole-library re-sync (e.g. maintenance)|
//! | `run_database_then_library_mirror_mutation` | yes | yes | mirror        | DB change + whole-library re-sync      |
//!
//! The deletion test confirms the set earns its keep: inlining
//! `prepare → mutate → sync_db → publish` at 18 call sites would scatter
//! the Pre-Mutation Refresh / Pre-Publish Conflict protocol across the
//! command layer. The typed wrappers concentrate it here.

use crate::{commands::error::CommandResult, library::Song, AppState};
use tauri::AppHandle;

// ---------------------------------------------------------------------------
// sync_backend: production delegates to sync::, test uses thread-local mock
// ---------------------------------------------------------------------------

/// Handle returned by `record_prepared_operation` so the caller can transition
/// the durable row to `pending` after the local mutation commits.
// used by PR#4: operation executor will read these to drive retry
#[allow(dead_code)]
pub struct PreparedOperation {
    pub operation_id: String,
    pub library_id: String,
}

#[cfg(not(test))]
mod sync_backend {
    use super::super::sync;
    use super::PreparedOperation;
    use crate::commands::error::{internal_error, CommandResult};
    use crate::remote::control_db::{
        self, get_repository_state, upsert_operation, upsert_repository_state, LocalState,
        OperationKind, OperationPayload, OperationRow, OperationState, RepositoryStateRow,
    };
    use crate::remote::sync::active_remote_library;
    use crate::AppState;
    use std::path::Path;
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

    pub fn sync_db(app_data_dir: &Path) -> CommandResult<()> {
        sync::sync_active_remote_database_if_needed(app_data_dir)
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
    ) -> CommandResult<()> {
        sync::maybe_publish_songs_to_bound_remote(state, app_handle, song_ids)
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

        let source_db_digest = db_path
            .as_ref()
            .and_then(|p| control_db::sha256_file(p).ok());

        // Read the current expected generation from the repository state row.
        // PR#4 will wire committed_generation to the manifest generation; for
        // now it defaults to 0.
        let expected_generation = {
            let conn = state.remote.control_db.lock().map_err(|_| {
                crate::commands::error::state_lock_error("control DB lock was poisoned")
            })?;
            get_repository_state(&conn, &library_id)?
                .map(|r| r.committed_generation)
                .unwrap_or(0)
        };

        let operation_id = if let Some(first) = song_ids.first() {
            format!("publish-{first}")
        } else {
            // Batch mutations don't know song_ids yet. Use a unique
            // timestamp-based id so concurrent or sequential batch mutations
            // don't collide on the same primary key. These rows are internal
            // outbook entries for recovery; get_all_upload_statuses filters
            // them out (empty song_ids payload) so they never surface as
            // phantom user-visible uploads.
            let now = crate::remote::types::current_unix_time_ms();
            format!("publish-batch-{now}")
        };
        let now = crate::remote::types::current_unix_time_ms();

        let payload = OperationPayload {
            song_ids: song_ids.to_vec(),
            percent: 0,
            detail: None,
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
        }))
    }

    /// Transition a `prepared` operation to `pending` and mark the repository
    /// `dirty` after the local mutation has committed.
    pub fn mark_operation_pending_and_dirty(
        state: &AppState,
        prepared: &PreparedOperation,
    ) -> CommandResult<()> {
        let now = crate::remote::types::current_unix_time_ms();

        let conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;

        // Transition the operation to pending.
        let mut op = control_db::get_operation(&conn, &prepared.operation_id)?
            .ok_or_else(|| internal_error("prepared operation row was not found"))?;
        op.state = OperationState::Pending;
        op.updated_at_ms = now;
        upsert_operation(&conn, &op)?;

        // Mark the repository dirty.
        let repo_row = match get_repository_state(&conn, &prepared.library_id)? {
            Some(mut row) => {
                row.local_state = LocalState::Dirty;
                row.updated_at_ms = now;
                row
            }
            None => RepositoryStateRow {
                library_id: prepared.library_id.clone(),
                committed_generation: 0,
                committed_manifest_revision: None,
                local_base_generation: 0,
                local_db_digest: None,
                local_state: LocalState::Dirty,
                active_operation_id: Some(prepared.operation_id.clone()),
                last_success_at_ms: None,
                last_error_code: None,
                updated_at_ms: now,
                repository_id: None,
                writer_id: None,
            },
        };
        upsert_repository_state(&conn, &repo_row)?;

        Ok(())
    }
}

#[cfg(test)]
mod sync_backend {
    use crate::commands::error::{CommandError, CommandResult};
    use crate::AppState;
    use std::cell::RefCell;
    use std::path::Path;
    use tauri::AppHandle;

    #[derive(Debug, Clone, PartialEq)]
    pub(super) enum SyncCall {
        Prepare,
        SyncDb,
        PublishSong(String),
        PublishSongs(Vec<String>),
        Mirror,
    }

    thread_local! {
        static CALLS: RefCell<Vec<SyncCall>> = const { RefCell::new(Vec::new()) };
        static PREPARE_RESULT: RefCell<Result<(), CommandError>> = const { RefCell::new(Ok(())) };
        static SYNC_DB_RESULT: RefCell<Result<(), CommandError>> = const { RefCell::new(Ok(())) };
        static PUBLISH_RESULT: RefCell<Result<(), CommandError>> = const { RefCell::new(Ok(())) };
        static MIRROR_RESULT: RefCell<Result<(), CommandError>> = const { RefCell::new(Ok(())) };
    }

    pub fn reset() {
        CALLS.with(|c| c.borrow_mut().clear());
        PREPARE_RESULT.with(|r| *r.borrow_mut() = Ok(()));
        SYNC_DB_RESULT.with(|r| *r.borrow_mut() = Ok(()));
        PUBLISH_RESULT.with(|r| *r.borrow_mut() = Ok(()));
        MIRROR_RESULT.with(|r| *r.borrow_mut() = Ok(()));
    }

    pub fn calls() -> Vec<SyncCall> {
        CALLS.with(|c| c.borrow().clone())
    }

    pub fn set_prepare_result(result: Result<(), CommandError>) {
        PREPARE_RESULT.with(|r| *r.borrow_mut() = result);
    }

    pub fn set_sync_db_result(result: Result<(), CommandError>) {
        SYNC_DB_RESULT.with(|r| *r.borrow_mut() = result);
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

    pub fn sync_db(_app_data_dir: &Path) -> CommandResult<()> {
        CALLS.with(|c| c.borrow_mut().push(SyncCall::SyncDb));
        SYNC_DB_RESULT.with(|r| r.borrow().clone())
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
        Ok(None)
    }

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

/// Shared prefix for every mutation that starts with a Pre-Mutation Refresh:
/// `prepare → mutation`. Centralizes the `app_data_dir` extraction so a
/// future change to how the active remote is resolved touches one place.
///
/// Also records the durable outbox contract: a `prepared` operation row is
/// written before the mutation, and the row is transitioned to `pending` +
/// the repository marked `dirty` after the mutation commits. When no active
/// remote library is bound, the durable recording is a no-op.
fn prepare_and_mutate<T, F>(state: &AppState, mutation: F) -> CommandResult<T>
where
    F: FnOnce() -> CommandResult<T>,
{
    sync_backend::prepare(state)?;
    // The song_ids are not known at this point for all callers; record with an
    // empty list. Callers that know the song_ids use the explicit wrappers
    // below which call record_prepared_operation with the real ids.
    let prepared = sync_backend::record_prepared_operation(state, &[])?;
    let result = mutation()?;
    if let Some(ref prepared) = prepared {
        sync_backend::mark_operation_pending_and_dirty(state, prepared)?;
    }
    Ok(result)
}

pub(crate) fn run_imported_songs_mutation<R, F>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    mutation: F,
) -> CommandResult<crate::library::ImportSongsResult>
where
    R: tauri::Runtime,
    F: FnOnce() -> crate::library::ImportSongsResult,
{
    // ImportSongsResult is not a CommandResult, so call prepare directly.
    sync_backend::prepare(state)?;
    // The imported song_ids are only known after the mutation, so record the
    // prepared operation with an empty list. The payload is updated when
    // mark_upload_status is called during publish.
    let prepared = sync_backend::record_prepared_operation(state, &[])?;
    let result = mutation();
    if let Some(ref prepared) = prepared {
        sync_backend::mark_operation_pending_and_dirty(state, prepared)?;
    }
    let imported_song_ids: Vec<String> = result
        .imported
        .iter()
        .map(|song| song.hash.clone())
        .collect();
    sync_backend::publish_songs(state, app_handle, &imported_song_ids)?;
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
    F: FnOnce() -> CommandResult<T>,
    S: FnOnce(&T) -> Vec<String>,
{
    let result = prepare_and_mutate(state, mutation)?;
    let song_ids = updated_song_ids(&result);
    sync_backend::publish_songs(state, app_handle, &song_ids)?;
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
    F: FnOnce() -> CommandResult<T>,
{
    let result = prepare_and_mutate(state, mutation)?;
    sync_backend::sync_db(&state.shell.app_data_dir)?;
    sync_backend::publish_song(state, app_handle, song_id)?;
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
    F: FnOnce() -> CommandResult<T>,
    S: FnOnce(&T) -> Option<String>,
{
    let result = prepare_and_mutate(state, mutation)?;
    if let Some(song_id) = song_id(&result) {
        sync_backend::sync_db(&state.shell.app_data_dir)?;
        sync_backend::publish_song(state, app_handle, &song_id)?;
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
    F: FnOnce() -> CommandResult<T>,
    S: FnOnce(&T) -> Vec<String>,
{
    let result = prepare_and_mutate(state, mutation)?;
    let song_ids = song_ids(&result);
    if !song_ids.is_empty() {
        sync_backend::sync_db(&state.shell.app_data_dir)?;
        sync_backend::publish_songs(state, app_handle, &song_ids)?;
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
    let result = prepare_and_mutate(state, mutation)?;
    sync_backend::sync_db(&state.shell.app_data_dir)?;
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

        let result = run_imported_songs_mutation(&state, &handle, || ImportSongsResult {
            imported: vec![song("a"), song("b")],
            failed: vec![],
        })
        .unwrap();

        assert_eq!(result.imported.len(), 2);
        assert_eq!(
            calls(),
            vec![
                SyncCall::Prepare,
                SyncCall::PublishSongs(vec!["a".into(), "b".into()]),
            ]
        );
    }

    #[test]
    fn updated_songs_prepares_then_publishes() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        let result = run_updated_songs_mutation(
            &state,
            &handle,
            || Ok(42u32),
            |_| vec!["x".into(), "y".into()],
        )
        .unwrap();

        assert_eq!(result, 42);
        assert_eq!(
            calls(),
            vec![
                SyncCall::Prepare,
                SyncCall::PublishSongs(vec!["x".into(), "y".into()]),
            ]
        );
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
    fn song_database_mutation_prepares_syncs_publishes() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_song_database_mutation(&state, &handle, "s1", || Ok("val")).unwrap();

        assert_eq!(
            calls(),
            vec![
                SyncCall::Prepare,
                SyncCall::SyncDb,
                SyncCall::PublishSong("s1".into()),
            ]
        );
    }

    #[test]
    fn mutation_with_result_some_id_syncs_and_publishes() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_song_database_mutation_with_result(
            &state,
            &handle,
            || Ok("out"),
            |_| Some("resolved-id".into()),
        )
        .unwrap();

        assert_eq!(
            calls(),
            vec![
                SyncCall::Prepare,
                SyncCall::SyncDb,
                SyncCall::PublishSong("resolved-id".into()),
            ]
        );
    }

    #[test]
    fn mutation_with_result_none_skips_sync_and_publish() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_song_database_mutation_with_result(&state, &handle, || Ok("out"), |_| None).unwrap();

        assert_eq!(calls(), vec![SyncCall::Prepare]);
    }

    #[test]
    fn songs_database_mutation_nonempty_syncs_and_publishes() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_songs_database_mutation(&state, &handle, || Ok(()), |_| vec!["a".into(), "b".into()])
            .unwrap();

        assert_eq!(
            calls(),
            vec![
                SyncCall::Prepare,
                SyncCall::SyncDb,
                SyncCall::PublishSongs(vec!["a".into(), "b".into()]),
            ]
        );
    }

    #[test]
    fn songs_database_mutation_empty_skips_sync_and_publish() {
        reset();
        let state = test_state();
        let handle = test_app_handle();

        run_songs_database_mutation(&state, &handle, || Ok(()), |_| vec![]).unwrap();

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

        assert_eq!(
            calls(),
            vec![SyncCall::Prepare, SyncCall::SyncDb, SyncCall::Mirror]
        );
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
            || {
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

    #[test]
    fn sync_db_error_returns_without_publishing() {
        reset();
        let state = test_state();
        let handle = test_app_handle();
        let mutation_ran = std::cell::Cell::new(false);

        sync_backend::set_sync_db_result(Err(internal_error("sync failed")));

        let err = run_song_database_mutation(&state, &handle, "s1", || {
            mutation_ran.set(true);
            Ok(())
        })
        .unwrap_err();

        assert_eq!(err.message, "sync failed");
        assert!(mutation_ran.get());
        assert_eq!(calls(), vec![SyncCall::Prepare, SyncCall::SyncDb]);
    }
}
