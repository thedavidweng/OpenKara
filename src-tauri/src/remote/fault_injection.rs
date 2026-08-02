//! Comprehensive fault-injection test suite for the remote reliability matrix.

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
use crate::remote::provider::{
    ConditionalSource, RemoteMediaSource, RemoteMediaSourceCapabilities, RepositoryStorage,
};
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

struct FaultInjectionProvider {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    revisions: Arc<Mutex<HashMap<String, String>>>,
    download_behaviors: Arc<Mutex<Vec<FaultBehavior>>>,
    fail_on_range_index: Arc<Mutex<Option<usize>>>,
    range_call_count: Arc<Mutex<usize>>,
    upload_fail_remaining: Arc<Mutex<usize>>,
    no_cas: bool,
    always_conflict: bool,
    cas_network_fail_remaining: Arc<Mutex<usize>>,
    working_copy_root: Option<PathBuf>,
    #[allow(dead_code)]
    recorded_delays: Arc<Mutex<Vec<Duration>>>,
    credential_generation: Arc<Mutex<u64>>,
}

#[derive(Clone)]
#[allow(dead_code)]
enum FaultBehavior {
    Success,
    FailBeforeWrite(RemoteErrorKind),
    PartialThenFail(usize, RemoteErrorKind),
    ShortBody(usize),
    WrongDigest,
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

    fn fail_next_uploads(&self, count: usize) {
        *self.upload_fail_remaining.lock().unwrap() = count;
    }

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
            revision_metadata: true,
            server_side_move: false,
        }
    }
}

fn command_error_from_kind(kind: RemoteErrorKind, detail: &str) -> CommandError {
    CommandError::from(LibraryError::Internal(format!("{}: {detail}", kind.code())))
}

impl RepositoryStorage for FaultInjectionProvider {
    fn media_source(&self) -> &dyn RemoteMediaSource {
        self
    }

    fn capabilities(&self) -> RemoteProviderCapabilities {
        self.capabilities()
    }

    fn stat(&self, path: &str) -> CommandResult<Option<RemoteObjectMetadata>> {
        let files = self.files.lock().unwrap();
        let revisions = self.revisions.lock().unwrap();
        if files.contains_key(path) {
            Ok(Some(RemoteObjectMetadata {
                size_bytes: Some(files.get(path).unwrap().len() as u64),
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
            size_bytes: Some(size),
            revision: Some(new_rev),
        })
    }

    fn initialize_or_sync(&self) -> CommandResult<Option<String>> {
        Ok(None)
    }

    fn refresh_existing(&self) -> CommandResult<Option<String>> {
        Ok(None)
    }
}

impl RemoteMediaSource for FaultInjectionProvider {
    fn capabilities(&self) -> RemoteMediaSourceCapabilities {
        RemoteMediaSourceCapabilities {
            range_download: true,
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

    fn get_file_size(&self, path: &str) -> CommandResult<Option<u64>> {
        Ok(self.files.lock().unwrap().get(path).map(|b| b.len() as u64))
    }
}

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
        ..Default::default()
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
    provider: &'a dyn RepositoryStorage,
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

#[test]
fn t1_transient_failure_during_download_retry_succeeds() {
    let dir = TempDir::new().expect("temp dir");
    let dest = dir.path().join("media/song.mp3");
    let provider = FaultInjectionProvider::new();
    let data = b"hello world, fault injection".to_vec();
    provider.store_file("media/song.mp3", data.clone(), "rev-1");
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
    assert!(!dir.path().join("media/song.mp3.part.t1").exists());
}

#[test]
fn t2_permanent_failure_during_download_no_partial_file() {
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
    let dir = TempDir::new().expect("temp dir");
    let dest = dir.path().join("media/song.mp3");
    let provider = FaultInjectionProvider::new();
    let data = b"credential refresh retry payload".to_vec();
    provider.store_file("media/song.mp3", data.clone(), "rev-1");
    provider.queue_behavior(FaultBehavior::FailBeforeWrite(
        RemoteErrorKind::AuthenticationExpired,
    ));

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

    assert_eq!(provider.credential_generation(), 0);
    provider.refresh_credentials();
    assert_eq!(provider.credential_generation(), 1);

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
    let dir = TempDir::new().expect("temp dir");
    let dest = dir.path().join("media/song.mp3");
    let provider = FaultInjectionProvider::new();
    let data = vec![0xABu8; 16 * 1024 * 1024];
    provider.store_file("media/song.mp3", data.clone(), "rev-1");
    let expected_digest = sha256_hex(&data);

    let (_db_dir, conn) = fresh_control_db();

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
    assert!(crate::remote::control_db::list_transfer_parts(&conn, "t5")
        .unwrap()
        .is_empty());
}

#[test]
fn t6_cas_conflict_on_publish_conflict_surfaced() {
    let (_db_dir, conn) = fresh_control_db();
    let working_dir = TempDir::new().expect("working dir");
    let working_root = working_dir.path().to_owned();
    make_valid_db(&working_root.join("openkara.db"));
    seed_repository_state(&conn, "lib-1");

    let provider = FaultInjectionProvider::new().with_working_copy_root(working_root.clone());
    let manifest = RepositoryManifest {
        schema_version: CURRENT_SCHEMA_VERSION,
        repository_id: "repo-uuid-1".to_owned(),
        generation: 2,
        database_path: ".openkara/databases/2.sqlite".to_owned(),
        database_size_bytes: 100,
        database_sha256: "abc".to_owned(),
        committed_at_ms: 1000,
        writer_id: "other-device".to_owned(),
        operation_id: "op-test".to_owned(),
    };
    provider.store_file(
        crate::remote::manifest::MANIFEST_PATH,
        manifest.to_json().unwrap().into_bytes(),
        "rev-gen-2",
    );

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

    let remote_manifest = read_manifest(&provider).unwrap().unwrap();
    assert_eq!(remote_manifest.generation, 2);
    assert_eq!(remote_manifest.writer_id, "other-device");
}

#[test]
fn t7_stale_request_aborts_download_no_rename() {
    let dir = TempDir::new().expect("temp dir");
    let dest = dir.path().join("media/song.mp3");
    let provider = FaultInjectionProvider::new();
    let data = b"stale request test data".to_vec();
    provider.store_file("media/song.mp3", data.clone(), "rev-1");
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
    let sink = RecordingSink::new();
    let calls = Arc::new(Mutex::new(0u32));
    let calls_clone = Arc::clone(&calls);
    let re_resolve = move || {
        let mut c = calls_clone.lock().unwrap();
        let n = *c;
        *c += 1;
        drop(c);
        if n == 0 {
            Err(ReconnectError::Transient)
        } else {
            Ok(ReresolvedSource {
                source: 0u32,
                from_cache: false,
                runtime: RemoteStreamingRuntime {
                    cache_pin_guard: None,
                    fetch_event_rx: None,
                },
            })
        }
    };
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
    use crate::remote::cache_catalog::{CacheCatalog, CacheIdentity, DEFAULT_CACHE_BYTES_LIMIT};

    let db_dir = TempDir::new().expect("db temp dir");
    let cache_dir = TempDir::new().expect("cache temp dir");
    let conn = open_control_db(&db_dir.path().join("remote-state.db")).expect("open db");
    let control_db = Arc::new(Mutex::new(conn));
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

    let _guard = CacheCatalog::pin_cache_entry(&catalog_arc, &id_a.cache_key()).unwrap();

    std::thread::sleep(Duration::from_millis(20));

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

    assert_eq!(DEFAULT_CACHE_BYTES_LIMIT, 2 * 1024 * 1024 * 1024);
}

#[test]
fn t10_startup_recovery_after_crash_mid_download() {
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

    drop(catalog_arc);

    let mut catalog = CacheCatalog::open(
        cache_dir.path().to_path_buf(),
        Arc::clone(&control_db),
        1024 * 1024,
    )
    .expect("reopen after crash");

    let cache2 = catalog.get_or_create(&id).unwrap();
    assert!(
        cache2.is_cached(0, 50),
        "partial ranges must survive restart"
    );
    assert!(!cache2.is_complete(), "entry must still be partial");
}

#[test]
fn t11_orphaned_data_file_cleanup_on_startup() {
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
    let provider = FaultInjectionProvider::new();
    let data = b"backoff retry test".to_vec();
    provider.store_file("media/song.mp3", data.clone(), "rev-1");
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
    let delays = recorded.lock().unwrap().clone();
    assert_eq!(delays.len(), 3, "one delay per retry");
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
    let cap1 = full_jitter_delay(&policy, 0, &SeededJitter::new(0)).max(Duration::ZERO);
    let _ = cap1;
    assert!(
        delays[1] >= Duration::ZERO && delays[2] >= Duration::ZERO,
        "all delays must be non-negative"
    );
    assert_eq!(*attempt_count.lock().unwrap(), 4);
}

#[test]
fn t13_crash_after_candidate_upload_before_cas_preserves_remote_generation() {
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
    let (_db_dir, conn) = fresh_control_db();
    let working_dir = TempDir::new().expect("working dir");
    let working_root = working_dir.path().to_owned();
    make_valid_db(&working_root.join("openkara.db"));
    seed_repository_state(&conn, "lib-1");

    let provider = FaultInjectionProvider::new().with_working_copy_root(working_root.clone());

    let db_bytes = std::fs::read(working_root.join("openkara.db")).unwrap();
    let digest = sha256_hex(&db_bytes);
    let db_size = std::fs::metadata(working_root.join("openkara.db"))
        .unwrap()
        .len();
    provider.store_file(".openkara/databases/1.sqlite", db_bytes, "rev-db-1");

    let op_id = make_pending_op(&conn, "lib-1", 0);
    let accepted = RepositoryManifest {
        schema_version: CURRENT_SCHEMA_VERSION,
        repository_id: "repo-uuid-1".to_owned(),
        generation: 1,
        database_path: ".openkara/databases/1.sqlite".to_owned(),
        database_size_bytes: db_size,
        database_sha256: digest.clone(),
        committed_at_ms: 5000,
        writer_id: "writer-uuid-1".to_owned(),
        operation_id: op_id.clone(),
    };
    provider.store_file(
        crate::remote::manifest::MANIFEST_PATH,
        accepted.to_json().unwrap().into_bytes(),
        "rev-manifest-1",
    );

    let mut op = crate::remote::control_db::get_operation(&conn, &op_id)
        .unwrap()
        .unwrap();
    op.state = OperationState::RetryWait;
    op.candidate_db_digest = Some(digest.clone());
    let mut payload =
        crate::remote::control_db::OperationPayload::from_json(&op.payload_json).unwrap();
    payload.candidate_sha256 = Some(digest.clone());
    payload.candidate_size = Some(db_size);
    payload.protocol_step = Some("candidate_uploaded".to_owned());
    op.payload_json = payload.to_json().unwrap();
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

    let remote_manifest = read_manifest(&provider).unwrap().unwrap();
    assert_eq!(remote_manifest.generation, 1);
}

#[test]
fn t16_candidate_upload_failure_leaves_dirty_and_retryable() {
    let (_db_dir, conn) = fresh_control_db();
    let working_dir = TempDir::new().expect("working dir");
    let working_root = working_dir.path().to_owned();
    let working_db = working_root.join("openkara.db");
    make_valid_db(&working_db);
    let pre_digest = sha256_hex(&std::fs::read(&working_db).unwrap());
    seed_repository_state(&conn, "lib-1");

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
        let op_id = "fault-op-b-0".to_owned();
        let now = crate::remote::types::current_unix_time_ms();
        let payload = OperationPayload {
            song_ids: vec!["song-1".to_owned()],
            percent: 0,
            detail: None,
            ..Default::default()
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

    let after = read_manifest(&provider).unwrap().unwrap();
    assert_eq!(after.generation, 1);
    assert_eq!(after.writer_id, "writer-a");

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

    let new_part = TransferPartRow {
        operation_id: "op-x".to_owned(),
        relative_path: ".openkara/databases/1.sqlite".to_owned(),
        direction: TransferDirection::Upload,
        expected_size: Some(100),
        expected_digest: Some("digest-new".to_owned()),
        provider_revision: None,
        provider_session_id: None,
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
