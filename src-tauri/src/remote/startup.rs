use crate::config::RegisteredLibrary;
use crate::library_root::LibraryRoot;
use crate::remote::control_db::{
    self, bind_scope_mark_pending_and_dirty_tx, get_operation, upsert_operation, OperationKind,
    OperationPayload, OperationRow, OperationState,
};
use crate::remote::library_outbox::{
    delete_library_publish_outbox, list_unprojected_library_outbox, LibraryPublishOutboxRow,
};
use crate::remote::recovery::{
    recover_stale_part_files, run_recovery, Clock, DigestResolver, FileDigestResolver,
    RecoveryReport,
};
use crate::state::RemoteState;
use crate::AppState;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

const DURABLE_OPERATION_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// What one startup pass decided, for inspection by tests and callers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupReport {
    /// Library outbox rows that became, or rebound, a control-plane operation.
    pub projected_operations: Vec<String>,
    /// Residual outbox rows dropped because a terminal operation covers them.
    pub dropped_residual_outboxes: Vec<String>,
    /// Residual outbox rows kept because no terminal operation covers them.
    pub retained_residual_outboxes: Vec<String>,
    /// `None` when the control database was unreachable or the pass failed.
    pub recovery: Option<RecoveryReport>,
    pub removed_part_files: Vec<PathBuf>,
}

/// Prepare the Remote Repository control plane for a new session and start the
/// durable operation executor.
///
/// Call once from app bootstrap, after `RemoteState` exists and before any
/// caller may reach the control plane. Errors are logged rather than returned:
/// startup recovery must never abort app start. The executor polls in the
/// background until the managed [`DurableOperationExecutor`] is told to stop;
/// the run loop's exit handler does that via
/// [`shutdown_durable_operation_executor`].
pub(crate) fn prepare_control_plane<R: Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
) -> StartupReport {
    let app_data_dir = &state.shell.app_data_dir;
    let libraries = registered_libraries(app_data_dir);
    let digest_resolver = working_copy_digest_resolver(libraries.clone());
    let clock: Clock = Box::new(crate::remote::types::current_unix_time_ms);

    let report = recover_control_plane(
        &state.remote,
        app_data_dir,
        &libraries,
        &digest_resolver,
        &clock,
    );

    let executor = spawn_durable_operation_executor(state.clone(), app_handle.clone());
    app_handle.manage(executor);

    report
}

/// Recover the Remote Repository control plane with its seams injected.
///
/// `libraries` is the registered-library snapshot the pass operates over,
/// `digest_resolver` answers what a Local Working Copy hashes to right now, and
/// `clock` stamps every row the pass writes.
///
/// The step order is part of the contract. Library outboxes project into the
/// control database first, so recovery judges each interrupted Publish Changes
/// intent with its song identity restored instead of cancelling it as
/// unrecoverable. Recovery runs next, under the control database lock. Stale
/// part files are cleaned up last, on a separate connection opened after that
/// lock is released. The pass stops early — leaving the control plane
/// untouched — when the Remote Repository is unavailable, and stops after
/// projection when the control database lock is poisoned.
pub(crate) fn recover_control_plane(
    remote: &RemoteState,
    app_data_dir: &Path,
    libraries: &[RegisteredLibrary],
    digest_resolver: &dyn DigestResolver,
    clock: &Clock,
) -> StartupReport {
    let mut report = StartupReport::default();

    if !remote.is_available() {
        tracing::warn!("remote repository state is unavailable; skipping recovery");
        return report;
    }

    project_library_outboxes(remote, libraries, clock, &mut report);

    let recovery_result = {
        let control = match remote.control_db().and_then(|db| {
            db.lock().map_err(|_| {
                crate::commands::error::state_lock_error("control DB lock was poisoned")
            })
        }) {
            Ok(control) => control,
            Err(_) => {
                tracing::warn!("remote control DB lock was poisoned during recovery");
                return report;
            }
        };

        run_recovery(&control, digest_resolver, clock)
    };

    match recovery_result {
        Ok(recovery) => report.recovery = Some(recovery),
        Err(error) => tracing::warn!("remote control DB recovery failed: {:?}", error),
    }

    recover_stale_part_files_for_libraries(app_data_dir, libraries, &mut report);

    report
}

fn registered_libraries(app_data_dir: &Path) -> Vec<RegisteredLibrary> {
    crate::config::load_config(app_data_dir)
        .ok()
        .flatten()
        .map(|config| config.libraries)
        .unwrap_or_default()
}

fn working_copy_digest_resolver(libraries: Vec<RegisteredLibrary>) -> impl DigestResolver {
    FileDigestResolver::new(move |library_id: &str| {
        let library = libraries.iter().find(|entry| entry.id() == library_id)?;
        let root = LibraryRoot::open(&library.working_copy_root()?).ok()?;
        Some(root.database_path())
    })
}

fn project_library_outboxes(
    remote: &RemoteState,
    libraries: &[RegisteredLibrary],
    clock: &Clock,
    report: &mut StartupReport,
) {
    for library in libraries {
        if !matches!(library, RegisteredLibrary::Remote { .. }) {
            continue;
        }
        let library_id = library.id().to_owned();
        let Some(root_path) = library.working_copy_root() else {
            continue;
        };
        let Ok(root) = LibraryRoot::open(&root_path) else {
            continue;
        };
        let Ok(library_db) = crate::cache::open_database(&root.database_path()) else {
            continue;
        };
        if let Err(error) = crate::cache::apply_migrations(&library_db) {
            tracing::warn!(
                "library migrations failed during outbox projection for {}: {error}",
                root_path.display()
            );
            continue;
        }
        let rows = match list_unprojected_library_outbox(&library_db) {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(
                    "failed to list library outbox for {}: {:?}",
                    root_path.display(),
                    error
                );
                continue;
            }
        };
        if rows.is_empty() {
            continue;
        }
        let Ok(control_db) = remote.control_db() else {
            tracing::warn!("control DB unavailable during outbox projection");
            continue;
        };
        let Ok(control) = control_db.lock() else {
            tracing::warn!("control DB lock poisoned during outbox projection");
            continue;
        };
        let now = (clock)();
        for row in rows {
            if row.song_ids.is_empty() && !row.whole_repository {
                continue;
            }
            // Only remove the library outbox after control projection succeeds.
            if project_outbox_row(&control, &library_id, &row, now, report) {
                if let Err(error) = delete_library_publish_outbox(&library_db, &row.operation_id) {
                    tracing::warn!(
                        "failed to delete projected outbox {}: {:?}",
                        row.operation_id,
                        error
                    );
                }
            }
        }
    }
}

fn project_outbox_row(
    control: &Connection,
    library_id: &str,
    row: &LibraryPublishOutboxRow,
    now_ms: i64,
    report: &mut StartupReport,
) -> bool {
    match get_operation(control, &row.operation_id) {
        Ok(Some(existing)) if existing.state.is_terminal() => {
            // Terminal row: drop residual outbox only when intent is covered.
            let (terminal_song_ids, terminal_whole_repository) =
                OperationPayload::from_json(&existing.payload_json)
                    .map(|payload| (payload.song_ids, payload.whole_repository))
                    .unwrap_or_default();
            if residual_outbox_safe_to_drop(
                &existing.library_id,
                library_id,
                &row.song_ids,
                row.whole_repository,
                &terminal_song_ids,
                terminal_whole_repository,
            ) {
                report
                    .dropped_residual_outboxes
                    .push(row.operation_id.clone());
                true
            } else {
                tracing::warn!(
                    "residual outbox {} not covered by terminal op; keeping",
                    row.operation_id
                );
                report
                    .retained_residual_outboxes
                    .push(row.operation_id.clone());
                false
            }
        }
        Ok(Some(_)) => bind_projected_scope(control, library_id, row, report),
        Ok(None) => {
            let payload = OperationPayload {
                song_ids: row.song_ids.clone(),
                whole_repository: row.whole_repository,
                percent: 0,
                detail: Some("Recovered from library outbox".to_owned()),
                ..Default::default()
            };
            let payload_json = match payload.to_json() {
                Ok(json) => json,
                Err(error) => {
                    tracing::warn!(
                        "outbox payload serialize failed for {}: {:?}",
                        row.operation_id,
                        error
                    );
                    return false;
                }
            };
            let operation = OperationRow {
                operation_id: row.operation_id.clone(),
                library_id: library_id.to_owned(),
                operation_kind: OperationKind::Publish,
                state: OperationState::Pending,
                expected_generation: row.expected_generation,
                target_generation: None,
                source_db_digest: row.source_db_digest.clone(),
                candidate_db_digest: None,
                payload_json,
                attempt_count: 0,
                next_attempt_at_ms: None,
                error_code: None,
                error_detail: None,
                created_at_ms: row.created_at_ms,
                updated_at_ms: now_ms,
            };
            match upsert_operation(control, &operation) {
                Ok(()) => bind_projected_scope(control, library_id, row, report),
                Err(error) => {
                    tracing::warn!(
                        "control projection upsert failed for {}: {:?}",
                        row.operation_id,
                        error
                    );
                    false
                }
            }
        }
        Err(error) => {
            tracing::warn!(
                "control get_operation failed for {}: {:?}",
                row.operation_id,
                error
            );
            false
        }
    }
}

fn bind_projected_scope(
    control: &Connection,
    library_id: &str,
    row: &LibraryPublishOutboxRow,
    report: &mut StartupReport,
) -> bool {
    let bound = bind_scope_mark_pending_and_dirty_tx(
        control,
        &row.operation_id,
        library_id,
        &row.song_ids,
        row.whole_repository,
    )
    .is_ok();
    if bound {
        report.projected_operations.push(row.operation_id.clone());
    }
    bound
}

/// Residual outbox may drop only when a terminal op covers the same library
/// and payload is a non-empty superset of outbox song ids (not the reverse).
fn residual_outbox_safe_to_drop(
    op_library_id: &str,
    outbox_library_id: &str,
    outbox_song_ids: &[String],
    outbox_whole_repository: bool,
    terminal_payload_song_ids: &[String],
    terminal_whole_repository: bool,
) -> bool {
    if op_library_id != outbox_library_id {
        return false;
    }
    if outbox_whole_repository {
        return terminal_whole_repository;
    }
    if terminal_payload_song_ids.is_empty() {
        return false;
    }
    outbox_song_ids
        .iter()
        .all(|s| terminal_payload_song_ids.contains(s))
}

fn recover_stale_part_files_for_libraries(
    app_data_dir: &Path,
    libraries: &[RegisteredLibrary],
    report: &mut StartupReport,
) {
    let control_db_path = control_db::control_db_path(app_data_dir);
    let Ok(control_db) = control_db::open_control_db(&control_db_path) else {
        // Fail closed: keep all partials when control plane is unavailable.
        tracing::warn!(
            "control DB unavailable during part-file recovery; \
             preserving all partial downloads (fail-closed)"
        );
        return;
    };

    for library in libraries {
        if !matches!(library, RegisteredLibrary::Remote { .. }) {
            continue;
        }
        let Some(root_path) = library.working_copy_root() else {
            continue;
        };
        match recover_stale_part_files(&root_path, &control_db) {
            Ok(removed) => report.removed_part_files.extend(removed),
            Err(error) => tracing::warn!(
                "stale part-file recovery failed for {}: {:?}",
                root_path.display(),
                error
            ),
        }
    }
}

/// Handle to the durable operation executor thread, kept in managed state so
/// teardown can stop the poller and wait for an in-flight pass to finish
/// instead of letting it run against an app that is going away.
pub(crate) struct DurableOperationExecutor {
    stop: mpsc::SyncSender<()>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl DurableOperationExecutor {
    fn shutdown(&self) {
        let _ = self.stop.try_send(());
        let thread = self.thread.lock().ok().and_then(|mut slot| slot.take());
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

impl Drop for DurableOperationExecutor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Stop the durable operation executor and wait for it to finish. Called by
/// the run loop's exit handler; a no-op when the executor was never started.
pub(crate) fn shutdown_durable_operation_executor<R: Runtime>(app_handle: &AppHandle<R>) {
    if let Some(executor) = app_handle.try_state::<DurableOperationExecutor>() {
        executor.shutdown();
    }
}

fn spawn_durable_operation_executor<R: Runtime>(
    state: AppState,
    app_handle: AppHandle<R>,
) -> DurableOperationExecutor {
    let (stop, stop_rx) = mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        let publish_changes = crate::remote::PublishChanges::new(&state, &app_handle);
        run_durable_operation_executor(&stop_rx, DURABLE_OPERATION_POLL_INTERVAL, || {
            if let Err(error) = publish_changes.recover_pending() {
                tracing::warn!("durable operation executor pass failed: {:?}", error);
            }
        });
    });
    DurableOperationExecutor {
        stop,
        thread: Mutex::new(Some(thread)),
    }
}

/// One pass immediately, then one per poll interval, until the stop channel
/// signals or its sender is dropped.
fn run_durable_operation_executor(
    stop: &mpsc::Receiver<()>,
    poll_interval: Duration,
    mut pass: impl FnMut(),
) {
    pass();
    while let Err(mpsc::RecvTimeoutError::Timeout) = stop.recv_timeout(poll_interval) {
        pass();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{RemoteLibraryConnectionConfig, RemoteLibraryProvider};
    use crate::remote::control_db::{get_repository_state, LocalState};
    use crate::remote::library_outbox::upsert_library_publish_outbox;
    use crate::remote::recovery::{MapDigestResolver, NullDigestResolver};
    use std::collections::HashMap;
    use std::sync::MutexGuard;
    use tempfile::TempDir;

    const LIBRARY_ID: &str = "library-remote-1";
    const OTHER_LIBRARY_ID: &str = "library-remote-2";

    struct StartupHarness {
        app_data: TempDir,
        working_copy: PathBuf,
        remote: RemoteState,
        libraries: Vec<RegisteredLibrary>,
    }

    impl StartupHarness {
        fn new() -> Self {
            let app_data = TempDir::new().expect("temp app data dir");
            let working_copy = app_data.path().join("working-copies").join(LIBRARY_ID);
            LibraryRoot::create(&working_copy).expect("library working copy");
            let library_db = crate::cache::open_database(&working_copy.join("openkara.db"))
                .expect("library database");
            crate::cache::apply_migrations(&library_db).expect("library migrations");
            drop(library_db);

            let remote = RemoteState::new(app_data.path());
            assert!(remote.is_available(), "remote control plane must be open");

            let library = RegisteredLibrary::remote(
                LIBRARY_ID.to_owned(),
                "Remote Repository".to_owned(),
                RemoteLibraryProvider::WebDav,
                "account-1".to_owned(),
                "/OpenKara".to_owned(),
                "/OpenKara".to_owned(),
                Some(RemoteLibraryConnectionConfig::WebDav {
                    server_url: "https://example.invalid".to_owned(),
                }),
                Some(working_copy.join("openkara.db").display().to_string()),
                None,
            );

            Self {
                app_data,
                working_copy,
                remote,
                libraries: vec![library],
            }
        }

        fn library_db(&self) -> Connection {
            crate::cache::open_database(&self.working_copy.join("openkara.db"))
                .expect("library database")
        }

        fn control_db(&self) -> MutexGuard<'_, Connection> {
            self.remote
                .control_db()
                .expect("control database")
                .lock()
                .expect("control database lock")
        }

        fn run(&self, digest_resolver: &dyn DigestResolver, now_ms: i64) -> StartupReport {
            let clock: Clock = Box::new(move || now_ms);
            recover_control_plane(
                &self.remote,
                self.app_data.path(),
                &self.libraries,
                digest_resolver,
                &clock,
            )
        }
    }

    fn outbox_row(operation_id: &str, song_ids: &[&str]) -> LibraryPublishOutboxRow {
        LibraryPublishOutboxRow {
            operation_id: operation_id.to_owned(),
            song_ids: song_ids.iter().map(|id| (*id).to_owned()).collect(),
            whole_repository: false,
            expected_generation: Some(3),
            source_db_digest: Some("source-digest".to_owned()),
            created_at_ms: 10,
            projected_at_ms: None,
        }
    }

    fn operation(
        operation_id: &str,
        state: OperationState,
        song_ids: &[&str],
        source_db_digest: Option<&str>,
    ) -> OperationRow {
        let payload = OperationPayload {
            song_ids: song_ids.iter().map(|id| (*id).to_owned()).collect(),
            ..Default::default()
        };
        OperationRow {
            operation_id: operation_id.to_owned(),
            library_id: LIBRARY_ID.to_owned(),
            operation_kind: OperationKind::Publish,
            state,
            expected_generation: None,
            target_generation: None,
            source_db_digest: source_db_digest.map(str::to_owned),
            candidate_db_digest: None,
            payload_json: payload.to_json().expect("payload json"),
            attempt_count: 0,
            next_attempt_at_ms: None,
            error_code: None,
            error_detail: None,
            created_at_ms: 10,
            updated_at_ms: 10,
        }
    }

    #[test]
    fn unprojected_outbox_lands_in_the_control_database() {
        let harness = StartupHarness::new();
        upsert_library_publish_outbox(
            &harness.library_db(),
            &outbox_row("op-publish", &["song-a", "song-b"]),
        )
        .expect("outbox row");

        let report = harness.run(&NullDigestResolver, 5_000);

        assert_eq!(report.projected_operations, vec!["op-publish".to_owned()]);
        let control = harness.control_db();
        let projected = get_operation(&control, "op-publish")
            .expect("control query")
            .expect("projected operation");
        assert_eq!(projected.state, OperationState::Pending);
        assert_eq!(projected.library_id, LIBRARY_ID);
        assert_eq!(projected.expected_generation, Some(3));
        assert_eq!(projected.created_at_ms, 10);
        let payload = OperationPayload::from_json(&projected.payload_json).expect("payload");
        assert_eq!(payload.song_ids, vec!["song-a", "song-b"]);
        assert_eq!(
            get_repository_state(&control, LIBRARY_ID)
                .expect("repository query")
                .expect("repository state")
                .local_state,
            LocalState::Dirty
        );
        drop(control);

        assert!(
            list_unprojected_library_outbox(&harness.library_db())
                .expect("outbox query")
                .is_empty(),
            "projected outbox rows are removed from the working copy"
        );
    }

    #[test]
    fn residual_outbox_covered_by_a_terminal_operation_is_dropped() {
        let harness = StartupHarness::new();
        upsert_operation(
            &harness.control_db(),
            &operation(
                "op-done",
                OperationState::Completed,
                &["song-a", "song-b"],
                None,
            ),
        )
        .expect("terminal operation");
        upsert_library_publish_outbox(&harness.library_db(), &outbox_row("op-done", &["song-a"]))
            .expect("outbox row");

        let report = harness.run(&NullDigestResolver, 5_000);

        assert_eq!(report.dropped_residual_outboxes, vec!["op-done".to_owned()]);
        assert!(report.retained_residual_outboxes.is_empty());
        assert!(list_unprojected_library_outbox(&harness.library_db())
            .expect("outbox query")
            .is_empty());
        assert_eq!(
            get_operation(&harness.control_db(), "op-done")
                .expect("control query")
                .expect("terminal operation")
                .state,
            OperationState::Completed,
            "a covered residual outbox must never reopen the terminal operation"
        );
    }

    #[test]
    fn residual_outbox_wider_than_the_terminal_operation_is_preserved() {
        let harness = StartupHarness::new();
        upsert_operation(
            &harness.control_db(),
            &operation("op-done", OperationState::Completed, &["song-a"], None),
        )
        .expect("terminal operation");
        upsert_library_publish_outbox(
            &harness.library_db(),
            &outbox_row("op-done", &["song-a", "song-b"]),
        )
        .expect("outbox row");

        let report = harness.run(&NullDigestResolver, 5_000);

        assert_eq!(
            report.retained_residual_outboxes,
            vec!["op-done".to_owned()]
        );
        assert!(report.dropped_residual_outboxes.is_empty());
        assert_eq!(
            list_unprojected_library_outbox(&harness.library_db())
                .expect("outbox query")
                .len(),
            1,
            "uncovered publish intent must survive the restart"
        );
    }

    #[test]
    fn interrupted_publish_changes_work_is_recovered() {
        let harness = StartupHarness::new();
        {
            let control = harness.control_db();
            upsert_operation(
                &control,
                &operation("op-running", OperationState::Running, &["song-a"], None),
            )
            .expect("running operation");
            upsert_operation(
                &control,
                &operation(
                    "op-prepared",
                    OperationState::Prepared,
                    &["song-b"],
                    Some("digest-before-mutation"),
                ),
            )
            .expect("prepared operation");
        }

        let resolver = MapDigestResolver::new(HashMap::from([(
            LIBRARY_ID.to_owned(),
            "digest-after-mutation".to_owned(),
        )]));
        let report = harness.run(&resolver, 5_000);

        let recovery = report.recovery.expect("recovery pass ran");
        assert_eq!(recovery.transitioned_to_retry_wait, vec!["op-running"]);
        assert_eq!(recovery.transitioned_to_pending, vec!["op-prepared"]);

        let control = harness.control_db();
        let running = get_operation(&control, "op-running")
            .expect("control query")
            .expect("running operation");
        assert_eq!(running.state, OperationState::RetryWait);
        assert_eq!(
            running.updated_at_ms, 5_000,
            "the injected clock is the time base for recovery writes"
        );
        assert_eq!(
            get_operation(&control, "op-prepared")
                .expect("control query")
                .expect("prepared operation")
                .state,
            OperationState::Pending
        );
        assert_eq!(
            get_repository_state(&control, LIBRARY_ID)
                .expect("repository query")
                .expect("repository state")
                .local_state,
            LocalState::Dirty
        );
    }

    #[test]
    fn projection_runs_before_recovery_so_outbox_intent_is_recovered_in_one_pass() {
        let harness = StartupHarness::new();
        upsert_operation(
            &harness.control_db(),
            &operation(
                "op-prepared",
                OperationState::Prepared,
                &[],
                Some("digest-before-mutation"),
            ),
        )
        .expect("prepared operation");
        upsert_library_publish_outbox(
            &harness.library_db(),
            &outbox_row("op-prepared", &["song-a"]),
        )
        .expect("outbox row");

        let resolver = MapDigestResolver::new(HashMap::from([(
            LIBRARY_ID.to_owned(),
            "digest-after-mutation".to_owned(),
        )]));
        let report = harness.run(&resolver, 5_000);

        assert_eq!(report.projected_operations, vec!["op-prepared".to_owned()]);
        let recovery = report.recovery.expect("recovery pass ran");
        assert!(
            recovery.cancelled.is_empty(),
            "projection must restore the song identity before recovery judges it"
        );
        let control = harness.control_db();
        let recovered = get_operation(&control, "op-prepared")
            .expect("control query")
            .expect("recovered operation");
        assert_eq!(recovered.state, OperationState::Pending);
        assert_eq!(
            OperationPayload::from_json(&recovered.payload_json)
                .expect("payload")
                .song_ids,
            vec!["song-a"]
        );
    }

    #[test]
    fn orphaned_part_files_in_the_working_copy_are_removed() {
        let harness = StartupHarness::new();
        let orphan = harness
            .working_copy
            .join("media")
            .join("song.mp3.part.op-gone");
        std::fs::write(&orphan, b"partial").expect("orphan part file");

        let report = harness.run(&NullDigestResolver, 5_000);

        assert_eq!(report.removed_part_files, vec![orphan.clone()]);
        assert!(!orphan.exists());
    }

    #[test]
    fn equal_sets_are_safe() {
        let ids = vec!["a".to_owned(), "b".to_owned()];
        assert!(residual_outbox_safe_to_drop(
            LIBRARY_ID, LIBRARY_ID, &ids, false, &ids, false
        ));
    }

    #[test]
    fn outbox_subset_of_payload_is_safe() {
        let outbox = vec!["a".to_owned()];
        let payload = vec!["a".to_owned(), "b".to_owned()];
        assert!(residual_outbox_safe_to_drop(
            LIBRARY_ID, LIBRARY_ID, &outbox, false, &payload, false
        ));
    }

    #[test]
    fn payload_subset_of_outbox_is_not_safe() {
        let outbox = vec!["a".to_owned(), "b".to_owned()];
        let payload = vec!["a".to_owned()];
        assert!(!residual_outbox_safe_to_drop(
            LIBRARY_ID, LIBRARY_ID, &outbox, false, &payload, false
        ));
    }

    #[test]
    fn empty_terminal_payload_never_authorizes_delete() {
        let outbox = vec!["a".to_owned()];
        let payload: Vec<String> = vec![];
        assert!(!residual_outbox_safe_to_drop(
            LIBRARY_ID, LIBRARY_ID, &outbox, false, &payload, false
        ));
    }

    #[test]
    fn library_mismatch_is_not_safe() {
        let ids = vec!["a".to_owned()];
        assert!(!residual_outbox_safe_to_drop(
            LIBRARY_ID,
            OTHER_LIBRARY_ID,
            &ids,
            false,
            &ids,
            false
        ));
    }

    #[test]
    fn matching_whole_repository_scopes_are_safe() {
        assert!(residual_outbox_safe_to_drop(
            LIBRARY_ID,
            LIBRARY_ID,
            &[],
            true,
            &[],
            true
        ));
        assert!(!residual_outbox_safe_to_drop(
            LIBRARY_ID,
            LIBRARY_ID,
            &[],
            true,
            &[],
            false
        ));
    }

    #[test]
    fn the_executor_loop_stops_when_signalled() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::time::Instant;

        let (stop, stop_rx) = mpsc::sync_channel(1);
        let passes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&passes);
        let executor = std::thread::spawn(move || {
            run_durable_operation_executor(&stop_rx, Duration::from_millis(2), move || {
                counter.fetch_add(1, Ordering::SeqCst);
            });
        });

        let deadline = Instant::now() + Duration::from_secs(10);
        while passes.load(Ordering::SeqCst) < 3 {
            assert!(
                Instant::now() < deadline,
                "the executor should keep polling"
            );
            std::thread::sleep(Duration::from_millis(1));
        }

        stop.try_send(()).expect("stop signal");
        executor
            .join()
            .expect("the executor thread should stop when signalled");
    }

    #[test]
    fn the_executor_loop_stops_when_the_stop_handle_is_dropped() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let (stop, stop_rx) = mpsc::sync_channel::<()>(1);
        drop(stop);

        let passes = AtomicUsize::new(0);
        run_durable_operation_executor(&stop_rx, Duration::from_secs(60), || {
            passes.fetch_add(1, Ordering::SeqCst);
        });

        assert_eq!(
            passes.load(Ordering::SeqCst),
            1,
            "only the initial pass runs once the handle is gone"
        );
    }

    #[test]
    fn a_spawned_executor_joins_on_shutdown() {
        let app = tauri::test::mock_app();
        let executor =
            spawn_durable_operation_executor(AppState::test_fixture(), app.handle().clone());

        executor.shutdown();

        let thread = executor.thread.lock().expect("thread slot");
        assert!(thread.is_none(), "shutdown must join the executor thread");
    }
}
