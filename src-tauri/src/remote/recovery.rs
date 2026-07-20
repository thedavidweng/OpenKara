//! Startup recovery for the durable remote control plane.
//!
//! On app startup, after the control DB is migrated and BEFORE remote
//! libraries are loaded for use, a recovery pass transitions interrupted
//! operations to safe states. Actual re-execution of operations is deferred to
//! PR#4/#5 — this module only performs state transitions and scheduling.
//!
//! ## Recovery rules
//!
//! - `running`, `committing`, `verifying`: these were interrupted mid-flight
//!   and must not be assumed complete. Transition to `retry_wait` (or
//!   `pending` if no attempt backoff is needed) with `next_attempt_at_ms` set
//!   to a near-future time.
//! - `prepared` mutation intents: inspect the working DB digest vs
//!   `source_db_digest`:
//!   - unchanged → the mutation never committed locally; mark `cancelled`.
//!   - changed → the local mutation committed but publication didn't finish;
//!     promote to `pending` and mark `remote_repository_state.local_state =
//!     'dirty'`.
//!   - If `expected_generation` differs from the currently-known
//!     `committed_generation`, mark `conflicted`.
//! - `completed` operations stay `completed` and are not re-queued.
//!
//! ## Shutdown safety
//!
//! Because operations are durable, app shutdown does NOT delete rows or
//! pretend to finish active operations. The next startup's recovery pass
//! transitions any interrupted operations to safe retry states. No atexit
//! hooks are needed.

use crate::commands::error::CommandResult;
use crate::remote::control_db::{
    self, get_repository_state, list_operations_in_states, upsert_operation,
    upsert_repository_state, LocalState, OperationRow, OperationState, RepositoryStateRow,
};
use rusqlite::Connection;

/// Near-future offset (ms) applied to `next_attempt_at_ms` for operations
/// transitioned to `retry_wait` during recovery. PR#4's executor will pick
/// these up once credentials and the active library are available.
const RECOVERY_RETRY_OFFSET_MS: i64 = 5_000;

/// Clock abstraction so tests can use deterministic timestamps.
pub type Clock = Box<dyn Fn() -> i64 + Send + Sync>;

/// Trait for resolving the current working DB digest for a library during
/// recovery. Production resolves the active library's DB path; tests inject a
/// fixed digest.
pub trait DigestResolver {
    /// Return the SHA-256 hex digest of the working DB for `library_id`, or
    /// `None` if the library is not available locally.
    fn working_db_digest(&self, library_id: &str) -> Option<String>;
}

/// Result of running the recovery pass, for inspection in tests and logging.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub transitioned_to_retry_wait: Vec<String>,
    pub transitioned_to_pending: Vec<String>,
    pub cancelled: Vec<String>,
    pub conflicted: Vec<String>,
    pub already_completed: usize,
    /// Paths of stale `*.part.*` temp files removed during recovery.
    pub removed_part_files: Vec<std::path::PathBuf>,
}

/// Run the startup recovery pass.
///
/// This transitions interrupted operations to safe states but does NOT
/// re-execute them. PR#4 will drive retry via the operation executor.
///
/// `digest_resolver` provides the current working DB digest for `prepared`
/// operations. `clock` provides the current time in milliseconds.
pub fn run_recovery(
    connection: &Connection,
    digest_resolver: &dyn DigestResolver,
    clock: &Clock,
) -> CommandResult<RecoveryReport> {
    let mut report = RecoveryReport::default();

    // 1. running / committing / verifying → retry_wait (interrupted mid-flight)
    let in_flight = list_operations_in_states(
        connection,
        &[
            OperationState::Running,
            OperationState::Committing,
            OperationState::Verifying,
        ],
    )?;
    for op in in_flight {
        transition_in_flight_to_retry_wait(connection, &op, clock)?;
        report.transitioned_to_retry_wait.push(op.operation_id);
    }

    // 2. prepared mutation intents → cancelled / pending / conflicted
    let prepared = list_operations_in_states(connection, &[OperationState::Prepared])?;
    for op in prepared {
        let outcome = resolve_prepared_operation(connection, &op, digest_resolver, clock)?;
        match outcome {
            PreparedOutcome::Cancelled => report.cancelled.push(op.operation_id),
            PreparedOutcome::PromotedToPending => {
                report.transitioned_to_pending.push(op.operation_id)
            }
            PreparedOutcome::Conflicted => report.conflicted.push(op.operation_id),
        }
    }

    // 3. completed operations stay completed — count for observability
    let completed = list_operations_in_states(connection, &[OperationState::Completed])?;
    report.already_completed = completed.len();

    // PR#4: After recovery transitions, the caller (startup hook) invokes
    // `retry_pending_operations` to drive pending/retry_wait operations
    // through the executor. This is done in a separate call so the recovery
    // pass itself remains fast and testable without a provider.
    Ok(report)
}

/// Retry pending and retry_wait publish operations via the executor.
///
/// Called after the recovery pass and after credentials/the active library
/// are available. Picks up operations in `pending` or `retry_wait` state and
/// re-executes them through the transactional publish protocol.
///
/// Operations whose `next_attempt_at_ms` is in the future are skipped (rate
/// limiting). Operations that are not `Publish` kind are skipped (PR#5 handles
/// other kinds).
#[allow(dead_code)]
pub fn retry_pending_operations(state: &crate::AppState) -> CommandResult<()> {
    use crate::remote::control_db::{list_operations_in_states, OperationKind};
    use crate::remote::executor::{
        execute_publish, generate_repository_id, generate_writer_id, PublishContext,
    };
    use crate::remote::provider::create_provider;
    use crate::remote::sync::{load_registered_remote_library, resolve_active_remote};
    use crate::remote::types::load_app_config;

    let config = load_app_config(&state.shell.app_data_dir)?;
    let Some(remote_library) = resolve_active_remote(&config) else {
        return Ok(());
    };
    let library_id = remote_library.id().to_owned();

    let pending = {
        let conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        list_operations_in_states(&conn, &[OperationState::Pending, OperationState::RetryWait])?
    };

    let now = crate::remote::types::current_unix_time_ms();
    for op in pending {
        // Skip non-publish operations (PR#5 handles other kinds).
        if op.operation_kind != OperationKind::Publish {
            continue;
        }

        // Skip operations that are rate-limited (next_attempt_at_ms in the
        // future).
        if let Some(next_attempt) = op.next_attempt_at_ms {
            if next_attempt > now {
                continue;
            }
        }

        // Reload the library to get the latest revision.
        let remote_library =
            match load_registered_remote_library(&state.shell.app_data_dir, &library_id) {
                Ok(lib) => lib,
                Err(_) => continue,
            };

        let provider = match create_provider(&state.shell.app_data_dir, &remote_library) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let remote_root = match crate::remote::types::load_remote_root(
            &state.shell.app_data_dir,
            &remote_library,
        ) {
            Ok(root) => root,
            Err(_) => continue,
        };

        // Resolve or generate stable repository_id and writer_id.
        // Persist newly generated ids so the next publish/retry uses the
        // same identity instead of generating a different one.
        let (repository_id, writer_id) = {
            let conn = state.remote.control_db.lock().map_err(|_| {
                crate::commands::error::state_lock_error("control DB lock was poisoned")
            })?;
            let repo_state = get_repository_state(&conn, &library_id)?;
            let needs_persist = repo_state.as_ref().map_or(false, |r| {
                r.repository_id.is_none() || r.writer_id.is_none()
            });
            let repository_id = repo_state
                .as_ref()
                .and_then(|r| r.repository_id.clone())
                .unwrap_or_else(generate_repository_id);
            let writer_id = repo_state
                .as_ref()
                .and_then(|r| r.writer_id.clone())
                .unwrap_or_else(generate_writer_id);
            if needs_persist {
                if let Some(mut row) = repo_state {
                    row.repository_id = Some(repository_id.clone());
                    row.writer_id = Some(writer_id.clone());
                    upsert_repository_state(&conn, &row)?;
                }
            }
            (repository_id, writer_id)
        };

        let conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;

        let ctx = PublishContext {
            control_db: &conn,
            provider: provider.as_ref(),
            working_copy_root: remote_root.root(),
            library_id: &library_id,
            writer_id: &writer_id,
            repository_id: &repository_id,
        };

        // Execute the publish protocol. Errors are recorded by the executor
        // in the operation row; we continue to the next operation.
        let _ = execute_publish(&ctx, &op.operation_id);
    }

    Ok(())
}

/// Remove stale `*.part.*` temp files from a working-copy directory.
///
/// Called during the startup recovery pass for each remote library working
/// copy. In PR #3 there are no async transfers, so every `*.part.*` file is
/// stale. PR #5's running transfers must be excluded before removal — see
/// the TODO seam in `atomic_download::remove_stale_part_files`.
///
/// Returns the list of removed paths so callers/tests can observe the result.
pub fn recover_stale_part_files(
    working_copy_dir: &std::path::Path,
) -> CommandResult<Vec<std::path::PathBuf>> {
    crate::remote::atomic_download::remove_stale_part_files(working_copy_dir)
}

/// Transition an interrupted in-flight operation to `retry_wait`.
fn transition_in_flight_to_retry_wait(
    connection: &Connection,
    op: &OperationRow,
    clock: &Clock,
) -> CommandResult<()> {
    let now = (clock)();
    let mut updated = op.clone();
    updated.state = OperationState::RetryWait;
    updated.next_attempt_at_ms = Some(now + RECOVERY_RETRY_OFFSET_MS);
    updated.updated_at_ms = now;
    upsert_operation(connection, &updated)
}

/// Outcome of resolving a `prepared` operation during recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreparedOutcome {
    /// Working DB unchanged → mutation never committed; operation cancelled.
    Cancelled,
    /// Working DB changed → mutation committed; promoted to `pending` + dirty.
    PromotedToPending,
    /// Expected generation differs from committed generation → conflicted.
    Conflicted,
}

/// Resolve a `prepared` operation by comparing digests and generations.
fn resolve_prepared_operation(
    connection: &Connection,
    op: &OperationRow,
    digest_resolver: &dyn DigestResolver,
    clock: &Clock,
) -> CommandResult<PreparedOutcome> {
    let now = (clock)();

    // Conflict check: if expected_generation differs from the currently-known
    // committed_generation, the remote advanced independently.
    // TODO(PR#4): replace revision-based conflict check with manifest
    // generation CAS.
    if let Some(expected_gen) = op.expected_generation {
        if let Some(repo_state) = get_repository_state(connection, &op.library_id)? {
            if repo_state.committed_generation != expected_gen {
                let mut updated = op.clone();
                updated.state = OperationState::Conflicted;
                updated.updated_at_ms = now;
                upsert_operation(connection, &updated)?;
                return Ok(PreparedOutcome::Conflicted);
            }
        }
    }

    let working_digest = digest_resolver.working_db_digest(&op.library_id);
    let source_digest = op.source_db_digest.as_deref();

    let working_unchanged = working_digest.as_deref() == source_digest;

    if working_unchanged {
        // The mutation never committed locally — discard the intent.
        let mut updated = op.clone();
        updated.state = OperationState::Cancelled;
        updated.updated_at_ms = now;
        upsert_operation(connection, &updated)?;
        Ok(PreparedOutcome::Cancelled)
    } else {
        // The local mutation committed but publication didn't finish.
        // Promote to pending and mark the repository dirty.
        let mut updated = op.clone();
        updated.state = OperationState::Pending;
        updated.updated_at_ms = now;
        upsert_operation(connection, &updated)?;

        mark_repository_dirty(connection, &op.library_id, now)?;
        Ok(PreparedOutcome::PromotedToPending)
    }
}

/// Mark a repository's `local_state` as `dirty`. Inserts a row if none exists
/// (e.g. the library was never recorded before the mutation).
fn mark_repository_dirty(
    connection: &Connection,
    library_id: &str,
    now_ms: i64,
) -> CommandResult<()> {
    let existing = get_repository_state(connection, library_id)?;
    let row = match existing {
        Some(mut row) => {
            row.local_state = LocalState::Dirty;
            row.updated_at_ms = now_ms;
            row
        }
        None => RepositoryStateRow {
            library_id: library_id.to_owned(),
            committed_generation: 0,
            committed_manifest_revision: None,
            local_base_generation: 0,
            local_db_digest: None,
            local_state: LocalState::Dirty,
            active_operation_id: None,
            last_success_at_ms: None,
            last_error_code: None,
            updated_at_ms: now_ms,
            repository_id: None,
            writer_id: None,
        },
    };
    upsert_repository_state(connection, &row)
}

/// A `DigestResolver` that always returns `None`. Used when no active library
/// is available (e.g. the control DB is open but libraries are not loaded yet
/// in a minimal test).
// used by recovery tests
#[allow(dead_code)]
pub struct NullDigestResolver;

impl DigestResolver for NullDigestResolver {
    fn working_db_digest(&self, _library_id: &str) -> Option<String> {
        None
    }
}

/// A `DigestResolver` backed by a map of library_id → digest. Used in tests to
/// simulate working DB state without real files.
// used by recovery tests
#[allow(dead_code)]
pub struct MapDigestResolver {
    digests: std::collections::HashMap<String, String>,
}

impl MapDigestResolver {
    // used by recovery tests
    #[allow(dead_code)]
    pub fn new(digests: std::collections::HashMap<String, String>) -> Self {
        Self { digests }
    }
}

impl DigestResolver for MapDigestResolver {
    fn working_db_digest(&self, library_id: &str) -> Option<String> {
        self.digests.get(library_id).cloned()
    }
}

/// A `DigestResolver` that computes the real SHA-256 of the working DB file
/// for a library. The closure maps `library_id` → DB path.
pub struct FileDigestResolver<F>
where
    F: Fn(&str) -> Option<std::path::PathBuf> + Send + Sync,
{
    resolve_path: F,
}

impl<F> FileDigestResolver<F>
where
    F: Fn(&str) -> Option<std::path::PathBuf> + Send + Sync,
{
    pub fn new(resolve_path: F) -> Self {
        Self { resolve_path }
    }
}

impl<F> DigestResolver for FileDigestResolver<F>
where
    F: Fn(&str) -> Option<std::path::PathBuf> + Send + Sync,
{
    fn working_db_digest(&self, library_id: &str) -> Option<String> {
        let path = (self.resolve_path)(library_id)?;
        if !path.exists() {
            return None;
        }
        control_db::sha256_file(&path).ok()
    }
}

/// Helper: acquire the per-library commit lock. Returns a guard that serializes
/// concurrent commit attempts for the same library. Different libraries proceed
/// concurrently.
///
/// This is a no-op placeholder seam for PR#2 — the actual lock map lives in
/// `RemoteState`. This function is provided so PR#4/#5 can call it with a
/// resolved lock without reaching into `RemoteState` internals.
// used by PR#4/#5: operation executor
#[allow(dead_code)]
pub fn acquire_commit_lock<'a>(
    locks: &'a std::collections::HashMap<String, std::sync::Arc<std::sync::Mutex<()>>>,
    library_id: &str,
) -> Option<std::sync::MutexGuard<'a, ()>> {
    locks
        .get(library_id)
        .map(|lock| lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::control_db::{get_operation, open_control_db, OperationKind, OperationRow};
    use tempfile::TempDir;

    fn fresh_db() -> (TempDir, Connection) {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("remote-state.db");
        let conn = open_control_db(&path).expect("open control DB");
        (dir, conn)
    }

    fn fixed_clock(ms: i64) -> Clock {
        Box::new(move || ms)
    }

    fn make_operation(
        id: &str,
        library_id: &str,
        state: OperationState,
        source_db_digest: Option<&str>,
        expected_generation: Option<i64>,
    ) -> OperationRow {
        OperationRow {
            operation_id: id.to_owned(),
            library_id: library_id.to_owned(),
            operation_kind: OperationKind::Publish,
            state,
            expected_generation,
            target_generation: None,
            source_db_digest: source_db_digest.map(|s| s.to_owned()),
            candidate_db_digest: None,
            payload_json: r#"{"song_ids":[],"percent":0}"#.to_owned(),
            attempt_count: 0,
            next_attempt_at_ms: None,
            error_code: None,
            error_detail: None,
            created_at_ms: 1000,
            updated_at_ms: 1000,
        }
    }

    fn make_repo_state(library_id: &str, committed_generation: i64) -> RepositoryStateRow {
        RepositoryStateRow {
            library_id: library_id.to_owned(),
            committed_generation,
            committed_manifest_revision: None,
            local_base_generation: 0,
            local_db_digest: None,
            local_state: LocalState::Clean,
            active_operation_id: None,
            last_success_at_ms: None,
            last_error_code: None,
            updated_at_ms: 1000,
            repository_id: None,
            writer_id: None,
        }
    }

    // --- Crash/restart at every transition ---

    #[test]
    fn recovery_transitions_running_to_retry_wait() {
        let (_dir, conn) = fresh_db();
        let op = make_operation("op-1", "lib-1", OperationState::Running, None, None);
        upsert_operation(&conn, &op).unwrap();

        let report = run_recovery(&conn, &NullDigestResolver, &fixed_clock(5000)).unwrap();

        assert_eq!(report.transitioned_to_retry_wait, vec!["op-1".to_owned()]);
        let loaded = get_operation(&conn, "op-1").unwrap().unwrap();
        assert_eq!(loaded.state, OperationState::RetryWait);
        assert_eq!(
            loaded.next_attempt_at_ms,
            Some(5000 + RECOVERY_RETRY_OFFSET_MS)
        );
    }

    #[test]
    fn recovery_transitions_committing_to_retry_wait() {
        let (_dir, conn) = fresh_db();
        let op = make_operation("op-1", "lib-1", OperationState::Committing, None, None);
        upsert_operation(&conn, &op).unwrap();

        let report = run_recovery(&conn, &NullDigestResolver, &fixed_clock(5000)).unwrap();

        assert_eq!(report.transitioned_to_retry_wait, vec!["op-1".to_owned()]);
        let loaded = get_operation(&conn, "op-1").unwrap().unwrap();
        assert_eq!(loaded.state, OperationState::RetryWait);
    }

    #[test]
    fn recovery_transitions_verifying_to_retry_wait() {
        let (_dir, conn) = fresh_db();
        let op = make_operation("op-1", "lib-1", OperationState::Verifying, None, None);
        upsert_operation(&conn, &op).unwrap();

        let report = run_recovery(&conn, &NullDigestResolver, &fixed_clock(5000)).unwrap();

        assert_eq!(report.transitioned_to_retry_wait, vec!["op-1".to_owned()]);
        let loaded = get_operation(&conn, "op-1").unwrap().unwrap();
        assert_eq!(loaded.state, OperationState::RetryWait);
    }

    #[test]
    fn recovery_transitions_pending_stays_pending() {
        let (_dir, conn) = fresh_db();
        let op = make_operation("op-1", "lib-1", OperationState::Pending, None, None);
        upsert_operation(&conn, &op).unwrap();

        let report = run_recovery(&conn, &NullDigestResolver, &fixed_clock(5000)).unwrap();

        // pending is not in the in-flight set — it should not be transitioned.
        assert!(report.transitioned_to_retry_wait.is_empty());
        assert!(report.transitioned_to_pending.is_empty());
        let loaded = get_operation(&conn, "op-1").unwrap().unwrap();
        assert_eq!(loaded.state, OperationState::Pending);
    }

    #[test]
    fn recovery_transitions_retry_wait_stays_retry_wait() {
        let (_dir, conn) = fresh_db();
        let op = make_operation("op-1", "lib-1", OperationState::RetryWait, None, None);
        upsert_operation(&conn, &op).unwrap();

        let report = run_recovery(&conn, &NullDigestResolver, &fixed_clock(5000)).unwrap();

        assert!(report.transitioned_to_retry_wait.is_empty());
        let loaded = get_operation(&conn, "op-1").unwrap().unwrap();
        assert_eq!(loaded.state, OperationState::RetryWait);
    }

    // --- Completed work does not repeat ---

    #[test]
    fn recovery_completed_stays_completed_and_not_requeued() {
        let (_dir, conn) = fresh_db();
        let op = make_operation("op-1", "lib-1", OperationState::Completed, None, None);
        upsert_operation(&conn, &op).unwrap();

        let report = run_recovery(&conn, &NullDigestResolver, &fixed_clock(5000)).unwrap();

        assert_eq!(report.already_completed, 1);
        assert!(report.transitioned_to_retry_wait.is_empty());
        assert!(report.transitioned_to_pending.is_empty());
        let loaded = get_operation(&conn, "op-1").unwrap().unwrap();
        assert_eq!(loaded.state, OperationState::Completed);
    }

    // --- Prepared local mutation reconstruction from DB digests ---

    #[test]
    fn recovery_prepared_unchanged_db_is_cancelled() {
        let (_dir, conn) = fresh_db();
        let op = make_operation(
            "op-1",
            "lib-1",
            OperationState::Prepared,
            Some("digest-aaa"),
            Some(0),
        );
        upsert_operation(&conn, &op).unwrap();
        upsert_repository_state(&conn, &make_repo_state("lib-1", 0)).unwrap();

        let resolver = MapDigestResolver::new(
            [("lib-1".to_owned(), "digest-aaa".to_owned())]
                .into_iter()
                .collect(),
        );
        let report = run_recovery(&conn, &resolver, &fixed_clock(5000)).unwrap();

        assert_eq!(report.cancelled, vec!["op-1".to_owned()]);
        let loaded = get_operation(&conn, "op-1").unwrap().unwrap();
        assert_eq!(loaded.state, OperationState::Cancelled);
    }

    #[test]
    fn recovery_prepared_changed_db_is_pending_and_dirty() {
        let (_dir, conn) = fresh_db();
        let op = make_operation(
            "op-1",
            "lib-1",
            OperationState::Prepared,
            Some("digest-aaa"),
            Some(0),
        );
        upsert_operation(&conn, &op).unwrap();
        upsert_repository_state(&conn, &make_repo_state("lib-1", 0)).unwrap();

        let resolver = MapDigestResolver::new(
            [("lib-1".to_owned(), "digest-bbb".to_owned())]
                .into_iter()
                .collect(),
        );
        let report = run_recovery(&conn, &resolver, &fixed_clock(5000)).unwrap();

        assert_eq!(report.transitioned_to_pending, vec!["op-1".to_owned()]);
        let loaded = get_operation(&conn, "op-1").unwrap().unwrap();
        assert_eq!(loaded.state, OperationState::Pending);

        let repo = get_repository_state(&conn, "lib-1").unwrap().unwrap();
        assert_eq!(repo.local_state, LocalState::Dirty);
    }

    #[test]
    fn recovery_prepared_generation_mismatch_is_conflicted() {
        let (_dir, conn) = fresh_db();
        let op = make_operation(
            "op-1",
            "lib-1",
            OperationState::Prepared,
            Some("digest-aaa"),
            Some(2),
        );
        upsert_operation(&conn, &op).unwrap();
        // committed_generation advanced to 3 independently
        upsert_repository_state(&conn, &make_repo_state("lib-1", 3)).unwrap();

        let resolver = MapDigestResolver::new(
            [("lib-1".to_owned(), "digest-bbb".to_owned())]
                .into_iter()
                .collect(),
        );
        let report = run_recovery(&conn, &resolver, &fixed_clock(5000)).unwrap();

        assert_eq!(report.conflicted, vec!["op-1".to_owned()]);
        let loaded = get_operation(&conn, "op-1").unwrap().unwrap();
        assert_eq!(loaded.state, OperationState::Conflicted);
    }

    // --- Idempotency: re-running recovery does not create duplicates ---

    #[test]
    fn recovery_is_idempotent() {
        let (_dir, conn) = fresh_db();
        let op = make_operation("op-1", "lib-1", OperationState::Running, None, None);
        upsert_operation(&conn, &op).unwrap();

        let report1 = run_recovery(&conn, &NullDigestResolver, &fixed_clock(5000)).unwrap();
        assert_eq!(report1.transitioned_to_retry_wait, vec!["op-1".to_owned()]);

        // Second run: op-1 is now retry_wait, not in the in-flight set.
        let report2 = run_recovery(&conn, &NullDigestResolver, &fixed_clock(6000)).unwrap();
        assert!(report2.transitioned_to_retry_wait.is_empty());

        // Still exactly one row.
        let all = control_db::list_operations(&conn).unwrap();
        assert_eq!(all.len(), 1);
    }

    // --- Per-library commit serialization ---

    #[test]
    fn commit_lock_serializes_same_library() {
        use std::sync::{Arc, Mutex};
        use std::thread;

        let lock: Arc<Mutex<()>> = Arc::new(Mutex::new(()));
        let mut locks: std::collections::HashMap<String, Arc<Mutex<()>>> =
            std::collections::HashMap::new();
        locks.insert("lib-1".to_owned(), Arc::clone(&lock));

        // Hold the lock from the main thread.
        let guard = lock.lock().unwrap();
        let lock_clone = Arc::clone(&lock);
        let handle = thread::spawn(move || {
            // This blocks until the main thread drops its guard.
            let _g = lock_clone.lock().unwrap();
            true
        });

        // The thread cannot have finished yet because we still hold the lock.
        // Use a short sleep + is_finished check rather than a timed join.
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(
            !handle.is_finished(),
            "concurrent acquire for the same library must block"
        );

        drop(guard);
        assert!(handle.join().expect("thread should complete after unlock"));
    }

    #[test]
    fn commit_lock_different_libraries_proceed_concurrently() {
        use std::sync::{Arc, Mutex};

        let mut locks: std::collections::HashMap<String, Arc<Mutex<()>>> =
            std::collections::HashMap::new();
        locks.insert("lib-1".to_owned(), Arc::new(Mutex::new(())));
        locks.insert("lib-2".to_owned(), Arc::new(Mutex::new(())));

        // Holding lib-1's lock must not block lib-2's lock.
        let _guard1 = acquire_commit_lock(&locks, "lib-1").unwrap();
        let guard2 = acquire_commit_lock(&locks, "lib-2");
        assert!(guard2.is_some());
        // guard2 acquired while guard1 held — different libraries proceed.
    }

    // --- Stale partial-file recovery ---

    #[test]
    fn recover_stale_part_files_removes_part_files_in_working_copy() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        // Simulate a working copy with a stale part file and a real DB.
        let part_file = dir.path().join("openkara.db.part.pull-1");
        std::fs::write(&part_file, b"partial download").unwrap();
        let real_db = dir.path().join("openkara.db");
        std::fs::write(&real_db, b"real database").unwrap();
        let lkg = dir.path().join("openkara.db.lkg");
        std::fs::write(&lkg, b"last known good").unwrap();
        // A part file in a subdirectory (e.g. stems).
        std::fs::create_dir_all(dir.path().join("stems")).unwrap();
        let stem_part = dir.path().join("stems/vocals.wav.part.op-2");
        std::fs::write(&stem_part, b"partial stem").unwrap();

        let removed = recover_stale_part_files(dir.path()).expect("recovery");

        assert!(!part_file.exists(), "top-level part file removed");
        assert!(!stem_part.exists(), "subdirectory part file removed");
        assert!(real_db.exists(), "real database preserved");
        assert!(lkg.exists(), "last-known-good preserved");
        assert_eq!(removed.len(), 2);
    }

    #[test]
    fn recover_stale_part_files_missing_dir_is_not_an_error() {
        let result = recover_stale_part_files(std::path::Path::new("/nonexistent/xyz"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    // --- Placeholder tests for PR#4/#5 ---

    // TODO(PR#5): incomplete transfers resume from verified offsets.
    #[test]
    #[ignore = "PR#5: resumable uploads/downloads from verified offsets"]
    fn recovery_resumes_incomplete_transfers_from_offsets() {
        // PR#5 will implement resume-from-offset using remote_transfer_parts.
    }

    // TODO(PR#4): an accepted remote commit is detected even when the process
    // died before recording success.
    #[test]
    #[ignore = "PR#4: detect accepted remote commit after process death"]
    fn recovery_detects_accepted_commit_after_death() {
        // PR#4 will verify the remote manifest generation to detect commits
        // that succeeded remotely but were not recorded locally.
    }
}
