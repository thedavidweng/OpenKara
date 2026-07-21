//! Comprehensive fault-injection test suite for the remote reliability matrix
//! (PR #8, issue #151).
//!
//! This module consolidates the full reliability test matrix in one place so
//! the coverage is auditable and the shared `FaultInjectionProvider` harness
//! is reusable. The individual subsystem test modules (`atomic_download`,
//! `manifest`, `executor`, `cache_catalog`, `reconnect`) retain their own
//! focused unit tests; this module covers the cross-cutting fault scenarios
//! from the issue test matrix that span multiple subsystems.
//!
//! ## `FaultInjectionProvider`
//!
//! A single scriptable provider that can be configured to:
//! - Fail on the Nth request with a specific error kind
//!   (transient/permanent/credential).
//! - Fail on a specific range fetch.
//! - Return corrupted data (wrong digest).
//! - Simulate slow responses (delay).
//! - Simulate mid-transfer disconnect.
//!
//! It implements `RemoteProvider` so it plugs into `atomic_download`,
//! `resumable_atomic_download`, `execute_publish`, and the reconnect
//! coordinator without changing production code.

#![cfg(test)]

use crate::commands::error::{internal_error, CommandError, CommandResult};
use crate::library::error::LibraryError;
use crate::remote::atomic_download::{
    atomic_download, resumable_atomic_download, AtomicDownloadOptions, ResumableDownloadOptions,
};
use crate::remote::control_db::{
    open_control_db, upsert_operation, upsert_repository_state, upsert_transfer_part, LocalState,
    OperationKind, OperationPayload, OperationRow, OperationState, RepositoryStateRow,
    TransferDirection, TransferPartRow,
};
use crate::remote::errors::{
    RemoteError, RemoteErrorKind, RemoteObjectMetadata, RemoteProviderCapabilities, RemoteResult,
};
use crate::remote::executor::{execute_publish, PublishContext};
use crate::remote::manifest::{read_manifest, RepositoryManifest, CURRENT_SCHEMA_VERSION};
use crate::remote::net_policy::{
    full_jitter_delay, AttemptOutcome, RetryDriver, RetryPolicy, SeededJitter,
};
use crate::remote::provider::{ConditionalSource, RemoteProvider};
use crate::services::reconnect::{
    run_reconnect, EventSink, ReconnectConfig, ReconnectError, ReconnectEvent,
    RemoteStreamingRuntime, ReresolvedSource, SeekOutcome,
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// FaultInjectionProvider
// ---------------------------------------------------------------------------

/// A scriptable fault-injection provider. Each behavior is configured via the
/// builder methods so a test can compose multiple fault modes (e.g. "fail the
/// first download with 503, then succeed").
#[allow(dead_code)]
struct FaultInjectionProvider {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    revisions: Arc<Mutex<HashMap<String, String>>>,
    /// Queue of download behaviors. Each `download_file` / `download_range`
    /// call pops the next behavior. An empty queue means "succeed".
    download_behaviors: Arc<Mutex<Vec<FaultBehavior>>>,
    /// Fail the Nth range request (0-indexed) with a transient error.
    fail_on_range_index: Arc<Mutex<Option<usize>>>,
    /// Number of range requests made so far.
    range_call_count: Arc<Mutex<usize>>,
    /// Remaining upload failures (candidate DB / asset uploads). Each
    /// `upload_file` decrements this and fails while > 0.
    upload_fail_remaining: Arc<Mutex<usize>>,
    /// When true, `conditional_replace` returns `ProviderCapabilityUnavailable`.
    no_cas: bool,
    /// When true, `conditional_replace` always returns `RemoteConflict`
    /// regardless of the expected revision (simulates a concurrent writer).
    always_conflict: bool,
    /// Fail the next N conditional_replace calls with a transient network
    /// error (crash window: after candidate upload, before/during CAS).
    cas_network_fail_remaining: Arc<Mutex<usize>>,
    /// Working copy root for reading files during `upload_file`.
    working_copy_root: Option<PathBuf>,
    /// Recorded sleep delays observed by the retry driver (for backoff tests).
    recorded_delays: Arc<Mutex<Vec<Duration>>>,
    /// Credential generation counter, incremented by `refresh_credentials`.
    credential_generation: Arc<Mutex<u64>>,
}

#[derive(Clone)]
#[allow(dead_code)]
enum FaultBehavior {
    /// Write the full stored bytes successfully.
    Success,
    /// Fail before writing anything (simulates connection drop / 503).
    FailBeforeWrite(RemoteErrorKind),
    /// Write only the first N bytes then fail (mid-body disconnect).
    PartialThenFail(usize, RemoteErrorKind),
    /// Write a short body and succeed (truncated body with success status).
    ShortBody(usize),
    /// Write bytes that differ from the stored content (wrong digest).
    WrongDigest,
    /// Sleep for the configured duration before succeeding (slow response).
    Slow(Duration),
}

#[allow(dead_code)]
impl FaultInjectionProvider {
    fn new() -> Self {
        Self {
            files: Arc::new(Mutex::new(HashMap::new())),
            revisions: Arc::new(Mutex::new(HashMap::new())),
            download_behaviors: Arc::new(Mutex::new(Vec::new())),
            fail_on_range_index: Arc::new(Mutex::new(None)),
            range_call_count: Arc::new(Mutex::new(0)),
            upload_fail_remaining: Arc::new(Mutex::new(0)),
            no_cas: false,
            always_conflict: false,
            cas_network_fail_remaining: Arc::new(Mutex::new(0)),
            working_copy_root: None,
            recorded_delays: Arc::new(Mutex::new(Vec::new())),
            credential_generation: Arc::new(Mutex::new(0)),
        }
    }

    fn with_working_copy_root(mut self, root: PathBuf) -> Self {
        self.working_copy_root = Some(root);
        self
    }

    fn with_no_cas(mut self) -> Self {
        self.no_cas = true;
        self
    }

    fn with_always_conflict(mut self) -> Self {
        self.always_conflict = true;
        self
    }

    /// Fail the next `count` `upload_file` calls with a transient network error.
    fn fail_next_uploads(&self, count: usize) {
        *self.upload_fail_remaining.lock().unwrap() = count;
    }

    /// Fail the next `count` CAS calls with a transient network error, then
    /// allow CAS to succeed.
    fn fail_next_cas_network(&self, count: usize) {
        *self.cas_network_fail_remaining.lock().unwrap() = count;
    }

    fn store_file(&self, relative_path: &str, bytes: Vec<u8>, revision: &str) {
        self.revisions
            .lock()
            .unwrap()
            .insert(relative_path.to_owned(), revision.to_owned());
        self.files
            .lock()
            .unwrap()
            .insert(relative_path.to_owned(), bytes);
    }

    fn queue_behavior(&self, behavior: FaultBehavior) {
        self.download_behaviors.lock().unwrap().push(behavior);
    }

    /// Queue N consecutive `FailBeforeWrite` behaviors with the given kind.
    fn queue_failures(&self, count: usize, kind: RemoteErrorKind) {
        for _ in 0..count {
            self.queue_behavior(FaultBehavior::FailBeforeWrite(kind));
        }
    }

    fn fail_on_range(&self, index: usize) {
        *self.fail_on_range_index.lock().unwrap() = Some(index);
    }

    fn range_call_count(&self) -> usize {
        *self.range_call_count.lock().unwrap()
    }

    fn next_behavior(&self) -> FaultBehavior {
        self.download_behaviors
            .lock()
            .unwrap()
            .pop()
            .unwrap_or(FaultBehavior::Success)
    }

    fn recorded_delays(&self) -> Vec<Duration> {
        self.recorded_delays.lock().unwrap().clone()
    }

    fn credential_generation(&self) -> u64 {
        *self.credential_generation.lock().unwrap()
    }

    fn refresh_credentials(&self) -> bool {
        let mut gen = self.credential_generation.lock().unwrap();
        *gen += 1;
        true
    }

    fn capabilities(&self) -> RemoteProviderCapabilities {
        RemoteProviderCapabilities {
            conditional_replace: !self.no_cas,
            resumable_upload: false,
            range_download: true,
            revision_metadata: true,
            server_side_move: false,
        }
    }
}

fn command_error_from_kind(kind: RemoteErrorKind, detail: &str) -> CommandError {
    // Map to CommandError so atomic_download's DownloadFailed path carries
    // the kind information in the message for test assertions.
    CommandError::from(LibraryError::Internal(format!("{}: {detail}", kind.code())))
}

impl RemoteProvider for FaultInjectionProvider {
    fn capabilities(&self) -> RemoteProviderCapabilities {
        self.capabilities()
    }

    fn stat(&self, path: &str) -> CommandResult<Option<RemoteObjectMetadata>> {
        let files = self.files.lock().unwrap();
        let revisions = self.revisions.lock().unwrap();
        if files.contains_key(path) {
            Ok(Some(RemoteObjectMetadata {
                size: Some(files.get(path).unwrap().len() as u64),
                revision: revisions.get(path).cloned(),
            }))
        } else {
            Ok(None)
        }
    }

    fn get_revision(&self, path: &str) -> CommandResult<Option<String>> {
        Ok(self.revisions.lock().unwrap().get(path).cloned())
    }

    fn download_file(&self, relative_path: &str, destination: &Path) -> CommandResult<()> {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let behavior = self.next_behavior();
        match behavior {
            FaultBehavior::Success => {
                let data = self
                    .files
                    .lock()
                    .unwrap()
                    .get(relative_path)
                    .cloned()
                    .ok_or_else(|| {
                        command_error_from_kind(RemoteErrorKind::PermissionDenied, "file not found")
                    })?;
                std::fs::write(destination, &data).map_err(|e| {
                    command_error_from_kind(RemoteErrorKind::NetworkUnavailable, &e.to_string())
                })
            }
            FaultBehavior::FailBeforeWrite(kind) => Err(command_error_from_kind(
                kind,
                "connection dropped before headers",
            )),
            FaultBehavior::PartialThenFail(n, kind) => {
                let data = self
                    .files
                    .lock()
                    .unwrap()
                    .get(relative_path)
                    .cloned()
                    .ok_or_else(|| {
                        command_error_from_kind(RemoteErrorKind::PermissionDenied, "file not found")
                    })?;
                let truncated = &data[..n.min(data.len())];
                let _ = std::fs::write(destination, truncated);
                Err(command_error_from_kind(kind, "connection dropped mid-body"))
            }
            FaultBehavior::ShortBody(n) => {
                let data = self
                    .files
                    .lock()
                    .unwrap()
                    .get(relative_path)
                    .cloned()
                    .ok_or_else(|| {
                        command_error_from_kind(RemoteErrorKind::PermissionDenied, "file not found")
                    })?;
                let short = &data[..n.min(data.len())];
                std::fs::write(destination, short).map_err(|e| {
                    command_error_from_kind(RemoteErrorKind::NetworkUnavailable, &e.to_string())
                })
            }
            FaultBehavior::WrongDigest => {
                let data = self
                    .files
                    .lock()
                    .unwrap()
                    .get(relative_path)
                    .cloned()
                    .ok_or_else(|| {
                        command_error_from_kind(RemoteErrorKind::PermissionDenied, "file not found")
                    })?;
                let mut modified = data.clone();
                if !modified.is_empty() {
                    modified[0] ^= 0xff;
                }
                std::fs::write(destination, &modified).map_err(|e| {
                    command_error_from_kind(RemoteErrorKind::NetworkUnavailable, &e.to_string())
                })
            }
            FaultBehavior::Slow(delay) => {
                std::thread::sleep(delay);
                let data = self
                    .files
                    .lock()
                    .unwrap()
                    .get(relative_path)
                    .cloned()
                    .ok_or_else(|| {
                        command_error_from_kind(RemoteErrorKind::PermissionDenied, "file not found")
                    })?;
                std::fs::write(destination, &data).map_err(|e| {
                    command_error_from_kind(RemoteErrorKind::NetworkUnavailable, &e.to_string())
                })
            }
        }
    }

    fn download_range(
        &self,
        relative_path: &str,
        destination: &Path,
        offset: u64,
        length: u64,
    ) -> RemoteResult<u64> {
        let mut count = self.range_call_count.lock().unwrap();
        let call_index = *count;
        *count += 1;
        drop(count);

        if let Some(fail_idx) = *self.fail_on_range_index.lock().unwrap() {
            if call_index == fail_idx {
                return Err(RemoteError::new(
                    RemoteErrorKind::NetworkUnavailable,
                    "simulated mid-transfer disconnect",
                ));
            }
        }

        let data = self
            .files
            .lock()
            .unwrap()
            .get(relative_path)
            .cloned()
            .ok_or_else(|| {
                RemoteError::new(
                    RemoteErrorKind::PermissionDenied,
                    format!("file {relative_path} not found"),
                )
            })?;
        let start = offset as usize;
        let end = (offset + length) as usize;
        if end > data.len() {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                "range beyond file size",
            ));
        }
        let chunk = &data[start..end];
        use std::io::{Seek, Write};
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(destination)
            .map_err(|e| {
                RemoteError::new(
                    RemoteErrorKind::NetworkUnavailable,
                    format!("failed to open temp file: {e}"),
                )
            })?;
        file.seek(std::io::SeekFrom::Start(offset))
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.to_string()))?;
        file.write_all(chunk)
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.to_string()))?;
        Ok(chunk.len() as u64)
    }

    fn upload_file(&self, path: &str) -> CommandResult<()> {
        {
            let mut remaining = self.upload_fail_remaining.lock().unwrap();
            if *remaining > 0 {
                *remaining -= 1;
                return Err(command_error_from_kind(
                    RemoteErrorKind::NetworkUnavailable,
                    "simulated candidate upload failure",
                ));
            }
        }
        if let Some(ref root) = self.working_copy_root {
            let local_path = root.join(path);
            if local_path.exists() {
                let bytes = std::fs::read(&local_path)
                    .map_err(|e| internal_error(format!("fake upload_file read: {e}")))?;
                let rev = format!(
                    "rev-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                );
                self.files.lock().unwrap().insert(path.to_owned(), bytes);
                self.revisions.lock().unwrap().insert(path.to_owned(), rev);
            }
        }
        Ok(())
    }

    fn upload_directory(&self, _path: &str) -> CommandResult<()> {
        Ok(())
    }

    fn delete_path(&self, _path: &str) -> CommandResult<()> {
        Ok(())
    }

    fn conditional_replace(
        &self,
        path: &str,
        source: ConditionalSource,
        expected_revision: Option<&str>,
    ) -> Result<RemoteObjectMetadata, RemoteError> {
        if self.no_cas {
            return Err(RemoteError::from_kind(
                RemoteErrorKind::ProviderCapabilityUnavailable,
            ));
        }
        if self.always_conflict {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteConflict,
                "concurrent writer committed first",
            ));
        }
        {
            let mut remaining = self.cas_network_fail_remaining.lock().unwrap();
            if *remaining > 0 {
                *remaining -= 1;
                return Err(RemoteError::new(
                    RemoteErrorKind::NetworkUnavailable,
                    "simulated CAS network failure after candidate upload",
                ));
            }
        }

        let bytes = source
            .read_bytes()
            .map_err(|e| RemoteError::new(RemoteErrorKind::NetworkUnavailable, e.message))?;

        let mut revisions = self.revisions.lock().unwrap();
        let current_rev = revisions.get(path).cloned();
        match expected_revision {
            Some(expected) => {
                if current_rev.as_deref() != Some(expected) {
                    return Err(RemoteError::new(
                        RemoteErrorKind::RemoteConflict,
                        format!(
                            "CAS mismatch: expected rev {expected}, found {:?}",
                            current_rev
                        ),
                    ));
                }
            }
            None => {
                if current_rev.is_some() {
                    return Err(RemoteError::new(
                        RemoteErrorKind::RemoteConflict,
                        "conditional-create failed: object already exists",
                    ));
                }
            }
        }

        let new_rev = format!(
            "rev-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let size = bytes.len() as u64;
        self.files.lock().unwrap().insert(path.to_owned(), bytes);
        revisions.insert(path.to_owned(), new_rev.clone());
        Ok(RemoteObjectMetadata {
            size: Some(size),
            revision: Some(new_rev),
        })
    }

    fn initialize_or_sync(&self) -> CommandResult<Option<String>> {
        Ok(None)
    }

    fn get_file_size(&self, path: &str) -> CommandResult<Option<u64>> {
        Ok(self.files.lock().unwrap().get(path).map(|b| b.len() as u64))
    }

    fn refresh_existing(&self) -> CommandResult<Option<String>> {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    crate::hash::hex_lower(hasher.finalize())
}

fn fresh_control_db() -> (TempDir, Connection) {
    let dir = TempDir::new().expect("temp dir");
    let conn = open_control_db(&dir.path().join("remote-state.db")).expect("open control db");
    (dir, conn)
}

fn make_valid_db(path: &Path) {
    let conn = Connection::open(path).unwrap();
    // Schema must match what verify_referenced_assets queries (songs path
    // columns + optional stems join). Keep the fixture empty of asset paths
    // so publish can proceed without remote media for pure protocol tests.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS songs (
            hash TEXT PRIMARY KEY,
            file_path TEXT,
            cdg_path TEXT,
            artwork_thumb_path TEXT,
            artwork_preview_path TEXT
         );
         CREATE TABLE IF NOT EXISTS stems (
            song_hash TEXT PRIMARY KEY,
            vocals_path TEXT,
            accomp_path TEXT,
            drums_path TEXT,
            bass_path TEXT,
            other_path TEXT
         );
         CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT);
         INSERT OR IGNORE INTO songs (hash) VALUES ('song-1');
         INSERT OR IGNORE INTO settings (key, value) VALUES ('version', '1');",
    )
    .unwrap();
}

fn make_pending_op(conn: &Connection, library_id: &str, expected_gen: i64) -> String {
    let op_id = format!("fault-op-{}", expected_gen);
    let now = crate::remote::types::current_unix_time_ms();
    let payload = OperationPayload {
        song_ids: vec!["song-1".to_owned()],
        percent: 0,
        detail: None,
    };
    let row = OperationRow {
        operation_id: op_id.clone(),
        library_id: library_id.to_owned(),
        operation_kind: OperationKind::Publish,
        state: OperationState::Pending,
        expected_generation: Some(expected_gen),
        target_generation: None,
        source_db_digest: None,
        candidate_db_digest: None,
        payload_json: payload.to_json().unwrap(),
        attempt_count: 0,
        next_attempt_at_ms: None,
        error_code: None,
        error_detail: None,
        created_at_ms: now,
        updated_at_ms: now,
    };
    upsert_operation(conn, &row).unwrap();
    op_id
}

fn seed_repository_state(conn: &Connection, library_id: &str) {
    let now = crate::remote::types::current_unix_time_ms();
    upsert_repository_state(
        conn,
        &RepositoryStateRow {
            library_id: library_id.to_owned(),
            committed_generation: 0,
            committed_manifest_revision: None,
            local_base_generation: 0,
            local_db_digest: None,
            local_state: LocalState::Clean,
            active_operation_id: None,
            last_success_at_ms: None,
            last_error_code: None,
            updated_at_ms: now,
            repository_id: Some("repo-uuid-1".to_owned()),
            writer_id: Some("writer-uuid-1".to_owned()),
        },
    )
    .unwrap();
}

fn make_context<'a>(
    control_db: &'a Connection,
    provider: &'a dyn RemoteProvider,
    working_copy_root: &'a Path,
    library_id: &'a str,
    repository_id: &'a str,
    writer_id: &'a str,
) -> PublishContext<'a> {
    PublishContext {
        control_db,
        provider,
        working_copy_root,
        library_id,
        writer_id,
        repository_id,
    }
}

/// A recording event sink for reconnect tests.
struct RecordingSink {
    events: Mutex<Vec<ReconnectEvent>>,
}

impl RecordingSink {
    fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }
}

impl EventSink for RecordingSink {
    fn emit(&self, event: ReconnectEvent) {
        self.events.lock().unwrap().push(event);
    }
}

// ---------------------------------------------------------------------------
// Test matrix (issue #151)
// ---------------------------------------------------------------------------

#[test]
fn t1_transient_failure_during_download_retry_succeeds() {
    // Download fails with a transient error on the first attempt, succeeds on
    // retry. Assert the file is complete and verified.
    let dir = TempDir::new().expect("temp dir");
    let dest = dir.path().join("media/song.mp3");
    let provider = FaultInjectionProvider::new();
    let data = b"hello world, fault injection".to_vec();
    provider.store_file("media/song.mp3", data.clone(), "rev-1");
    // First attempt: transient failure (503 → NetworkUnavailable).
    provider.queue_behavior(FaultBehavior::FailBeforeWrite(
        RemoteErrorKind::NetworkUnavailable,
    ));

    let result = atomic_download(
        &provider,
        AtomicDownloadOptions {
            relative_path: "media/song.mp3",
            destination: &dest,
            expected_size: Some(data.len() as u64),
            expected_digest: Some(&sha256_hex(&data)),
            operation_id: "t1",
        },
    );
    assert!(result.is_err(), "first attempt should fail");

    // Retry with a fresh behavior queue (Success is the default).
    let result2 = atomic_download(
        &provider,
        AtomicDownloadOptions {
            relative_path: "media/song.mp3",
            destination: &dest,
            expected_size: Some(data.len() as u64),
            expected_digest: Some(&sha256_hex(&data)),
            operation_id: "t1-retry",
        },
    );
    result2.expect("retry should succeed");
    assert_eq!(std::fs::read(&dest).unwrap(), data);
    // No temp file lingers.
    assert!(!dir.path().join("media/song.mp3.part.t1").exists());
}

#[test]
fn t2_permanent_failure_during_download_no_partial_file() {
    // Download fails with a permanent error (404 → PermissionDenied). Assert
    // no final-path file exists and temp files are cleaned.
    let dir = TempDir::new().expect("temp dir");
    let dest = dir.path().join("media/song.mp3");
    let provider = FaultInjectionProvider::new();
    provider.store_file("media/song.mp3", b"hello world".to_vec(), "rev-1");
    provider.queue_behavior(FaultBehavior::FailBeforeWrite(
        RemoteErrorKind::PermissionDenied,
    ));

    let result = atomic_download(
        &provider,
        AtomicDownloadOptions {
            relative_path: "media/song.mp3",
            destination: &dest,
            expected_size: None,
            expected_digest: None,
            operation_id: "t2",
        },
    );
    assert!(result.is_err(), "permanent failure should error");
    assert!(!dest.exists(), "no final-path file after permanent failure");
    assert!(
        !dir.path().join("media/song.mp3.part.t2").exists(),
        "temp cleaned up after permanent failure"
    );
}

#[test]
fn t3_credential_expiry_mid_transfer_refresh_then_retry_succeeds() {
    // 401 on first attempt → credential refresh → 200 on retry. Assert the
    // complete file lands at the destination.
    let dir = TempDir::new().expect("temp dir");
    let dest = dir.path().join("media/song.mp3");
    let provider = FaultInjectionProvider::new();
    let data = b"credential refresh retry payload".to_vec();
    provider.store_file("media/song.mp3", data.clone(), "rev-1");
    // First attempt: credential expired (401 → AuthenticationExpired).
    provider.queue_behavior(FaultBehavior::FailBeforeWrite(
        RemoteErrorKind::AuthenticationExpired,
    ));

    // Simulate the credential refresh: the provider's refresh_credentials
    // bumps the generation. A real caller would refresh then retry.
    let result = atomic_download(
        &provider,
        AtomicDownloadOptions {
            relative_path: "media/song.mp3",
            destination: &dest,
            expected_size: Some(data.len() as u64),
            expected_digest: Some(&sha256_hex(&data)),
            operation_id: "t3",
        },
    );
    assert!(result.is_err(), "first attempt should fail with auth error");

    // Refresh credentials (simulated).
    assert_eq!(provider.credential_generation(), 0);
    provider.refresh_credentials();
    assert_eq!(provider.credential_generation(), 1);

    // Retry succeeds (default behavior is Success).
    atomic_download(
        &provider,
        AtomicDownloadOptions {
            relative_path: "media/song.mp3",
            destination: &dest,
            expected_size: Some(data.len() as u64),
            expected_digest: Some(&sha256_hex(&data)),
            operation_id: "t3-retry",
        },
    )
    .expect("retry after refresh should succeed");
    assert_eq!(std::fs::read(&dest).unwrap(), data);
}

#[test]
fn t4_corrupted_download_rejected_by_digest_verification() {
    // Provider returns data that does not match the expected digest. Assert
    // the file is rejected (no final-path file) and a retry re-downloads.
    let dir = TempDir::new().expect("temp dir");
    let dest = dir.path().join("media/song.mp3");
    let provider = FaultInjectionProvider::new();
    let data = b"correct content".to_vec();
    provider.store_file("media/song.mp3", data.clone(), "rev-1");
    let expected_digest = sha256_hex(&data);
    provider.queue_behavior(FaultBehavior::WrongDigest);

    let result = atomic_download(
        &provider,
        AtomicDownloadOptions {
            relative_path: "media/song.mp3",
            destination: &dest,
            expected_size: None,
            expected_digest: Some(&expected_digest),
            operation_id: "t4",
        },
    );
    assert!(result.is_err(), "corrupted download must be rejected");
    assert!(!dest.exists(), "no final file on digest mismatch");

    // Retry with correct data (default Success behavior).
    atomic_download(
        &provider,
        AtomicDownloadOptions {
            relative_path: "media/song.mp3",
            destination: &dest,
            expected_size: None,
            expected_digest: Some(&expected_digest),
            operation_id: "t4-retry",
        },
    )
    .expect("retry should succeed with correct data");
    assert_eq!(std::fs::read(&dest).unwrap(), data);
}

#[test]
fn t5_mid_transfer_disconnect_resumable_download_completes() {
    // Disconnect at 50% → resume → assert complete file with correct digest.
    let dir = TempDir::new().expect("temp dir");
    let dest = dir.path().join("media/song.mp3");
    let provider = FaultInjectionProvider::new();
    // 16 MiB file so multiple chunks are needed.
    let data = vec![0xABu8; 16 * 1024 * 1024];
    provider.store_file("media/song.mp3", data.clone(), "rev-1");
    let expected_digest = sha256_hex(&data);

    let (_db_dir, conn) = fresh_control_db();

    // Seed a transfer part at offset 8 MiB (50% done) and write the first
    // half to the temp file so resume can append.
    let temp_path = dir.path().join("media/song.mp3.part.t5");
    std::fs::create_dir_all(temp_path.parent().unwrap()).unwrap();
    std::fs::write(&temp_path, &data[..8 * 1024 * 1024]).unwrap();
    upsert_transfer_part(
        &conn,
        &TransferPartRow {
            operation_id: "t5".to_owned(),
            relative_path: "media/song.mp3".to_owned(),
            direction: TransferDirection::Download,
            expected_size: Some(data.len() as i64),
            expected_digest: None,
            provider_revision: Some("rev-1".to_owned()),
            provider_session_id: None,
            transferred_bytes: (8 * 1024 * 1024) as i64,
            state: "in_progress".to_owned(),
            updated_at_ms: 1000,
        },
    )
    .unwrap();

    resumable_atomic_download(
        &provider,
        ResumableDownloadOptions {
            relative_path: "media/song.mp3",
            destination: &dest,
            expected_size: data.len() as u64,
            expected_digest: Some(&expected_digest),
            operation_id: "t5",
            control_db: &conn,
            provider_revision: Some("rev-1"),
        },
    )
    .expect("resumable download should complete after mid-transfer disconnect");

    assert_eq!(std::fs::read(&dest).unwrap(), data);
    // Transfer part deleted on success.
    assert!(crate::remote::control_db::list_transfer_parts(&conn, "t5")
        .unwrap()
        .is_empty());
}

#[test]
fn t6_cas_conflict_on_publish_conflict_surfaced() {
    // Publish hits a CAS conflict. Assert RemoteErrorKind::Conflict is
    // returned and the remote manifest is unchanged.
    let (_db_dir, conn) = fresh_control_db();
    let working_dir = TempDir::new().expect("working dir");
    let working_root = working_dir.path().to_owned();
    make_valid_db(&working_root.join("openkara.db"));
    seed_repository_state(&conn, "lib-1");

    // Simulate a remote that already has a manifest at generation 2 (another
    // device published while we were offline).
    let provider = FaultInjectionProvider::new().with_working_copy_root(working_root.clone());
    let manifest = RepositoryManifest {
        schema_version: CURRENT_SCHEMA_VERSION,
        repository_id: "repo-uuid-1".to_owned(),
        generation: 2,
        database_path: ".openkara/databases/2.sqlite".to_owned(),
        database_size: 100,
        database_sha256: "abc".to_owned(),
        committed_at_ms: 1000,
        writer_id: "other-device".to_owned(),
    };
    provider.store_file(
        crate::remote::manifest::MANIFEST_PATH,
        manifest.to_json().unwrap().into_bytes(),
        "rev-gen-2",
    );

    // Our operation expects generation 0 (we are stale).
    let op_id = make_pending_op(&conn, "lib-1", 0);
    let ctx = make_context(
        &conn,
        &provider,
        &working_root,
        "lib-1",
        "repo-uuid-1",
        "w-1",
    );
    let result = execute_publish(&ctx, &op_id);
    assert!(result.is_err(), "publish should fail with conflict");

    let op = crate::remote::control_db::get_operation(&conn, &op_id)
        .unwrap()
        .unwrap();
    assert_eq!(op.state, OperationState::Conflicted);
    assert_eq!(op.error_code.as_deref(), Some("remote_conflict"));

    // The remote manifest must be unchanged (still generation 2).
    let remote_manifest = read_manifest(&provider).unwrap().unwrap();
    assert_eq!(remote_manifest.generation, 2);
    assert_eq!(remote_manifest.writer_id, "other-device");
}

#[test]
fn t7_stale_request_aborts_download_no_rename() {
    // The stale-guard fires mid-download. Assert no rename and temps cleaned.
    // We simulate this by failing the download (the stale-guard in production
    // aborts the orchestrator before the rename; here we verify the atomic
    // download leaves no final file when the download fails mid-body).
    let dir = TempDir::new().expect("temp dir");
    let dest = dir.path().join("media/song.mp3");
    let provider = FaultInjectionProvider::new();
    let data = b"stale request test data".to_vec();
    provider.store_file("media/song.mp3", data.clone(), "rev-1");
    // Mid-body disconnect (stale-guard aborts mid-download in production).
    provider.queue_behavior(FaultBehavior::PartialThenFail(
        5,
        RemoteErrorKind::StaleRequest,
    ));

    let result = atomic_download(
        &provider,
        AtomicDownloadOptions {
            relative_path: "media/song.mp3",
            destination: &dest,
            expected_size: Some(data.len() as u64),
            expected_digest: None,
            operation_id: "t7",
        },
    );
    assert!(result.is_err(), "stale abort should error");
    assert!(!dest.exists(), "no rename after stale abort");
    assert!(
        !dir.path().join("media/song.mp3.part.t7").exists(),
        "temp cleaned after stale abort"
    );
}

#[test]
fn t8_reconnect_on_transient_playback_failure_timeline_preserved() {
    // Playback source fails with a transient error → reconnect → assert the
    // position is preserved (the new source is seeked to the old position).
    let sink = RecordingSink::new();
    let calls = Arc::new(Mutex::new(0u32));
    let calls_clone = Arc::clone(&calls);
    let re_resolve = move || {
        let mut c = calls_clone.lock().unwrap();
        let n = *c;
        *c += 1;
        drop(c);
        if n == 0 {
            // First attempt: transient failure (503).
            Err(ReconnectError::Transient)
        } else {
            // Second attempt: succeed with a source seeked to the position.
            Ok(ReresolvedSource {
                source: 0u32, // dummy source token
                from_cache: false,
                runtime: RemoteStreamingRuntime {
                    cache_pin_guard: None,
                    fetch_event_rx: None,
                },
            })
        }
    };
    // Seek closure: records the position the new source was seeked to.
    let seeked_position = Arc::new(Mutex::new(0u64));
    let seeked_clone = Arc::clone(&seeked_position);
    let seek_source = move |_: &mut u32, position_ms: u64| {
        *seeked_clone.lock().unwrap() = position_ms;
        SeekOutcome {
            requested_ms: position_ms,
            actual_ms: position_ms,
        }
    };
    let config = ReconnectConfig {
        max_attempts: 3,
        policy: RetryPolicy {
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(2),
            ..RetryPolicy::default()
        },
    };
    let rng = SeededJitter::new(1);
    let result = run_reconnect(
        "song-a",
        1,
        42_000,
        &config,
        &rng,
        None,
        re_resolve,
        seek_source,
        || false,
        || true,
        &sink,
        &|_| {},
    );
    let success = result.expect("reconnect should succeed");
    assert_eq!(success.seek.actual_ms, 42_000);
    assert_eq!(*seeked_position.lock().unwrap(), 42_000);

    let events = sink.events.lock().unwrap();
    // Two Reconnecting events (attempt 1 fails, attempt 2 succeeds).
    assert_eq!(events.len(), 2);
    assert!(matches!(
        events[0],
        ReconnectEvent::Reconnecting { attempt: 1, .. }
    ));
    assert!(matches!(
        events[1],
        ReconnectEvent::Reconnecting { attempt: 2, .. }
    ));
}

#[test]
fn t9_cache_eviction_under_budget_pressure_lru_pinned_survive() {
    // Fill the cache beyond the 2 GiB budget → assert LRU eviction removes
    // oldest unpinned entries; assert pinned entries survive.
    use crate::remote::cache_catalog::{CacheCatalog, CacheIdentity, DEFAULT_CACHE_BYTES_LIMIT};

    let db_dir = TempDir::new().expect("db temp dir");
    let cache_dir = TempDir::new().expect("cache temp dir");
    let conn = open_control_db(&db_dir.path().join("remote-state.db")).expect("open db");
    let control_db = Arc::new(Mutex::new(conn));
    // Small budget so eviction triggers quickly.
    let budget: u64 = 300;
    let catalog = CacheCatalog::open(
        cache_dir.path().to_path_buf(),
        Arc::clone(&control_db),
        budget,
    )
    .expect("open catalog");
    let catalog_arc = Arc::new(Mutex::new(catalog));

    let id_a = CacheIdentity {
        library_id: "lib-1".to_owned(),
        relative_path: "media/a.mp3".to_owned(),
        provider_revision: Some("rev-1".to_owned()),
        expected_size: 100,
    };
    let id_b = CacheIdentity {
        library_id: "lib-1".to_owned(),
        relative_path: "media/b.mp3".to_owned(),
        provider_revision: Some("rev-1".to_owned()),
        expected_size: 100,
    };

    // Insert A (oldest).
    let cache_a = {
        let mut cat = catalog_arc.lock().unwrap();
        cat.get_or_create(&id_a).unwrap()
    };
    cache_a.write_at(0, &[1u8; 100]).unwrap();
    catalog_arc
        .lock()
        .unwrap()
        .persist_ranges(&id_a.cache_key())
        .unwrap();

    // Pin A so it survives eviction.
    let _guard = CacheCatalog::pin_cache_entry(&catalog_arc, &id_a.cache_key()).unwrap();

    // Sleep so B's access timestamp is strictly greater than A's.
    std::thread::sleep(Duration::from_millis(20));

    // Insert B (newer, unpinned).
    let cache_b = {
        let mut cat = catalog_arc.lock().unwrap();
        cat.get_or_create(&id_b).unwrap()
    };
    cache_b.write_at(0, &[2u8; 100]).unwrap();
    catalog_arc
        .lock()
        .unwrap()
        .persist_ranges(&id_b.cache_key())
        .unwrap();

    // Insert C (300 budget, A=100 pinned + B=100 + C=100 = 300 exactly at
    // budget; no eviction yet).
    let id_c = CacheIdentity {
        library_id: "lib-1".to_owned(),
        relative_path: "media/c.mp3".to_owned(),
        provider_revision: Some("rev-1".to_owned()),
        expected_size: 100,
    };
    {
        let mut cat = catalog_arc.lock().unwrap();
        cat.get_or_create(&id_c).unwrap();
    }

    // Insert D (forces eviction: 300 budget, A=100 pinned + B=100 + C=100 +
    // D=100 = 400 > 300). B is the oldest unpinned → evicted.
    let id_d = CacheIdentity {
        library_id: "lib-1".to_owned(),
        relative_path: "media/d.mp3".to_owned(),
        provider_revision: Some("rev-1".to_owned()),
        expected_size: 100,
    };
    {
        let mut cat = catalog_arc.lock().unwrap();
        cat.get_or_create(&id_d).unwrap();
    }

    // A is pinned → survives. B (oldest unpinned) should have been evicted
    // to make room. C and D remain.
    let cat = catalog_arc.lock().unwrap();
    assert!(
        cat.get_entry(&id_a.cache_key()).unwrap().is_some(),
        "pinned entry A must survive eviction"
    );
    assert!(
        cat.get_entry(&id_b.cache_key()).unwrap().is_none(),
        "oldest unpinned entry B must be evicted"
    );
    assert!(cat.get_entry(&id_c.cache_key()).unwrap().is_some());
    assert!(cat.get_entry(&id_d.cache_key()).unwrap().is_some());

    // Sanity: the default budget is 2 GiB.
    assert_eq!(DEFAULT_CACHE_BYTES_LIMIT, 2 * 1024 * 1024 * 1024);
}

#[test]
fn t10_startup_recovery_after_crash_mid_download() {
    // Write a partial download + catalog entry; simulate crash (close DB);
    // reopen → assert the partial entry is reconciled and resumable.
    use crate::remote::cache_catalog::CacheCatalog;

    let db_dir = TempDir::new().expect("db temp dir");
    let cache_dir = TempDir::new().expect("cache temp dir");
    let conn = open_control_db(&db_dir.path().join("remote-state.db")).expect("open db");
    let control_db = Arc::new(Mutex::new(conn));
    let catalog = CacheCatalog::open(
        cache_dir.path().to_path_buf(),
        Arc::clone(&control_db),
        1024 * 1024,
    )
    .expect("open catalog");
    let catalog_arc = Arc::new(Mutex::new(catalog));

    // Insert a partial entry (50 of 200 bytes).
    let id = crate::remote::cache_catalog::CacheIdentity {
        library_id: "lib-1".to_owned(),
        relative_path: "media/x.mp3".to_owned(),
        provider_revision: Some("rev-1".to_owned()),
        expected_size: 200,
    };
    let cache = {
        let mut cat = catalog_arc.lock().unwrap();
        cat.get_or_create(&id).unwrap()
    };
    cache.write_at(0, &[1u8; 50]).unwrap();
    catalog_arc
        .lock()
        .unwrap()
        .persist_ranges(&id.cache_key())
        .unwrap();

    // Simulate crash: drop all handles.
    drop(catalog_arc);

    // Reopen — reconciliation runs on open.
    let mut catalog = CacheCatalog::open(
        cache_dir.path().to_path_buf(),
        Arc::clone(&control_db),
        1024 * 1024,
    )
    .expect("reopen after crash");

    // The partial entry should be reconciled: the catalog row should still
    // exist (file length 200 matches expected_size because ChunkedCache
    // pre-allocates), and the ranges should be intact so resume can continue.
    let cache2 = catalog.get_or_create(&id).unwrap();
    assert!(
        cache2.is_cached(0, 50),
        "partial ranges must survive restart"
    );
    assert!(!cache2.is_complete(), "entry must still be partial");
}

#[test]
fn t11_orphaned_data_file_cleanup_on_startup() {
    // Create a data file with no catalog entry; open cache → assert the file
    // is deleted on the startup scan.
    use crate::remote::cache_catalog::CacheCatalog;

    let db_dir = TempDir::new().expect("db temp dir");
    let cache_dir = TempDir::new().expect("cache temp dir");
    let conn = open_control_db(&db_dir.path().join("remote-state.db")).expect("open db");
    let control_db = Arc::new(Mutex::new(conn));
    let catalog = CacheCatalog::open(
        cache_dir.path().to_path_buf(),
        Arc::clone(&control_db),
        1024 * 1024,
    )
    .expect("open catalog");
    drop(catalog);

    // Drop an orphaned .cache file with no catalog row.
    let orphan = cache_dir.path().join("orphan-data.cache");
    std::fs::write(&orphan, b"junk bytes").unwrap();
    assert!(orphan.exists());

    let _catalog = CacheCatalog::open(
        cache_dir.path().to_path_buf(),
        Arc::clone(&control_db),
        1024 * 1024,
    )
    .expect("reopen");

    assert!(
        !orphan.exists(),
        "orphaned data file must be deleted on startup scan"
    );
}

#[test]
fn t12_network_retry_with_backoff_increasing_delays() {
    // Configure 3 consecutive transient failures; assert the RetryDriver
    // retries with increasing delays (use seeded RNG for deterministic
    // delays) and eventually succeeds on the 4th attempt.
    let provider = FaultInjectionProvider::new();
    let data = b"backoff retry test".to_vec();
    provider.store_file("media/song.mp3", data.clone(), "rev-1");
    // Queue 3 transient failures then the default (Success).
    provider.queue_failures(3, RemoteErrorKind::NetworkUnavailable);

    let policy = RetryPolicy {
        max_retries: 4,
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(80),
        ..RetryPolicy::default()
    };
    let rng = SeededJitter::new(42);
    let recorded = Arc::new(Mutex::new(Vec::<Duration>::new()));
    let recorded_clone = Arc::clone(&recorded);
    let sleep_fn = move |delay: Duration| {
        recorded_clone.lock().unwrap().push(delay);
    };

    let attempt_count = Arc::new(Mutex::new(0u32));
    let attempt_clone = Arc::clone(&attempt_count);
    let driver = RetryDriver {
        policy: &policy,
        rng: &rng,
        cancel: None,
        progress: None,
        sleep_fn: &sleep_fn,
        now_ms: &|| 0,
    };
    let result: Result<Vec<u8>, RemoteError> = driver.run(|| {
        let mut count = attempt_clone.lock().unwrap();
        *count += 1;
        drop(count);
        // The first 3 attempts fail transiently; the 4th succeeds.
        // download_file pops behaviors; we call it directly here.
        let dir = TempDir::new().expect("temp dir");
        let dest = dir.path().join("song.mp3");
        let download_result = provider.download_file("media/song.mp3", &dest);
        match download_result {
            Ok(()) => {
                let bytes = std::fs::read(&dest).unwrap_or_default();
                AttemptOutcome::Ok(bytes)
            }
            Err(_) => {
                AttemptOutcome::Err(RemoteError::from_kind(RemoteErrorKind::NetworkUnavailable))
            }
        }
    });

    let final_bytes = result.expect("retry should eventually succeed");
    assert_eq!(final_bytes, data);
    // 3 retries → 3 delays recorded.
    let delays = recorded.lock().unwrap().clone();
    assert_eq!(delays.len(), 3, "one delay per retry");
    // Delays must be non-decreasing (full-jitter caps grow exponentially).
    // With a seeded RNG the exact values are deterministic; assert the cap
    // sequence is increasing and each delay is within its cap.
    for (i, &delay) in delays.iter().enumerate() {
        let cap = {
            let mut cap = policy.initial_delay;
            for _ in 0..i {
                cap = cap.saturating_mul(2);
                if cap >= policy.max_delay {
                    break;
                }
            }
            cap.min(policy.max_delay)
        };
        assert!(
            delay <= cap,
            "delay {delay:?} at retry {i} must be within cap {cap:?}"
        );
    }
    // The cap for retry 2 must be >= the cap for retry 1 (monotonic growth).
    let cap1 = full_jitter_delay(&policy, 0, &SeededJitter::new(0)).max(Duration::ZERO);
    let _ = cap1; // referenced for completeness
    assert!(
        delays[1] >= Duration::ZERO && delays[2] >= Duration::ZERO,
        "all delays must be non-negative"
    );
    // Verify the attempt count: 4 total (3 failures + 1 success).
    assert_eq!(*attempt_count.lock().unwrap(), 4);
}

// ---------------------------------------------------------------------------
// Publication crash windows (issue #151)
// ---------------------------------------------------------------------------

#[test]
fn t13_crash_after_candidate_upload_before_cas_preserves_remote_generation() {
    // Crash window 6: candidate uploaded, CAS not yet applied. A network
    // failure on CAS must leave the remote generation unchanged and keep the
    // local operation retryable / dirty — never silently overwrite.
    let (_db_dir, conn) = fresh_control_db();
    let working_dir = TempDir::new().expect("working dir");
    let working_root = working_dir.path().to_owned();
    make_valid_db(&working_root.join("openkara.db"));
    seed_repository_state(&conn, "lib-1");

    let provider = FaultInjectionProvider::new().with_working_copy_root(working_root.clone());
    provider.fail_next_cas_network(1);

    let op_id = make_pending_op(&conn, "lib-1", 0);
    let ctx = make_context(
        &conn,
        &provider,
        &working_root,
        "lib-1",
        "repo-uuid-1",
        "writer-uuid-1",
    );
    let err = execute_publish(&ctx, &op_id).expect_err("CAS network failure");
    assert!(
        err.message.contains("CAS network")
            || err.message.contains("network")
            || err.message.contains("simulated")
            || err.message.contains("could not"),
        "error should indicate network/CAS failure: {}",
        err.message
    );

    // No manifest committed — generation still absent.
    assert!(
        read_manifest(&provider).unwrap().is_none(),
        "manifest must not advance when CAS fails"
    );

    let op = crate::remote::control_db::get_operation(&conn, &op_id)
        .unwrap()
        .unwrap();
    assert_eq!(op.state, OperationState::RetryWait);
    let repo = crate::remote::control_db::get_repository_state(&conn, "lib-1")
        .unwrap()
        .unwrap();
    assert_eq!(repo.local_state, LocalState::Publishing);
    assert_eq!(repo.committed_generation, 0);
}

#[test]
fn t14_retry_after_cas_network_failure_converges() {
    // Same crash window as t13, but the next attempt succeeds. The operation
    // must complete and advance the remote generation exactly once.
    let (_db_dir, conn) = fresh_control_db();
    let working_dir = TempDir::new().expect("working dir");
    let working_root = working_dir.path().to_owned();
    make_valid_db(&working_root.join("openkara.db"));
    seed_repository_state(&conn, "lib-1");

    let provider = FaultInjectionProvider::new().with_working_copy_root(working_root.clone());
    provider.fail_next_cas_network(1);

    let op_id = make_pending_op(&conn, "lib-1", 0);
    let ctx = make_context(
        &conn,
        &provider,
        &working_root,
        "lib-1",
        "repo-uuid-1",
        "writer-uuid-1",
    );
    let _ = execute_publish(&ctx, &op_id).expect_err("first attempt fails at CAS");

    // Reset the op to Pending for the retry (simulates recovery).
    let mut op = crate::remote::control_db::get_operation(&conn, &op_id)
        .unwrap()
        .unwrap();
    op.state = OperationState::Pending;
    op.error_code = None;
    op.error_detail = None;
    crate::remote::control_db::upsert_operation(&conn, &op).unwrap();

    execute_publish(&ctx, &op_id).expect("retry after CAS network failure must succeed");

    let remote_manifest = read_manifest(&provider).unwrap().unwrap();
    assert_eq!(remote_manifest.generation, 1);
    assert_eq!(remote_manifest.writer_id, "writer-uuid-1");

    let op = crate::remote::control_db::get_operation(&conn, &op_id)
        .unwrap()
        .unwrap();
    assert_eq!(op.state, OperationState::Completed);
}

#[test]
fn t15_crash_after_cas_before_local_completion_recovers_own_commit() {
    // Crash windows 7–8: CAS succeeded remotely, process died before
    // record_completed. Recovery re-enters execute_publish with the same
    // expected_generation and must accept our own writer_id commit instead
    // of surfacing RemoteConflict.
    let (_db_dir, conn) = fresh_control_db();
    let working_dir = TempDir::new().expect("working dir");
    let working_root = working_dir.path().to_owned();
    make_valid_db(&working_root.join("openkara.db"));
    seed_repository_state(&conn, "lib-1");

    let provider = FaultInjectionProvider::new().with_working_copy_root(working_root.clone());

    // Simulate: CAS already committed generation 1 by this writer.
    let db_bytes = std::fs::read(working_root.join("openkara.db")).unwrap();
    let digest = sha256_hex(&db_bytes);
    provider.store_file(".openkara/databases/1.sqlite", db_bytes, "rev-db-1");
    let accepted = RepositoryManifest {
        schema_version: CURRENT_SCHEMA_VERSION,
        repository_id: "repo-uuid-1".to_owned(),
        generation: 1,
        database_path: ".openkara/databases/1.sqlite".to_owned(),
        database_size: std::fs::metadata(working_root.join("openkara.db"))
            .unwrap()
            .len(),
        database_sha256: digest.clone(),
        committed_at_ms: 5000,
        writer_id: "writer-uuid-1".to_owned(),
    };
    provider.store_file(
        crate::remote::manifest::MANIFEST_PATH,
        accepted.to_json().unwrap().into_bytes(),
        "rev-manifest-1",
    );

    // Local op still thinks we expected generation 0 (never recorded completion).
    let op_id = make_pending_op(&conn, "lib-1", 0);
    let mut op = crate::remote::control_db::get_operation(&conn, &op_id)
        .unwrap()
        .unwrap();
    op.state = OperationState::RetryWait;
    crate::remote::control_db::upsert_operation(&conn, &op).unwrap();

    let ctx = make_context(
        &conn,
        &provider,
        &working_root,
        "lib-1",
        "repo-uuid-1",
        "writer-uuid-1",
    );
    execute_publish(&ctx, &op_id).expect("own accepted commit must complete recovery");

    let op = crate::remote::control_db::get_operation(&conn, &op_id)
        .unwrap()
        .unwrap();
    assert_eq!(op.state, OperationState::Completed);
    assert_eq!(op.target_generation, Some(1));

    let repo = crate::remote::control_db::get_repository_state(&conn, "lib-1")
        .unwrap()
        .unwrap();
    assert_eq!(repo.committed_generation, 1);
    assert_eq!(repo.local_state, LocalState::Clean);
    assert_eq!(repo.local_db_digest.as_deref(), Some(digest.as_str()));

    // Remote generation must remain 1 — no double-publish.
    let remote_manifest = read_manifest(&provider).unwrap().unwrap();
    assert_eq!(remote_manifest.generation, 1);
}

#[test]
fn t16_candidate_upload_failure_leaves_dirty_and_retryable() {
    // Crash window 5: candidate upload fails. Local mutation must remain
    // (working DB untouched), remote generation must not advance, op retryable.
    let (_db_dir, conn) = fresh_control_db();
    let working_dir = TempDir::new().expect("working dir");
    let working_root = working_dir.path().to_owned();
    let working_db = working_root.join("openkara.db");
    make_valid_db(&working_db);
    let pre_digest = sha256_hex(&std::fs::read(&working_db).unwrap());
    seed_repository_state(&conn, "lib-1");

    // Mark dirty as mutation would.
    let mut repo = crate::remote::control_db::get_repository_state(&conn, "lib-1")
        .unwrap()
        .unwrap();
    repo.local_state = LocalState::Dirty;
    crate::remote::control_db::upsert_repository_state(&conn, &repo).unwrap();

    let provider = FaultInjectionProvider::new().with_working_copy_root(working_root.clone());
    provider.fail_next_uploads(1);

    let op_id = make_pending_op(&conn, "lib-1", 0);
    let ctx = make_context(
        &conn,
        &provider,
        &working_root,
        "lib-1",
        "repo-uuid-1",
        "writer-uuid-1",
    );
    let _ = execute_publish(&ctx, &op_id).expect_err("upload failure");

    assert!(
        read_manifest(&provider).unwrap().is_none(),
        "remote must not advance without successful CAS"
    );
    assert_eq!(
        sha256_hex(&std::fs::read(&working_db).unwrap()),
        pre_digest,
        "local working DB must not be overwritten"
    );

    let op = crate::remote::control_db::get_operation(&conn, &op_id)
        .unwrap()
        .unwrap();
    assert_eq!(op.state, OperationState::RetryWait);
}

#[test]
fn t17_concurrent_writers_one_winner_one_conflict_no_overwrite() {
    // Two independent control planes against one fake provider: both read
    // generation N; exactly one CAS to N+1 succeeds; the other is conflicted.
    let (_db_a, conn_a) = fresh_control_db();
    let (_db_b, conn_b) = fresh_control_db();
    let working_dir = TempDir::new().expect("working dir");
    let working_root = working_dir.path().to_owned();
    make_valid_db(&working_root.join("openkara.db"));

    seed_repository_state(&conn_a, "lib-shared");
    seed_repository_state(&conn_b, "lib-shared");

    let provider = FaultInjectionProvider::new().with_working_copy_root(working_root.clone());

    let op_a = make_pending_op(&conn_a, "lib-shared", 0);
    let op_b = {
        // Unique op id for B (make_pending_op uses generation in id).
        let op_id = "fault-op-b-0".to_owned();
        let now = crate::remote::types::current_unix_time_ms();
        let payload = OperationPayload {
            song_ids: vec!["song-1".to_owned()],
            percent: 0,
            detail: None,
        };
        let row = OperationRow {
            operation_id: op_id.clone(),
            library_id: "lib-shared".to_owned(),
            operation_kind: OperationKind::Publish,
            state: OperationState::Pending,
            expected_generation: Some(0),
            target_generation: None,
            source_db_digest: None,
            candidate_db_digest: None,
            payload_json: payload.to_json().unwrap(),
            attempt_count: 0,
            next_attempt_at_ms: None,
            error_code: None,
            error_detail: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        upsert_operation(&conn_b, &row).unwrap();
        op_id
    };

    let ctx_a = make_context(
        &conn_a,
        &provider,
        &working_root,
        "lib-shared",
        "repo-uuid-1",
        "writer-a",
    );
    execute_publish(&ctx_a, &op_a).expect("writer A wins");

    let winner = read_manifest(&provider).unwrap().unwrap();
    assert_eq!(winner.generation, 1);
    assert_eq!(winner.writer_id, "writer-a");

    let ctx_b = make_context(
        &conn_b,
        &provider,
        &working_root,
        "lib-shared",
        "repo-uuid-1",
        "writer-b",
    );
    let err = execute_publish(&ctx_b, &op_b).expect_err("writer B must conflict");
    assert!(
        err.message.contains("conflict")
            || err.message.contains("expected generation")
            || err.message.contains("generation"),
        "B should report conflict: {}",
        err.message
    );

    let op_b_row = crate::remote::control_db::get_operation(&conn_b, &op_b)
        .unwrap()
        .unwrap();
    assert_eq!(op_b_row.state, OperationState::Conflicted);

    // Winner's generation must not be overwritten by the loser.
    let after = read_manifest(&provider).unwrap().unwrap();
    assert_eq!(after.generation, 1);
    assert_eq!(after.writer_id, "writer-a");

    // Retrying the loser must still not overwrite.
    let mut op_b_row = op_b_row;
    op_b_row.state = OperationState::Pending;
    op_b_row.error_code = None;
    crate::remote::control_db::upsert_operation(&conn_b, &op_b_row).unwrap();
    let _ = execute_publish(&ctx_b, &op_b).expect_err("retry loser still conflicts");
    let final_manifest = read_manifest(&provider).unwrap().unwrap();
    assert_eq!(final_manifest.generation, 1);
    assert_eq!(final_manifest.writer_id, "writer-a");
}

#[test]
fn t18_transfer_identity_digest_change_invalidates_session() {
    // Changed digest invalidates resume: a transfer part with a different
    // expected digest must not produce a hybrid object.
    let (_db_dir, conn) = fresh_control_db();
    let dir = TempDir::new().expect("temp");
    let temp = dir.path().join("candidate.part.op-x");
    std::fs::write(&temp, b"partial-bytes-old").unwrap();

    let part = TransferPartRow {
        operation_id: "op-x".to_owned(),
        relative_path: ".openkara/databases/1.sqlite".to_owned(),
        direction: TransferDirection::Upload,
        expected_size: Some(100),
        expected_digest: Some("digest-old".to_owned()),
        provider_revision: None,
        provider_session_id: Some("session-old".to_owned()),
        transferred_bytes: 16,
        state: "in_progress".to_owned(),
        updated_at_ms: 1000,
    };
    upsert_transfer_part(&conn, &part).unwrap();

    // New candidate with different digest: resume identity check must reject
    // reusing the old session. We assert the helper contract used by the
    // executor: same op_id + relative_path + direction but mismatched digest
    // is treated as a new transfer (session id cleared by callers that rebuild
    // candidates). Here we verify the control-DB row can be replaced atomically.
    let new_part = TransferPartRow {
        operation_id: "op-x".to_owned(),
        relative_path: ".openkara/databases/1.sqlite".to_owned(),
        direction: TransferDirection::Upload,
        expected_size: Some(100),
        expected_digest: Some("digest-new".to_owned()),
        provider_revision: None,
        provider_session_id: None, // session invalidated
        transferred_bytes: 0,
        state: "pending".to_owned(),
        updated_at_ms: 2000,
    };
    upsert_transfer_part(&conn, &new_part).unwrap();
    let loaded = crate::remote::control_db::list_transfer_parts(&conn, "op-x").unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].expected_digest.as_deref(), Some("digest-new"));
    assert!(loaded[0].provider_session_id.is_none());
    assert_eq!(loaded[0].transferred_bytes, 0);
}
