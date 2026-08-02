use crate::{
    commands::error::{database_error, CommandResult},
    library::Song,
    library_root::LibraryRoot,
    AppState,
};
use rusqlite::Connection;
use tauri::AppHandle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChangeScope {
    None,
    Songs(Vec<String>),
    WholeRepository,
}

impl ChangeScope {
    fn song_ids(&self) -> &[String] {
        match self {
            Self::Songs(song_ids) => song_ids,
            Self::None | Self::WholeRepository => &[],
        }
    }

    fn is_whole_repository(&self) -> bool {
        matches!(self, Self::WholeRepository)
    }
}

pub(crate) struct ChangeResult<T> {
    pub(crate) value: T,
    pub(crate) scope: ChangeScope,
}

pub(crate) struct Change<T, F, S> {
    prepare_scope: ChangeScope,
    mutation: F,
    scope: S,
    _value: std::marker::PhantomData<fn() -> T>,
}

impl<T, F, S> Change<T, F, S> {
    pub(crate) fn new(prepare_scope: ChangeScope, mutation: F, scope: S) -> Self {
        Self {
            prepare_scope,
            mutation,
            scope,
            _value: std::marker::PhantomData,
        }
    }
}

pub struct PreparedOperation {
    pub operation_id: String,
    pub library_id: String,
    pub expected_generation: Option<i64>,
    pub source_db_digest: Option<String>,
}

/// Deep publication interface for commands that change a Local Working Copy.
///
/// The implementation keeps pre-mutation refresh, the per-library lock,
/// atomic outbox, control projection, publication, and recovery state behind
/// this interface. Callers declare the affected scope and do not coordinate
/// those steps themselves.
pub(crate) struct PublishChanges<'a, R: tauri::Runtime> {
    state: &'a AppState,
    app_handle: &'a AppHandle<R>,
}

impl<'a, R: tauri::Runtime> PublishChanges<'a, R> {
    pub(crate) fn new(state: &'a AppState, app_handle: &'a AppHandle<R>) -> Self {
        Self { state, app_handle }
    }

    pub(crate) fn apply<T, F, S>(&self, change: Change<T, F, S>) -> CommandResult<ChangeResult<T>>
    where
        F: FnOnce(&Connection, &LibraryRoot) -> CommandResult<T>,
        S: FnOnce(&T) -> ChangeScope,
    {
        let Change {
            prepare_scope,
            mutation,
            scope,
            _value: _,
        } = change;
        let ((value, scope), _) =
            with_serialized_remote_mutation(self.state, &prepare_scope, |prepared| {
                mutate_with_atomic_outbox(self.state, prepared, mutation, scope)
            })?;
        Ok(ChangeResult { value, scope })
    }

    pub(crate) fn publish(&self, scope: &ChangeScope) -> CommandResult<()> {
        match scope {
            ChangeScope::None => Ok(()),
            ChangeScope::Songs(song_ids) => {
                if song_ids.is_empty() {
                    return Ok(());
                }
                sync_backend::publish_songs(self.state, self.app_handle, song_ids, None)
            }
            ChangeScope::WholeRepository => sync_backend::mirror(self.state, self.app_handle),
        }
    }

    pub(crate) fn recover_pending(&self) -> CommandResult<()> {
        crate::remote::recovery::retry_pending_operations(self.state)
    }
}

mod sync_backend {
    use super::super::sync;
    use super::{ChangeScope, PreparedOperation};
    use crate::commands::error::{internal_error, CommandResult};
    use crate::remote::control_db::{
        self, bind_scope_mark_pending_and_dirty_tx, get_repository_state, upsert_operation,
        OperationKind, OperationPayload, OperationRow, OperationState,
    };
    use crate::remote::sync::active_remote_library;
    use crate::AppState;
    use tauri::AppHandle;

    pub fn active_remote_library_id(state: &AppState) -> CommandResult<Option<String>> {
        Ok(
            active_remote_library(&state.shell.app_data_dir)?
                .map(|library| library.id().to_owned()),
        )
    }

    pub fn prepare_for_library(state: &AppState, library_id: &str) -> CommandResult<()> {
        let library = sync::load_registered_remote_library(&state.shell.app_data_dir, library_id)?;
        let control_db_conn = state.remote.control_db()?.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        let _ = sync::prepare_remote_database_for_mutation(
            &control_db_conn,
            &state.shell.app_data_dir,
            &library,
        )?;
        Ok(())
    }

    pub fn prepare(state: &AppState) -> CommandResult<()> {
        let Some(library_id) = active_remote_library_id(state)? else {
            return Ok(());
        };
        prepare_for_library(state, &library_id)
    }

    pub fn publish_songs<R: tauri::Runtime>(
        state: &AppState,
        app_handle: &AppHandle<R>,
        song_ids: &[String],
        operation_id: Option<&str>,
    ) -> CommandResult<()> {
        let operation_id = operation_id
            .map(str::to_owned)
            .or(find_matching_pending_operation(state, song_ids)?);
        sync::maybe_publish_songs_to_bound_remote(
            state,
            app_handle,
            song_ids,
            operation_id.as_deref(),
        )
    }

    fn find_matching_pending_operation(
        state: &AppState,
        song_ids: &[String],
    ) -> CommandResult<Option<String>> {
        if song_ids.is_empty() {
            return Ok(None);
        }
        let connection = state.remote.control_db()?.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        let mut expected = song_ids.to_vec();
        expected.sort();
        let mut candidates = control_db::list_operations(&connection)?
            .into_iter()
            .filter(|operation| {
                operation.operation_kind == OperationKind::Publish
                    && !operation.state.is_terminal()
                    && OperationPayload::from_json(&operation.payload_json)
                        .map(|payload| {
                            let mut actual = payload.song_ids;
                            actual.sort();
                            actual == expected
                        })
                        .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|operation| std::cmp::Reverse(operation.updated_at_ms));
        Ok(candidates
            .into_iter()
            .next()
            .map(|operation| operation.operation_id))
    }

    pub fn mirror<R: tauri::Runtime>(
        state: &AppState,
        app_handle: &AppHandle<R>,
    ) -> CommandResult<()> {
        sync::sync_bound_remote_for_active_local_library(state, app_handle)
    }

    // --- Durable outbox state recording ---

    pub fn record_prepared_operation_for_library(
        state: &AppState,
        library_id: &str,
        scope: &ChangeScope,
    ) -> CommandResult<Option<PreparedOperation>> {
        let library = sync::load_registered_remote_library(&state.shell.app_data_dir, library_id)?;

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

        let expected_generation = {
            let conn = state.remote.control_db()?.lock().map_err(|_| {
                crate::commands::error::state_lock_error("control DB lock was poisoned")
            })?;
            get_repository_state(&conn, library_id)?
                .map(|r| r.committed_generation)
                .unwrap_or(0)
        };

        let operation_id = uuid::Uuid::new_v4().to_string();
        let now = crate::remote::types::current_unix_time_ms();

        let payload = OperationPayload {
            song_ids: scope.song_ids().to_vec(),
            whole_repository: scope.is_whole_repository(),
            percent: 0,
            detail: None,
            ..Default::default()
        };

        let library_id = library_id.to_owned();
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
            let conn = state.remote.control_db()?.lock().map_err(|_| {
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

    pub fn record_prepared_operation(
        state: &AppState,
        scope: &ChangeScope,
    ) -> CommandResult<Option<PreparedOperation>> {
        let Some(library_id) = active_remote_library_id(state)? else {
            return Ok(None);
        };
        record_prepared_operation_for_library(state, &library_id, scope)
    }

    pub fn cancel_prepared_operation(
        state: &AppState,
        prepared: &PreparedOperation,
    ) -> CommandResult<()> {
        let now = crate::remote::types::current_unix_time_ms();
        let conn = state.remote.control_db()?.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        let mut op = control_db::get_operation(&conn, &prepared.operation_id)?
            .ok_or_else(|| internal_error("prepared operation row was not found"))?;
        op.state = OperationState::Cancelled;
        op.updated_at_ms = now;
        upsert_operation(&conn, &op)?;
        Ok(())
    }

    pub fn bind_scope_mark_pending_and_dirty(
        state: &AppState,
        prepared: &PreparedOperation,
        song_ids: &[String],
        whole_repository: bool,
    ) -> CommandResult<()> {
        let conn = state.remote.control_db()?.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        bind_scope_mark_pending_and_dirty_tx(
            &conn,
            &prepared.operation_id,
            &prepared.library_id,
            song_ids,
            whole_repository,
        )
    }
}

fn project_outbox_to_control_db(
    state: &AppState,
    prepared: &PreparedOperation,
    scope: &ChangeScope,
) -> CommandResult<()> {
    sync_backend::bind_scope_mark_pending_and_dirty(
        state,
        prepared,
        scope.song_ids(),
        scope.is_whole_repository(),
    )?;
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

fn peek_active_remote_library_id(state: &AppState) -> CommandResult<Option<String>> {
    sync_backend::active_remote_library_id(state)
}

fn with_serialized_remote_mutation<T, F>(
    state: &AppState,
    prepare_scope: &ChangeScope,
    body: F,
) -> CommandResult<(T, Option<PreparedOperation>)>
where
    F: FnOnce(Option<&PreparedOperation>) -> CommandResult<T>,
{
    let library_id = peek_active_remote_library_id(state)?;
    if let Some(library_id) = library_id {
        let commit_lock = state.remote.commit_lock(&library_id);
        let _commit_guard = commit_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Explicit library_id end-to-end — no active-library re-resolve.
        sync_backend::prepare_for_library(state, &library_id)?;
        let prepared =
            sync_backend::record_prepared_operation_for_library(state, &library_id, prepare_scope)?;
        if let Some(ref p) = prepared {
            if p.library_id != library_id {
                return Err(database_error(format!(
                    "prepared operation library {} does not match locked library {library_id}",
                    p.library_id
                )));
            }
        }
        let result = body(prepared.as_ref())?;
        Ok((result, prepared))
    } else {
        sync_backend::prepare(state)?;
        let prepared = sync_backend::record_prepared_operation(state, prepare_scope)?;
        let result = body(prepared.as_ref())?;
        Ok((result, prepared))
    }
}

/// Open the **operation's** library DB (from `prepared.library_id`), begin a
/// transaction, run `mutation`, write the publish outbox on the same
/// transaction when a remote is bound, then commit. Songs and outbox are
/// atomic.
///
/// Caller must already hold the per-library commit lock when `prepared` is
/// `Some` (see `with_serialized_remote_mutation`).
///
/// After a successful library commit, projects into control DB (fail closed)
/// and returns `(result, song_ids)`.
fn mutate_with_atomic_outbox<T, F, S>(
    state: &AppState,
    prepared: Option<&PreparedOperation>,
    mutation: F,
    scope_of: S,
) -> CommandResult<(T, ChangeScope)>
where
    F: FnOnce(&Connection, &LibraryRoot) -> CommandResult<T>,
    S: FnOnce(&T) -> ChangeScope,
{
    if prepared.is_none() {
        let library = state.library_root()?;
        let conn = crate::cache::open_database(&library.database_path())
            .map_err(|e| database_error(e.to_string()))?;
        crate::cache::apply_migrations(&conn)
            .map_err(|e| database_error(format!("library migrations failed: {e}")))?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| database_error(format!("failed to begin library transaction: {e}")))?;
        let result = mutation(&tx, &library)?;
        let scope = scope_of(&result);
        tx.commit()
            .map_err(|e| database_error(format!("failed to commit library transaction: {e}")))?;
        return Ok((result, scope));
    }

    let prepared = prepared.expect("checked is_some above");
    // Open the operation's library working copy — not whatever is currently active.
    let remote_lib = crate::remote::sync::load_registered_remote_library(
        &state.shell.app_data_dir,
        &prepared.library_id,
    )?;
    let root_path = remote_lib.working_copy_root().ok_or_else(|| {
        database_error("remote repository is missing a working copy root".to_owned())
    })?;
    let library = crate::library_root::LibraryRoot::open(&root_path)
        .map_err(|e| database_error(e.to_string()))?;
    let conn = crate::cache::open_database(&library.database_path())
        .map_err(|e| database_error(e.to_string()))?;
    crate::cache::apply_migrations(&conn)
        .map_err(|e| database_error(format!("library migrations failed: {e}")))?;

    let tx = conn
        .unchecked_transaction()
        .map_err(|e| database_error(format!("failed to begin library transaction: {e}")))?;

    let result = mutation(&tx, &library)?;
    let scope = scope_of(&result);
    let whole_repository = scope.is_whole_repository();
    let song_ids = if whole_repository {
        crate::cache::list_songs(&tx)
            .map_err(|e| {
                database_error(format!("failed to list songs for repository publish: {e}"))
            })?
            .into_iter()
            .map(|song| song.hash)
            .collect()
    } else {
        scope.song_ids().to_vec()
    };

    if !song_ids.is_empty() || whole_repository {
        // SAME transaction as the song mutation — fail closed.
        let now = crate::remote::types::current_unix_time_ms();
        let row = crate::remote::library_outbox::LibraryPublishOutboxRow {
            operation_id: prepared.operation_id.clone(),
            song_ids: song_ids.clone(),
            whole_repository,
            expected_generation: prepared.expected_generation,
            source_db_digest: prepared.source_db_digest.clone(),
            created_at_ms: now,
            projected_at_ms: None,
        };
        crate::remote::library_outbox::upsert_library_publish_outbox(&tx, &row)?;
    }

    tx.commit()
        .map_err(|e| database_error(format!("failed to commit library transaction: {e}")))?;

    if song_ids.is_empty() && !whole_repository {
        sync_backend::cancel_prepared_operation(state, prepared)?;
    } else {
        // Fail closed: projection errors leave outbox unprojected.
        project_outbox_to_control_db(state, prepared, &scope)?;
    }

    Ok((result, scope))
}

pub(crate) fn song_ids_from_songs(songs: &[Song]) -> Vec<String> {
    songs.iter().map(|song| song.hash.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache;
    use crate::commands::error::database_error;
    use crate::library::{ImportSongsResult, Song};
    use crate::library_root::LibraryRoot;
    use std::ops::Deref;
    use tempfile::TempDir;

    struct TestState {
        state: AppState,
        _directory: TempDir,
    }

    impl Deref for TestState {
        type Target = AppState;

        fn deref(&self) -> &Self::Target {
            &self.state
        }
    }

    fn test_state() -> TestState {
        let directory = tempfile::tempdir().expect("test library directory");
        let library = LibraryRoot::create(&directory.path().join("library"))
            .expect("test library should be created");
        cache::initialize_library_database(&library.database_path())
            .expect("test library database should be initialized");
        let state = AppState::test_fixture();
        *state.shell.library.lock().expect("library lock") = Some(library);
        TestState {
            state,
            _directory: directory,
        }
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
            artwork_thumb_path: None,
            imported_at: 0,
            original_ext: None,
        }
    }

    #[test]
    fn imported_songs_use_the_real_library_database() {
        let state = test_state();
        let handle = test_app_handle();

        let publication = PublishChanges::new(&state, &handle);
        let applied = publication
            .apply(Change::new(
                ChangeScope::None,
                |_: &Connection, library: &LibraryRoot| {
                    assert!(library.database_path().exists());
                    Ok(ImportSongsResult {
                        imported: vec![song("a"), song("b")],
                        failed: vec![],
                    })
                },
                |result: &ImportSongsResult| {
                    ChangeScope::Songs(song_ids_from_songs(&result.imported))
                },
            ))
            .expect("local import should succeed");
        assert_eq!(applied.value.imported.len(), 2);
        assert_eq!(
            applied.scope,
            ChangeScope::Songs(vec!["a".to_owned(), "b".to_owned()])
        );
    }

    #[test]
    fn apply_returns_business_value_and_scope() {
        let state = test_state();
        let handle = test_app_handle();

        let publication = PublishChanges::new(&state, &handle);
        let applied = publication
            .apply(Change::new(
                ChangeScope::Songs(vec!["x".to_owned(), "y".to_owned()]),
                |_: &Connection, _: &LibraryRoot| Ok(42u32),
                |_: &u32| ChangeScope::Songs(vec!["x".to_owned(), "y".to_owned()]),
            ))
            .expect("local mutation should succeed");
        assert_eq!(applied.value, 42);
        assert_eq!(
            applied.scope,
            ChangeScope::Songs(vec!["x".to_owned(), "y".to_owned()])
        );
    }

    #[test]
    fn publish_scope_is_a_noop_without_an_active_remote() {
        let state = test_state();
        let handle = test_app_handle();

        PublishChanges::new(&state, &handle)
            .publish(&ChangeScope::Songs(vec!["song-42".to_owned()]))
            .expect("publish without a remote should be skipped");
    }

    #[test]
    fn apply_uses_the_real_database() {
        let state = test_state();
        let handle = test_app_handle();

        let applied = PublishChanges::new(&state, &handle)
            .apply(Change::new(
                ChangeScope::Songs(vec!["s1".to_owned()]),
                |_: &Connection, _: &LibraryRoot| Ok("val"),
                |_: &&str| ChangeScope::Songs(vec!["s1".to_owned()]),
            ))
            .expect("song mutation should succeed");
        assert_eq!(applied.value, "val");
    }

    #[test]
    fn apply_supports_none_scope() {
        let state = test_state();
        let handle = test_app_handle();

        let applied = PublishChanges::new(&state, &handle)
            .apply(Change::new(
                ChangeScope::None,
                |_: &Connection, _: &LibraryRoot| Ok("out"),
                |_: &&str| ChangeScope::None,
            ))
            .expect("mutation should succeed");
        assert_eq!(applied.value, "out");
        assert_eq!(applied.scope, ChangeScope::None);
    }

    #[test]
    fn whole_repository_publish_is_a_noop_without_an_active_remote() {
        let state = test_state();
        let handle = test_app_handle();

        PublishChanges::new(&state, &handle)
            .publish(&ChangeScope::WholeRepository)
            .expect("whole repository publish should be skipped");
    }

    #[test]
    fn mutation_requires_a_configured_library() {
        let state = AppState::test_fixture();
        let handle = test_app_handle();

        let err = PublishChanges::new(&state, &handle)
            .apply(Change::new(
                ChangeScope::None,
                |_: &Connection, _: &LibraryRoot| Ok(()),
                |_: &()| ChangeScope::None,
            ))
            .err()
            .expect("missing library must fail");

        assert!(err.message.contains("no library configured"));
    }

    #[test]
    fn mutation_persists_changes_in_the_real_sqlite_database() {
        let state = test_state();
        let handle = test_app_handle();

        PublishChanges::new(&state, &handle)
            .apply(Change::new(
                ChangeScope::None,
                |connection: &Connection, _: &LibraryRoot| {
                    connection
                        .execute("CREATE TABLE mutation_probe (value INTEGER NOT NULL)", [])
                        .map_err(|error| database_error(error.to_string()))?;
                    connection
                        .execute("INSERT INTO mutation_probe (value) VALUES (7)", [])
                        .map_err(|error| database_error(error.to_string()))?;
                    Ok(())
                },
                |_: &()| ChangeScope::None,
            ))
            .expect("SQLite mutation should succeed");

        let library = state.library_root().expect("test library");
        let connection = cache::open_database(&library.database_path()).expect("open database");
        let value: i64 = connection
            .query_row("SELECT value FROM mutation_probe", [], |row| row.get(0))
            .expect("persisted mutation");
        assert_eq!(value, 7);
    }
}
