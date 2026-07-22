use crate::remote::cache_catalog::{CacheCatalog, DEFAULT_CACHE_BYTES_LIMIT};
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
    /// Persistent, verified cache catalog for remote streaming playback. The
    /// `remote_cache_entries` table in the control DB is the authoritative
    /// catalog; on-disk data files are content-addressed by the cache key
    /// digest. Replaces the old in-memory-only `CacheManager`.
    pub remote_chunk_cache: Arc<Mutex<CacheCatalog>>,
    /// Durable control-plane database handle (`remote-state.db`). Holds the
    /// authoritative local record of remote operation/outbox state, repository
    /// cleanliness, and resumable transfer offsets. Never uploaded.
    pub control_db: Arc<Mutex<rusqlite::Connection>>,
    /// `true` when the durable control DB could not be opened and the app
    /// is running on an in-memory fallback. Publication, resumable recovery,
    /// and automatic pull must fail closed when this is `true`.
    pub control_db_degraded: bool,
    /// Per-library commit locks. See `CommitLockMap`.
    pub commit_locks: CommitLockMap,
}

impl RemoteState {
    pub fn new_with_limit(app_data_dir: &Path, remote_cache_bytes_limit: Option<u64>) -> Self {
        let cache_dir = app_data_dir.join("remote-cache");

        // Default to a finite 2 GiB budget when no limit is configured. The
        // old default of u64::MAX (unbounded) let the cache grow without bound
        // on a fresh install. A finite default prevents disk exhaustion while
        // staying large enough that a typical session does not thrash.
        let max_bytes = remote_cache_bytes_limit.unwrap_or(DEFAULT_CACHE_BYTES_LIMIT);

        // Open the durable control DB. This stays outside every portable
        // library and is never uploaded. WAL mode is enabled on open so
        // concurrent readers (upload-status queries) do not block the writer.
        //
        // If the control DB cannot be opened, fall back to an in-memory
        // connection so the app can still start in a degraded read-only
        // mode. The in-memory fallback supports local library use and
        // cached playback, but must NOT be used for durable publication,
        // resumable recovery, or clean-state guarantees — those require a
        // writable control DB. The `control_db_degraded` flag below
        // distinguishes this state so callers can fail closed for
        // operations that need durable state.
        let control_db_path = control_db::control_db_path(app_data_dir);
        let (control_db_conn, control_db_degraded) =
            match control_db::open_control_db(&control_db_path) {
                Ok(conn) => (conn, false),
                Err(error) => {
                    eprintln!(
                        "warning: failed to open remote control DB at {}: {:?}; \
                         starting in degraded read-only mode — publication and \
                         resumable recovery are disabled until the control DB \
                         is repaired",
                        control_db_path.display(),
                        error
                    );
                    let conn = rusqlite::Connection::open_in_memory()
                        .expect("in-memory control DB fallback should always open");
                    control_db::apply_migrations(&conn)
                        .expect("in-memory control DB migrations should always succeed");
                    (conn, true)
                }
            };

        let control_db = Arc::new(Mutex::new(control_db_conn));

        // Open the persistent cache catalog. This runs startup reconciliation
        // (orphaned files removed, inconsistent rows discarded) before any
        // playback uses the cache.
        let remote_chunk_cache = CacheCatalog::open(cache_dir, Arc::clone(&control_db), max_bytes)
            .unwrap_or_else(|error| {
                eprintln!(
                    "warning: failed to open remote cache catalog: {:?}; \
                 falling back to an empty in-memory-only catalog",
                    error
                );
                // Fall back to an empty catalog backed by a temp dir so the app
                // can still start in degraded mode (cache will not persist).
                let fallback_dir = std::env::temp_dir().join("openkara-remote-cache-fallback");
                CacheCatalog::open(fallback_dir, Arc::clone(&control_db), max_bytes)
                    .expect("fallback cache catalog should always open")
            });

        Self {
            remote_auth_sessions: Arc::new(Mutex::new(HashMap::new())),
            remote_upload_statuses: Arc::new(Mutex::new(HashMap::new())),
            remote_chunk_cache: Arc::new(Mutex::new(remote_chunk_cache)),
            control_db,
            control_db_degraded,
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
