use crate::commands::error::{remote_repository_unavailable, CommandResult};
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
    remote_chunk_cache: Option<Arc<Mutex<CacheCatalog>>>,
    /// Durable control-plane database handle (`remote-state.db`). Holds the
    /// authoritative local record of remote operation/outbox state, repository
    /// cleanliness, and resumable transfer offsets. Never uploaded.
    control_db_handle: Option<Arc<Mutex<rusqlite::Connection>>>,
    unavailable_reason: Option<String>,
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
        // library and is never uploaded.
        let control_db_path = control_db::control_db_path(app_data_dir);
        let (control_db_handle, mut unavailable_reason) =
            match control_db::open_control_db(&control_db_path) {
                Ok(conn) => (Some(Arc::new(Mutex::new(conn))), None),
                Err(error) => (
                    None,
                    Some(format!("failed to open remote control database: {error:?}")),
                ),
            };

        // Open the persistent cache catalog. This runs startup reconciliation
        // (orphaned files removed, inconsistent rows discarded) before any
        // playback uses the cache.
        let remote_chunk_cache = control_db_handle.as_ref().and_then(|control_db| {
            match CacheCatalog::open(cache_dir, Arc::clone(control_db), max_bytes) {
                Ok(catalog) => Some(Arc::new(Mutex::new(catalog))),
                Err(error) => {
                    unavailable_reason =
                        Some(format!("failed to open remote cache catalog: {error:?}"));
                    None
                }
            }
        });

        Self {
            remote_auth_sessions: Arc::new(Mutex::new(HashMap::new())),
            remote_upload_statuses: Arc::new(Mutex::new(HashMap::new())),
            remote_chunk_cache,
            control_db_handle,
            unavailable_reason,
            commit_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn new(app_data_dir: &Path) -> Self {
        Self::new_with_limit(app_data_dir, None)
    }

    pub fn test_fixture() -> Self {
        let path =
            std::env::temp_dir().join(format!("openkara-test-remote-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("remote test directory should be writable");
        let state = Self::new(&path);
        assert!(state.is_available(), "remote test state must be available");
        state
    }

    pub fn is_available(&self) -> bool {
        self.control_db_handle.is_some() && self.remote_chunk_cache.is_some()
    }

    pub fn ensure_available(&self) -> CommandResult<()> {
        if self.is_available() {
            return Ok(());
        }
        Err(remote_repository_unavailable(
            self.unavailable_reason
                .as_deref()
                .unwrap_or("remote repository state is unavailable"),
        ))
    }

    pub fn control_db(&self) -> CommandResult<&Arc<Mutex<rusqlite::Connection>>> {
        self.control_db_handle.as_ref().ok_or_else(|| {
            remote_repository_unavailable(
                self.unavailable_reason
                    .as_deref()
                    .unwrap_or("remote control database is unavailable"),
            )
        })
    }

    pub fn remote_chunk_cache(&self) -> CommandResult<&Arc<Mutex<CacheCatalog>>> {
        self.remote_chunk_cache.as_ref().ok_or_else(|| {
            remote_repository_unavailable(
                self.unavailable_reason
                    .as_deref()
                    .unwrap_or("remote cache catalog is unavailable"),
            )
        })
    }

    #[cfg(test)]
    pub fn replace_control_db(&mut self, connection: rusqlite::Connection) {
        self.control_db_handle = Some(Arc::new(Mutex::new(connection)));
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
