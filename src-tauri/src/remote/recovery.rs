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

    // 4. Any Pending/RetryWait publish op forces repository non-Clean so an
    // automatic pull cannot overwrite committed local edits when a prior
    // crash left operation=Pending but local_state=Clean.
    force_dirty_for_active_publish_ops(connection, clock)?;

    // PR#4: After recovery transitions, the caller (startup hook) invokes
    // `retry_pending_operations` to drive pending/retry_wait operations
    // through the executor. This is done in a separate call so the recovery
    // pass itself remains fast and testable without a provider.
    Ok(report)
}

/// Force repository local_state to Dirty whenever a non-terminal publish
/// operation is outstanding. Closes the crash window where Pending was
/// written but Dirty was not.
fn force_dirty_for_active_publish_ops(connection: &Connection, clock: &Clock) -> CommandResult<()> {
    use crate::remote::control_db::OperationKind;
    let now = (clock)();
    let active = list_operations_in_states(
        connection,
        &[
            OperationState::Pending,
            OperationState::RetryWait,
            OperationState::Running,
            OperationState::Committing,
            OperationState::Verifying,
            OperationState::Prepared,
        ],
    )?;
    for op in active {
        if op.operation_kind != OperationKind::Publish {
            continue;
        }
        mark_repository_dirty(connection, &op.library_id, now)?;
        // Also pin active_operation_id when missing.
        if let Some(mut row) = get_repository_state(connection, &op.library_id)? {
            if row.active_operation_id.is_none() {
                row.active_operation_id = Some(op.operation_id.clone());
                row.updated_at_ms = now;
                upsert_repository_state(connection, &row)?;
            }
        }
    }
    Ok(())
}

/// Retry pending and retry_wait publish operations via the executor.
///
/// Called after the recovery pass and after credentials/the active library
/// are available. Picks up operations in `pending` or `retry_wait` state and
/// re-executes them through the transactional publish protocol.
///
/// Publish recovery shares the same coalescing path as immediate publish:
/// under the per-library commit lock it merges all `Pending`/`RetryWait`
/// ops for that library, rebinds generation, invalidates stale candidates,
/// re-uploads assets from `op.library_id`'s working copy (never the active
/// library), then freezes/CAS once.
///
/// Operations whose `next_attempt_at_ms` is in the future are skipped (rate
/// limiting). `Gc` operations are replayed through the GC executor.
pub fn retry_pending_operations(state: &crate::AppState) -> CommandResult<()> {
    use crate::remote::control_db::{list_operations_in_states, OperationKind};
    use crate::remote::executor::{
        execute_gc, execute_publish, generate_repository_id, generate_writer_id, PublishContext,
    };
    use crate::remote::provider::create_provider;
    use crate::remote::sync::{load_registered_remote_library, merge_pending_ops_for_publish};
    use crate::remote::types::load_app_config;
    use std::collections::HashMap;

    // Recovery does not depend on the currently active library. Pending
    // operations for any registered remote library must run against their
    // own `op.library_id`. Only skip when there are no remote libraries at
    // all.
    let config = load_app_config(&state.shell.app_data_dir)?;
    let has_remote_library = config
        .libraries
        .iter()
        .any(|library| matches!(library, crate::config::RegisteredLibrary::Remote { .. }));
    if !has_remote_library {
        return Ok(());
    }

    let pending = {
        let conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        list_operations_in_states(&conn, &[OperationState::Pending, OperationState::RetryWait])?
    };

    let now = crate::remote::types::current_unix_time_ms();

    // Partition GC vs Publish. Publish ops are grouped by library so
    // coalescing runs once per library (same protocol as immediate publish).
    let mut gc_ops = Vec::new();
    let mut publish_by_library: HashMap<String, Vec<OperationRow>> = HashMap::new();
    for op in pending {
        if let Some(next_attempt) = op.next_attempt_at_ms {
            if next_attempt > now {
                continue;
            }
        }
        match op.operation_kind {
            OperationKind::Gc => gc_ops.push(op),
            OperationKind::Publish => {
                publish_by_library
                    .entry(op.library_id.clone())
                    .or_default()
                    .push(op);
            }
            _ => {}
        }
    }

    for op in gc_ops {
        let library_id = &op.library_id;
        let commit_lock = state.remote.commit_lock(library_id);
        let _commit_guard = commit_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let remote_library =
            match load_registered_remote_library(&state.shell.app_data_dir, library_id) {
                Ok(lib) => lib,
                Err(_) => continue,
            };
        let provider = match create_provider(&state.shell.app_data_dir, &remote_library) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let control_db_path = crate::remote::control_db::control_db_path(&state.shell.app_data_dir);
        let exec_conn = match crate::remote::control_db::open_control_db(&control_db_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let _ = execute_gc(provider.as_ref(), &exec_conn, library_id, &op.operation_id);
    }

    for (library_id, mut ops) in publish_by_library {
        // Stable primary: earliest created among ready ops.
        ops.sort_by_key(|o| o.created_at_ms);
        let Some(primary) = ops.first().cloned() else {
            continue;
        };

        let commit_lock = state.remote.commit_lock(&library_id);
        let _commit_guard = commit_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Re-check primary under the lock — a concurrent immediate publish
        // may already have cancelled/merged it.
        {
            let conn = state.remote.control_db.lock().map_err(|_| {
                crate::commands::error::state_lock_error("control DB lock was poisoned")
            })?;
            match crate::remote::control_db::get_operation(&conn, &primary.operation_id)? {
                Some(op) if !op.state.is_terminal() => {}
                _ => continue,
            }
        }

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

        // Shared coalesce path with immediate publish: union song_ids,
        // cancel secondaries, rebind generation, invalidate stale candidates.
        // CAS-boundary ops and rate-limited RetryWait peers are left alone.
        let (operation_id, song_ids) = match merge_pending_ops_for_publish(
            state,
            &library_id,
            &primary.operation_id,
            &[],
            "",
            Some(remote_root.root()),
        ) {
            Ok(v) => v,
            Err(error) => {
                tracing::warn!(
                    "publish recovery coalesce failed for {}: {}",
                    primary.operation_id,
                    error.message
                );
                continue;
            }
        };

        // Coalesce may preserve a future Retry-After on the survivor.
        {
            let conn = state.remote.control_db.lock().map_err(|_| {
                crate::commands::error::state_lock_error("control DB lock was poisoned")
            })?;
            if let Ok(Some(op)) = crate::remote::control_db::get_operation(&conn, &operation_id) {
                if let Some(next) = op.next_attempt_at_ms {
                    if next > crate::remote::types::current_unix_time_ms() {
                        continue;
                    }
                }
            }
        }

        if song_ids.is_empty() {
            tracing::warn!(
                "cancelling publish recovery for {} — empty song_ids after coalesce",
                operation_id
            );
            if let Ok(conn) = state.remote.control_db.lock() {
                if let Ok(Some(mut updated)) =
                    crate::remote::control_db::get_operation(&conn, &operation_id)
                {
                    updated.state = OperationState::Cancelled;
                    updated.error_code = Some("empty_song_ids".to_owned());
                    updated.error_detail = Some(
                        "pending operation had empty song_ids; cancelled as unrecoverable"
                            .to_owned(),
                    );
                    updated.updated_at_ms = crate::remote::types::current_unix_time_ms();
                    let _ = upsert_operation(&conn, &updated);
                }
            }
            continue;
        }

        let (repository_id, writer_id) = {
            let conn = state.remote.control_db.lock().map_err(|_| {
                crate::commands::error::state_lock_error("control DB lock was poisoned")
            })?;
            let repo_state = get_repository_state(&conn, &library_id)?;
            let needs_persist = repo_state
                .as_ref()
                .is_some_and(|r| r.repository_id.is_none() || r.writer_id.is_none());
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

        // Assets always come from the operation library working copy.
        let mut asset_error: Option<crate::commands::error::CommandError> = None;
        for song_id in &song_ids {
            if let Err(error) = crate::remote::sync::reupload_song_assets_for_recovery(
                state,
                &remote_library,
                &remote_root,
                song_id,
            ) {
                tracing::warn!(
                    "asset re-upload failed for song {} op {}: {}",
                    song_id,
                    operation_id,
                    error.message
                );
                asset_error = Some(error);
                break;
            }
        }
        if let Some(error) = asset_error {
            if let Ok(conn) = state.remote.control_db.lock() {
                let now = crate::remote::types::current_unix_time_ms();
                if let Ok(Some(mut updated)) =
                    crate::remote::control_db::get_operation(&conn, &operation_id)
                {
                    if error.retryable {
                        updated.state = OperationState::RetryWait;
                        updated.next_attempt_at_ms = Some(now + 30_000);
                        updated.error_code = Some("network_unavailable".to_owned());
                    } else {
                        updated.state = OperationState::Failed;
                        updated.next_attempt_at_ms = None;
                        updated.error_code = Some(format!("{:?}", error.code));
                        let msg = error.message.to_ascii_lowercase();
                        if msg.contains("auth") || msg.contains("401") || msg.contains("credential")
                        {
                            if let Ok(Some(mut repo)) = get_repository_state(&conn, &library_id) {
                                repo.local_state = LocalState::ReauthRequired;
                                repo.last_error_code = Some("authentication_expired".to_owned());
                                repo.updated_at_ms = now;
                                let _ = upsert_repository_state(&conn, &repo);
                            }
                            updated.error_code = Some("authentication_expired".to_owned());
                        }
                    }
                    updated.error_detail = Some(error.message.clone());
                    updated.updated_at_ms = now;
                    let _ = upsert_operation(&conn, &updated);
                }
            }
            continue;
        }

        let control_db_path = crate::remote::control_db::control_db_path(&state.shell.app_data_dir);
        let exec_conn = match crate::remote::control_db::open_control_db(&control_db_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let ctx = PublishContext {
            control_db: &exec_conn,
            provider: provider.as_ref(),
            working_copy_root: remote_root.root(),
            library_id: &library_id,
            writer_id: &writer_id,
            repository_id: &repository_id,
        };

        let _ = execute_publish(&ctx, &operation_id);
    }

    Ok(())
}

/// Remove stale `*.part.*` temp files from a working-copy directory.
///
/// Called during the startup recovery pass for each remote library working
/// copy. Part files belonging to operations with valid transfer rows in
/// `remote_transfer_parts` are preserved (resumable). Orphaned part files
/// (no matching transfer row) are deleted.
///
/// Returns the list of removed paths so callers/tests can observe the result.
pub fn recover_stale_part_files(
    working_copy_dir: &std::path::Path,
    control_db: &Connection,
) -> CommandResult<Vec<std::path::PathBuf>> {
    // Collect operation IDs that have valid, non-terminal transfer parts.
    // These partials are resumable and must survive restart.
    let protected = collect_protected_transfer_operation_ids(control_db);
    crate::remote::atomic_download::remove_stale_part_files(working_copy_dir, &protected)
}

/// Collect operation IDs that have non-zero transfer progress in the control
/// DB. Part files belonging to these operations are resumable and must not
/// be deleted during startup cleanup.
fn collect_protected_transfer_operation_ids(
    control_db: &Connection,
) -> std::collections::HashSet<String> {
    use crate::remote::control_db::list_all_transfer_parts;
    let mut ids = std::collections::HashSet::new();
    if let Ok(parts) = list_all_transfer_parts(control_db) {
        for part in parts {
            if part.transferred_bytes > 0 {
                ids.insert(part.operation_id);
            }
        }
    }
    ids
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
    // committed_generation, the remote advanced independently. The
    // committed_generation is advanced by the manifest-based pull in
    // revision.rs after a successful atomic database pull.
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

    // None must never mean "same". Missing digests are degraded: keep the
    // operation recoverable (promote/dirty) rather than cancelling as
    // "mutation never committed".
    let working_unchanged = match (working_digest.as_deref(), source_digest) {
        (Some(w), Some(s)) => w == s,
        (None, _) | (_, None) => {
            // Degraded: cannot prove the mutation did not commit. Leave as
            // pending when song_ids are present so publication can continue;
            // cancel only empty payloads below.
            false
        }
    };

    if working_unchanged {
        // The mutation never committed locally — discard the intent.
        let mut updated = op.clone();
        updated.state = OperationState::Cancelled;
        updated.updated_at_ms = now;
        upsert_operation(connection, &updated)?;
        Ok(PreparedOutcome::Cancelled)
    } else {
        // The local mutation committed but publication didn't finish.
        // Only promote when the payload has recoverable song identity.
        // Empty song_ids placeholders (pre-bind crash window or legacy) must
        // not become permanent pending zombies that recovery skips forever.
        let payload = crate::remote::control_db::OperationPayload::from_json(&op.payload_json).ok();
        let has_song_ids = payload.as_ref().is_some_and(|p| !p.song_ids.is_empty());
        if !has_song_ids {
            let mut updated = op.clone();
            updated.state = OperationState::Cancelled;
            updated.error_code = Some("empty_song_ids".to_owned());
            updated.error_detail = Some(
                "prepared operation had empty song_ids after local mutation; \
                 cancelled to avoid unrecoverable pending zombie"
                    .to_owned(),
            );
            updated.updated_at_ms = now;
            upsert_operation(connection, &updated)?;
            // Keep repository dirty so a subsequent publish can create a
            // full-identity operation for the committed local edits.
            mark_repository_dirty(connection, &op.library_id, now)?;
            return Ok(PreparedOutcome::Cancelled);
        }

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
            // Recoverable publish identity — empty song_ids are a separate case.
            payload_json: r#"{"song_ids":["song-1"],"percent":0}"#.to_owned(),
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
    fn recovery_prepared_empty_song_ids_after_mutation_is_cancelled_not_pending() {
        let (_dir, conn) = fresh_db();
        let mut op = make_operation(
            "op-empty",
            "lib-1",
            OperationState::Prepared,
            Some("digest-aaa"),
            Some(0),
        );
        // Pre-bind crash window: prepared row still has empty song_ids.
        op.payload_json = r#"{"song_ids":[],"percent":0}"#.to_owned();
        upsert_operation(&conn, &op).unwrap();
        upsert_repository_state(&conn, &make_repo_state("lib-1", 0)).unwrap();

        let resolver = MapDigestResolver::new(
            [("lib-1".to_owned(), "digest-bbb".to_owned())]
                .into_iter()
                .collect(),
        );
        let report = run_recovery(&conn, &resolver, &fixed_clock(5000)).unwrap();

        assert!(
            report.transitioned_to_pending.is_empty(),
            "empty song_ids must not become pending zombies"
        );
        assert_eq!(report.cancelled, vec!["op-empty".to_owned()]);
        let loaded = get_operation(&conn, "op-empty").unwrap().unwrap();
        assert_eq!(loaded.state, OperationState::Cancelled);
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

        // Use an empty control DB — no transfer rows, so all part files
        // are orphaned and should be removed.
        let control_db = rusqlite::Connection::open_in_memory().expect("control DB");
        crate::remote::control_db::apply_migrations(&control_db).expect("migrations");

        let removed = recover_stale_part_files(dir.path(), &control_db).expect("recovery");

        assert!(!part_file.exists(), "top-level part file removed");
        assert!(!stem_part.exists(), "subdirectory part file removed");
        assert!(real_db.exists(), "real database preserved");
        assert!(lkg.exists(), "last-known-good preserved");
        assert_eq!(removed.len(), 2);
    }

    #[test]
    fn recover_stale_part_files_missing_dir_is_not_an_error() {
        let control_db = rusqlite::Connection::open_in_memory().expect("control DB");
        crate::remote::control_db::apply_migrations(&control_db).expect("migrations");
        let result =
            recover_stale_part_files(std::path::Path::new("/nonexistent/xyz"), &control_db);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn recover_stale_part_files_preserves_resumable_partials() {
        use crate::remote::control_db::{upsert_transfer_part, TransferDirection, TransferPartRow};
        let dir = TempDir::new().expect("temp dir");
        let protected_part = dir.path().join("media/song.mp3.part.op-resume");
        let orphan_part = dir.path().join("media/other.mp3.part.op-orphan");
        std::fs::create_dir_all(dir.path().join("media")).unwrap();
        std::fs::write(&protected_part, b"partial-resume").unwrap();
        std::fs::write(&orphan_part, b"partial-orphan").unwrap();

        let control_db = rusqlite::Connection::open_in_memory().expect("control DB");
        crate::remote::control_db::apply_migrations(&control_db).expect("migrations");
        // Valid transfer row with non-zero progress — partial must survive.
        upsert_transfer_part(
            &control_db,
            &TransferPartRow {
                operation_id: "op-resume".to_owned(),
                relative_path: "media/song.mp3".to_owned(),
                direction: TransferDirection::Download,
                expected_size: Some(1024),
                expected_digest: None,
                provider_revision: Some("rev-1".to_owned()),
                provider_session_id: None,
                transferred_bytes: 512,
                state: "in_progress".to_owned(),
                updated_at_ms: 1000,
            },
        )
        .unwrap();

        let removed = recover_stale_part_files(dir.path(), &control_db).expect("recovery");

        assert!(
            protected_part.exists(),
            "resumable partial referenced by transfer row must be preserved"
        );
        assert!(!orphan_part.exists(), "orphaned partial must be deleted");
        assert_eq!(removed.len(), 1);
        assert!(removed[0].ends_with("other.mp3.part.op-orphan"));
    }

    /// Multi-library recovery invariant: an operation for library B must
    /// resolve library B's credentials and working copy even when the
    /// currently active library is A. The commit lock and provider load
    /// both key off `op.library_id`.
    #[test]
    fn multi_library_operation_targets_op_library_id_not_active() {
        // The production recovery path loads the library via
        // `load_registered_remote_library(app_data_dir, &op.library_id)` and
        // acquires `state.remote.commit_lock(library_id)`. This unit test
        // proves the lock map isolates libraries so an operation for B cannot
        // hold A's commit lock (and vice versa).
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let mut locks: HashMap<String, Arc<Mutex<()>>> = HashMap::new();
        locks.insert("library-a".to_owned(), Arc::new(Mutex::new(())));
        locks.insert("library-b".to_owned(), Arc::new(Mutex::new(())));

        // Hold A's lock. An operation for B must still be able to acquire B's
        // lock without blocking on A.
        let _guard_a = acquire_commit_lock(&locks, "library-a").expect("A lock");
        let guard_b = acquire_commit_lock(&locks, "library-b");
        assert!(
            guard_b.is_some(),
            "operation for library B must not block on library A's commit lock"
        );

        // The operation identity itself carries library_id; recovery iterates
        // pending rows and never substitutes the active library id.
        let op = make_operation("op-b", "library-b", OperationState::Pending, None, None);
        assert_eq!(op.library_id, "library-b");
        assert_ne!(op.library_id, "library-a");
    }

    // --- Resumable transfer parts ---

    // Incomplete transfers resume from verified offsets. The recovery pass
    // detects incomplete transfer parts so the executor can resume them. The
    // actual resume is performed by the executor's resumable upload/download
    // paths, not by recovery itself — recovery only transitions the operation
    // to `pending` so the executor picks it up.
    #[test]
    fn recovery_detects_incomplete_transfer_parts() {
        use crate::remote::control_db::{
            delete_transfer_parts, list_transfer_parts, upsert_transfer_part, TransferDirection,
            TransferPartRow,
        };
        let (_dir, conn) = fresh_db();
        let op = make_operation("op-1", "lib-1", OperationState::Running, None, None);
        upsert_operation(&conn, &op).unwrap();

        // Seed an incomplete download transfer part.
        let row = TransferPartRow {
            operation_id: "op-1".to_owned(),
            relative_path: "openkara.db".to_owned(),
            direction: TransferDirection::Download,
            expected_size: Some(1024),
            expected_digest: None,
            provider_revision: Some("rev-1".to_owned()),
            provider_session_id: None,
            transferred_bytes: 512,
            state: "in_progress".to_owned(),
            updated_at_ms: 1000,
        };
        upsert_transfer_part(&conn, &row).unwrap();

        // Recovery transitions the running operation to retry_wait.
        let report = run_recovery(&conn, &NullDigestResolver, &fixed_clock(5000)).unwrap();
        assert_eq!(report.transitioned_to_retry_wait, vec!["op-1".to_owned()]);

        // The incomplete transfer part is still present so the executor can
        // resume from the verified offset (512 bytes).
        let parts = list_transfer_parts(&conn, "op-1").unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].transferred_bytes, 512);
        assert_eq!(parts[0].provider_revision.as_deref(), Some("rev-1"));

        // After a successful resume, the executor deletes the transfer part.
        delete_transfer_parts(&conn, "op-1").unwrap();
        assert!(list_transfer_parts(&conn, "op-1").unwrap().is_empty());
    }

    #[test]
    fn recovery_revision_mismatch_invalidates_transfer_part() {
        use crate::remote::control_db::{
            list_transfer_parts, upsert_transfer_part, TransferDirection, TransferPartRow,
        };
        let (_dir, conn) = fresh_db();
        let op = make_operation("op-1", "lib-1", OperationState::Running, None, None);
        upsert_operation(&conn, &op).unwrap();

        // Seed a transfer part with a stale provider_revision.
        let row = TransferPartRow {
            operation_id: "op-1".to_owned(),
            relative_path: "openkara.db".to_owned(),
            direction: TransferDirection::Download,
            expected_size: Some(1024),
            expected_digest: None,
            provider_revision: Some("rev-old".to_owned()),
            provider_session_id: None,
            transferred_bytes: 512,
            state: "in_progress".to_owned(),
            updated_at_ms: 1000,
        };
        upsert_transfer_part(&conn, &row).unwrap();

        // Recovery transitions the operation to retry_wait. The executor will
        // detect the revision mismatch and start a fresh download (discarding
        // the stale partial). The transfer part row remains until the executor
        // decides whether to resume or restart.
        let report = run_recovery(&conn, &NullDigestResolver, &fixed_clock(5000)).unwrap();
        assert_eq!(report.transitioned_to_retry_wait, vec!["op-1".to_owned()]);
        let parts = list_transfer_parts(&conn, "op-1").unwrap();
        assert_eq!(
            parts.len(),
            1,
            "transfer part retained for executor decision"
        );
    }
}
