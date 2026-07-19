use crate::audio::chunked_cache::CacheManager;
use crate::remote::{RemoteAuthSession, UploadStatusSnapshot};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct RemoteState {
    pub remote_auth_sessions: Arc<Mutex<HashMap<String, RemoteAuthSession>>>,
    pub remote_upload_statuses: Arc<Mutex<HashMap<String, UploadStatusSnapshot>>>,
    /// LRU-managed on-disk caches for remote streaming playback.
    pub remote_chunk_cache: Arc<Mutex<CacheManager>>,
}

impl RemoteState {
    pub fn new_with_limit(app_data_dir: &Path, remote_cache_bytes_limit: Option<u64>) -> Self {
        let cache_dir = app_data_dir.join("remote-cache");

        // When absent in config, allow the cache to grow unbounded (u64::MAX).
        // Eviction logic uses saturating arithmetic so this is safe.
        let max_bytes = remote_cache_bytes_limit.unwrap_or(u64::MAX);

        Self {
            remote_auth_sessions: Arc::new(Mutex::new(HashMap::new())),
            remote_upload_statuses: Arc::new(Mutex::new(HashMap::new())),
            remote_chunk_cache: Arc::new(Mutex::new(CacheManager::new(cache_dir, max_bytes))),
        }
    }

    pub fn new(app_data_dir: &Path) -> Self {
        Self::new_with_limit(app_data_dir, None)
    }

    pub fn test_fixture() -> Self {
        Self::new(Path::new("/tmp/openkara-test-remote-cache"))
    }
}
