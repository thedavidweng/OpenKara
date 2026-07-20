use crate::audio::chunked_cache::CacheManager;
use crate::remote::control_db;
use crate::remote::{RemoteAuthSession, UploadStatusSnapshot};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Per-library commit serialization locks.
///
/// Two concurrent commit attempts for the same library block each other;
/// independent asset downloads for different libraries proceed in parallel.
/// PR#5 will use this to coordinate resumable transfers.
pub type CommitLockMap = Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>;

#[derive(Clone)]
pub struct RemoteState {
    pub remote_auth_sessions: Arc<Mutex<HashMap<String, RemoteAuthSession>>>,
    /// In-memory projection of upload statuses for event delivery. The durable
    /// source of truth is the `remote_operations` table in the control DB; this
    /// map is kept only so we avoid re-emitting events for unchanged state.
    pub remote_upload_statuses: Arc<Mutex<HashMap<String, UploadStatusSnapshot>>>,
    /// LRU-managed on-disk caches for remote streaming playback.
    pub remote_chunk_cache: Arc<Mutex<CacheManager>>,
    /// Durable control-plane database handle (`remote-state.db`). Holds the
    /// authoritative local record of remote operation/outbox state, repository
    /// cleanliness, and resumable transfer offsets. Never uploaded.
    pub control_db: Arc<Mutex<rusqlite::Connection>>,
    /// Per-library commit locks. See `CommitLockMap`.
    pub commit_locks: CommitLockMap,
}

impl RemoteState {
    pub fn new_with_limit(app_data_dir: &Path, remote_cache_bytes_limit: Option<u64>) -> Self {
        let cache_dir = app_data_dir.join("remote-cache");

        // When absent in config, allow the cache to grow unbounded (u64::MAX).
        // Eviction logic uses saturating arithmetic so this is safe.
        let max_bytes = remote_cache_bytes_limit.unwrap_or(u64::MAX);

        // Open the durable control DB. This stays outside every portable
        // library and is never uploaded. WAL mode is enabled on open so
        // concurrent readers (upload-status queries) do not block the writer.
        let control_db_path = control_db::control_db_path(app_data_dir);
        let control_db_conn =
            control_db::open_control_db(&control_db_path).unwrap_or_else(|error| {
                eprintln!(
                    "warning: failed to open remote control DB at {}: {:?}",
                    control_db_path.display(),
                    error
                );
                // Fall back to an in-memory connection so the app can still
                // start; operation state will not persist across restarts in
                // this degraded mode. Apply migrations so the recovery and
                // upload-status paths can query remote_operations etc.
                // without hitting "no such table" errors.
                let conn = rusqlite::Connection::open_in_memory()
                    .expect("in-memory control DB fallback should always open");
                control_db::apply_migrations(&conn)
                    .expect("in-memory control DB migrations should always succeed");
                conn
            });

        Self {
            remote_auth_sessions: Arc::new(Mutex::new(HashMap::new())),
            remote_upload_statuses: Arc::new(Mutex::new(HashMap::new())),
            remote_chunk_cache: Arc::new(Mutex::new(CacheManager::new(cache_dir, max_bytes))),
            control_db: Arc::new(Mutex::new(control_db_conn)),
            commit_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn new(app_data_dir: &Path) -> Self {
        Self::new_with_limit(app_data_dir, None)
    }

    pub fn test_fixture() -> Self {
        Self::new(Path::new("/tmp/openkara-test-remote-cache"))
    }

    /// Resolve the per-library commit lock, creating it on first use. The
    /// caller should call `.lock()` on the returned `Arc` to serialize
    /// concurrent commit attempts for the same library. Different libraries
    /// proceed concurrently.
    ///
    /// The lock is kept in the `commit_locks` map for the lifetime of
    /// `RemoteState`, so the `Arc` is not leaked — it is also held by the map.
    pub fn commit_lock(&self, library_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self
            .commit_locks
            .lock()
            .expect("commit lock map was poisoned");
        locks
            .entry(library_id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}
