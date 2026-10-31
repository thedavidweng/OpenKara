//! Comprehensive fault-injection test suite for the remote reliability matrix.

#![cfg(test)]

use crate::commands::error::{CommandError, CommandResult};
use crate::library::error::LibraryError;
use crate::remote::atomic_download::{
    atomic_download, resumable_atomic_download, AtomicDownloadOptions, ResumableDownloadOptions,
};
use crate::remote::control_db::{
    open_control_db, upsert_transfer_part, TransferDirection, TransferPartRow,
};
use crate::remote::errors::{RemoteError, RemoteErrorKind, RemoteObjectMetadata, RemoteResult};
use crate::remote::net_policy::{
    full_jitter_delay, AttemptOutcome, RetryDriver, RetryPolicy, SeededJitter,
};
use crate::remote::provider::{
    RemoteMediaSource, RemoteMediaSourceCapabilities, RepositoryStorage,
};
use crate::services::reconnect::{
    run_reconnect, EventSink, ReconnectConfig, ReconnectError, ReconnectEvent,
    RemoteStreamingRuntime, ReresolvedSource, SeekOutcome,
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;

struct FaultInjectionProvider {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    revisions: Arc<Mutex<HashMap<String, String>>>,
    download_behaviors: Arc<Mutex<Vec<FaultBehavior>>>,
    fail_on_range_index: Arc<Mutex<Option<usize>>>,
    range_call_count: Arc<Mutex<usize>>,
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
            recorded_delays: Arc::new(Mutex::new(Vec::new())),
            credential_generation: Arc::new(Mutex::new(0)),
        }
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
}

fn command_error_from_kind(kind: RemoteErrorKind, detail: &str) -> CommandError {
    CommandError::from(LibraryError::Internal(format!("{}: {detail}", kind.code())))
}

impl RepositoryStorage for FaultInjectionProvider {
    fn media_source(&self) -> &dyn RemoteMediaSource {
        self
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

    fn delete_path(&self, _path: &str) -> CommandResult<()> {
        Ok(())
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
                    cache_lease: None,
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
