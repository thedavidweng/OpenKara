use super::range_set::RangeSet;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const READ_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum CacheError {
    Io(io::Error),
    CorruptedIndex(String),
    Timeout,
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::Io(e) => write!(f, "cache I/O error: {e}"),
            CacheError::CorruptedIndex(msg) => write!(f, "corrupted cache index: {msg}"),
            CacheError::Timeout => write!(f, "cache read timed out waiting for data"),
        }
    }
}

impl From<io::Error> for CacheError {
    fn from(e: io::Error) -> Self {
        CacheError::Io(e)
    }
}

/// Inner state of the chunked cache, protected by a mutex.
struct CacheInner {
    file: File,
    downloaded: RangeSet,
    file_size: u64,
    last_access: Instant,
}

/// Supports concurrent read (from the decode/symphonia thread) and write
/// (from the fetch thread) via a mutex + condvar pattern.
pub struct ChunkedCache {
    path: PathBuf,
    inner: Mutex<CacheInner>,
    data_available: Condvar,
}

fn acquire_lock(mutex: &Mutex<CacheInner>) -> std::sync::MutexGuard<'_, CacheInner> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl ChunkedCache {
    pub fn open(cache_dir: &Path, cache_key: &str, file_size: u64) -> Result<Self, CacheError> {
        let data_path = cache_dir.join(format!("{cache_key}.cache"));
        let index_path = cache_dir.join(format!("{cache_key}.index"));

        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&data_path)?;

        // Pre-allocate the file to the expected size so that partial downloads
        // (where only some ranges have been written) still report the correct
        // file length. Without this, a file with ranges [0,50) and [100,50)
        // would be 150 bytes, not `file_size`, and the persistent catalog's
        // startup reconciliation would discard it as a size mismatch.
        if file.metadata()?.len() < file_size {
            file.set_len(file_size)?;
        }

        let downloaded = if index_path.exists() {
            let json = fs::read_to_string(&index_path)?;
            serde_json::from_str(&json).unwrap_or_else(|_| RangeSet::new())
        } else {
            RangeSet::new()
        };

        Ok(Self {
            path: data_path,
            inner: Mutex::new(CacheInner {
                file,
                downloaded,
                file_size,
                last_access: Instant::now(),
            }),
            data_available: Condvar::new(),
        })
    }

    /// Open a cache file and initialize the downloaded range set from the
    /// persistent catalog instead of the `.index` sidecar. Used by the
    /// persistent cache catalog (PR#6) so ranges survive restart even when
    /// the `.index` sidecar was deleted on completion.
    pub fn open_with_ranges(
        cache_dir: &Path,
        cache_key: &str,
        file_size: u64,
        ranges: RangeSet,
    ) -> Result<Self, CacheError> {
        let data_path = cache_dir.join(format!("{cache_key}.cache"));

        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&data_path)?;

        // Pre-allocate to `file_size` so partial entries report the correct
        // length during startup reconciliation.
        if file.metadata()?.len() < file_size {
            file.set_len(file_size)?;
        }

        Ok(Self {
            path: data_path,
            inner: Mutex::new(CacheInner {
                file,
                downloaded: ranges,
                file_size,
                last_access: Instant::now(),
            }),
            data_available: Condvar::new(),
        })
    }

    pub fn is_cached(&self, offset: u64, length: u64) -> bool {
        let inner = acquire_lock(&self.inner);
        inner.downloaded.contains(offset, length)
    }

    pub fn cached_length_from(&self, offset: u64, max_length: u64) -> u64 {
        let inner = acquire_lock(&self.inner);
        inner.downloaded.contained_length_from(offset, max_length)
    }

    pub fn downloaded(&self) -> RangeSet {
        let inner = acquire_lock(&self.inner);
        inner.downloaded.clone()
    }

    pub fn file_size(&self) -> u64 {
        let inner = acquire_lock(&self.inner);
        inner.file_size
    }

    pub fn is_complete(&self) -> bool {
        let inner = acquire_lock(&self.inner);
        inner.downloaded.covers_full(inner.file_size)
    }

    pub fn last_access(&self) -> Instant {
        let inner = acquire_lock(&self.inner);
        inner.last_access
    }

    /// May include gaps filled with zeros due to `set_len` extension.
    pub fn data_bytes(&self) -> u64 {
        let inner = acquire_lock(&self.inner);
        inner.file.metadata().map(|m| m.len()).unwrap_or(0)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Extends the file if the write goes past the current end (required for
    /// non-contiguous downloads on macOS, which does not support sparse file holes).
    pub fn write_at(&self, offset: u64, data: &[u8]) -> Result<(), CacheError> {
        let mut inner = acquire_lock(&self.inner);
        let write_end = offset + data.len() as u64;
        let current_len = inner.file.metadata()?.len();
        if write_end > current_len {
            inner.file.set_len(write_end)?;
        }
        inner.file.seek(SeekFrom::Start(offset))?;
        inner.file.write_all(data)?;
        inner.downloaded.add_range(offset, data.len() as u64);
        inner.last_access = Instant::now();
        self.data_available.notify_all();
        Ok(())
    }

    /// Blocks (via condvar wait) if the range is not yet cached. Returns the
    /// number of bytes actually read.
    ///
    /// Uses `wait_timeout` instead of infinite `wait` so that a dead fetch
    /// thread does not hang the decode thread forever.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, CacheError> {
        self.read_at_with_timeout(offset, buf, READ_TIMEOUT)
    }

    /// Exposed for testing timeout behavior without waiting 30 seconds.
    fn read_at_with_timeout(
        &self,
        offset: u64,
        buf: &mut [u8],
        timeout: Duration,
    ) -> Result<usize, CacheError> {
        let mut inner = acquire_lock(&self.inner);
        let length = buf.len() as u64;

        while !inner.downloaded.contains(offset, length) {
            let available = inner.downloaded.contained_length_from(offset, length);
            if available > 0 {
                let to_read = available as usize;
                inner.file.seek(SeekFrom::Start(offset))?;
                inner.file.read_exact(&mut buf[..to_read])?;
                inner.last_access = Instant::now();
                return Ok(to_read);
            }
            let (guard, timeout_result) = self
                .data_available
                .wait_timeout(inner, timeout)
                .map_err(|_| CacheError::Io(io::Error::other("lock poisoned")))?;
            inner = guard;
            if timeout_result.timed_out() {
                return Err(CacheError::Timeout);
            }
        }

        inner.file.seek(SeekFrom::Start(offset))?;
        inner.file.read_exact(buf)?;
        inner.last_access = Instant::now();
        Ok(buf.len())
    }

    pub fn save_index(&self) -> Result<(), CacheError> {
        let inner = acquire_lock(&self.inner);
        if inner.downloaded.covers_full(inner.file_size) {
            // Complete file — no need to track partial state.
            let index_path = self.index_path();
            if index_path.exists() {
                fs::remove_file(&index_path)?;
            }
            return Ok(());
        }

        let json = serde_json::to_string(&inner.downloaded)
            .map_err(|e| CacheError::Io(io::Error::other(e)))?;
        fs::write(self.index_path(), json)?;
        Ok(())
    }

    fn index_path(&self) -> PathBuf {
        self.path.with_extension("index")
    }
}

/// Tracks total disk usage across all managed caches. When the total exceeds
/// `max_bytes`, the least-recently-used cache is evicted (its data file and
/// index are deleted).
pub struct CacheManager {
    cache_dir: PathBuf,
    caches: HashMap<String, Arc<ChunkedCache>>,
    max_bytes: u64,
}

impl CacheManager {
    pub fn new(cache_dir: PathBuf, max_bytes: u64) -> Self {
        Self {
            cache_dir,
            caches: HashMap::new(),
            max_bytes,
        }
    }

    pub const DEFAULT_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

    /// Evicts LRU caches if needed.
    pub fn get_or_create(
        &mut self,
        key: &str,
        file_size: u64,
    ) -> Result<Arc<ChunkedCache>, CacheError> {
        if let Some(cache) = self.caches.get(key) {
            return Ok(Arc::clone(cache));
        }

        self.evict_if_needed(file_size)?;

        let cache = Arc::new(ChunkedCache::open(&self.cache_dir, key, file_size)?);
        self.caches.insert(key.to_string(), Arc::clone(&cache));
        Ok(cache)
    }

    pub fn total_bytes(&self) -> u64 {
        self.caches.values().map(|c| c.data_bytes()).sum()
    }

    pub fn len(&self) -> usize {
        self.caches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.caches.is_empty()
    }

    fn evict_if_needed(&mut self, needed_bytes: u64) -> Result<(), CacheError> {
        let current: u64 = self.total_bytes();
        if current.saturating_add(needed_bytes) <= self.max_bytes {
            return Ok(());
        }

        let mut entries: Vec<(&String, &Arc<ChunkedCache>)> = self.caches.iter().collect();
        entries.sort_by_key(|(_, c)| c.last_access());

        let mut freed = current;
        let mut to_remove = Vec::new();

        for (key, cache) in &entries {
            if freed.saturating_add(needed_bytes) <= self.max_bytes {
                break;
            }
            let cache_bytes = cache.data_bytes();
            cache.save_index().ok();
            let _ = fs::remove_file(cache.path());
            let index_path = cache.path().with_extension("index");
            let _ = fs::remove_file(&index_path);
            freed -= cache_bytes;
            to_remove.push(key.to_string());
        }

        for key in to_remove {
            self.caches.remove(&key);
        }

        Ok(())
    }

    pub fn remove(&mut self, key: &str) -> Result<(), CacheError> {
        if let Some(cache) = self.caches.remove(key) {
            cache.save_index().ok();
            let _ = fs::remove_file(cache.path());
            let index_path = cache.path().with_extension("index");
            let _ = fs::remove_file(&index_path);
        }
        Ok(())
    }

    pub fn save_all_indices(&self) -> Result<(), CacheError> {
        for cache in self.caches.values() {
            cache.save_index()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temp_dir(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "chunked_cache_test_{}_{suffix}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn open_and_write_data() {
        let dir = temp_dir("open_write");
        let cache = ChunkedCache::open(&dir, "test1", 200).unwrap();

        assert!(!cache.is_cached(0, 100));
        assert_eq!(cache.file_size(), 200);
        assert!(!cache.is_complete());

        cache.write_at(0, &[42u8; 100]).unwrap();
        assert!(cache.is_cached(0, 100));
        assert!(!cache.is_cached(0, 200));
        assert_eq!(cache.cached_length_from(0, 200), 100);

        cleanup(&dir);
    }

    #[test]
    fn read_cached_data() {
        let dir = temp_dir("read_cached");
        let cache = ChunkedCache::open(&dir, "test2", 100).unwrap();

        let data = (0..100u8).collect::<Vec<_>>();
        cache.write_at(0, &data).unwrap();

        let mut buf = vec![0u8; 100];
        let read = cache.read_at(0, &mut buf).unwrap();
        assert_eq!(read, 100);
        assert_eq!(buf, data);

        cleanup(&dir);
    }

    #[test]
    fn read_blocks_until_data_available() {
        let dir = temp_dir("read_blocks");
        let cache = Arc::new(ChunkedCache::open(&dir, "test3", 100).unwrap());

        let cache_clone = Arc::clone(&cache);
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            cache_clone.write_at(0, &[99u8; 50]).unwrap();
        });

        let mut buf = vec![0u8; 50];
        let read = cache.read_at(0, &mut buf).unwrap();
        assert_eq!(read, 50);
        assert!(buf.iter().all(|&b| b == 99));

        writer.join().unwrap();
        cleanup(&dir);
    }

    #[test]
    fn save_and_load_index() {
        let dir = temp_dir("save_load");
        let cache = ChunkedCache::open(&dir, "test4", 200).unwrap();
        cache.write_at(0, &[1u8; 50]).unwrap();
        cache.write_at(100, &[2u8; 50]).unwrap();
        cache.save_index().unwrap();

        let cache2 = ChunkedCache::open(&dir, "test4", 200).unwrap();
        assert!(cache2.is_cached(0, 50));
        assert!(cache2.is_cached(100, 50));
        assert!(!cache2.is_cached(0, 200));

        cleanup(&dir);
    }

    #[test]
    fn complete_file_removes_index() {
        let dir = temp_dir("complete");
        let cache = ChunkedCache::open(&dir, "test5", 100).unwrap();
        cache.write_at(0, &[0u8; 100]).unwrap();
        cache.save_index().unwrap();

        let index_path = dir.join("test5.index");
        assert!(!index_path.exists());
        assert!(cache.is_complete());

        cleanup(&dir);
    }

    #[test]
    fn partial_read_returns_available_bytes() {
        let dir = temp_dir("partial_read");
        let cache = Arc::new(ChunkedCache::open(&dir, "test6", 200).unwrap());

        cache.write_at(0, &[7u8; 50]).unwrap();

        let cache_clone = Arc::clone(&cache);
        let reader = std::thread::spawn(move || {
            let mut buf = vec![0u8; 100];
            let read = cache_clone.read_at(0, &mut buf).unwrap();
            (read, buf)
        });

        let (read, buf) = reader.join().unwrap();
        assert_eq!(read, 50);
        assert!(buf[..50].iter().all(|&b| b == 7));

        cleanup(&dir);
    }

    #[test]
    fn partial_read_stops_at_uncached_gap() {
        let dir = temp_dir("partial_gap");
        let cache = ChunkedCache::open(&dir, "test_gap", 200).unwrap();

        cache.write_at(0, &[1u8; 30]).unwrap();
        cache.write_at(50, &[2u8; 30]).unwrap();

        let mut buf = vec![0u8; 80];
        let read = cache.read_at(0, &mut buf).unwrap();

        assert_eq!(read, 30);
        assert!(buf[..30].iter().all(|&b| b == 1));

        cleanup(&dir);
    }

    #[test]
    fn cache_manager_create_and_get() {
        let dir = temp_dir("mgr_create");
        let mut mgr = CacheManager::new(dir.clone(), CacheManager::DEFAULT_MAX_BYTES);

        let cache = mgr.get_or_create("file1", 1000).unwrap();
        assert_eq!(cache.file_size(), 1000);
        assert_eq!(mgr.len(), 1);

        let cache2 = mgr.get_or_create("file1", 1000).unwrap();
        assert!(Arc::ptr_eq(&cache, &cache2));
        assert_eq!(mgr.len(), 1);

        cleanup(&dir);
    }

    #[test]
    fn cache_manager_evicts_lru() {
        let dir = temp_dir("mgr_evict");
        // Enough for one small cache but not two.
        let mut mgr = CacheManager::new(dir.clone(), 200);

        let cache1 = mgr.get_or_create("small1", 100).unwrap();
        cache1.write_at(0, &[1u8; 50]).unwrap();

        let cache2 = mgr.get_or_create("small2", 100).unwrap();
        cache2.write_at(0, &[2u8; 50]).unwrap();

        assert_eq!(mgr.len(), 2);

        // Make cache2 more recent than cache1.
        let mut buf = [0u8; 10];
        cache2.read_at(0, &mut buf).unwrap();

        let _cache3 = mgr.get_or_create("big", 150).unwrap();

        assert!(mgr.len() <= 2);

        cleanup(&dir);
    }

    #[test]
    fn cache_manager_total_bytes() {
        let dir = temp_dir("mgr_bytes");
        let mut mgr = CacheManager::new(dir.clone(), CacheManager::DEFAULT_MAX_BYTES);

        let c1 = mgr.get_or_create("a", 1000).unwrap();
        c1.write_at(0, &[0u8; 100]).unwrap();

        let c2 = mgr.get_or_create("b", 2000).unwrap();
        c2.write_at(0, &[0u8; 200]).unwrap();

        // Files are pre-allocated to their declared size so partial downloads
        // report the correct length for the persistent catalog reconciliation.
        assert_eq!(mgr.total_bytes(), 3000);

        cleanup(&dir);
    }

    #[test]
    fn cache_manager_remove() {
        let dir = temp_dir("mgr_remove");
        let mut mgr = CacheManager::new(dir.clone(), CacheManager::DEFAULT_MAX_BYTES);

        mgr.get_or_create("removeme", 100).unwrap();
        assert_eq!(mgr.len(), 1);

        mgr.remove("removeme").unwrap();
        assert_eq!(mgr.len(), 0);
        assert!(mgr.is_empty());

        cleanup(&dir);
    }

    #[test]
    fn cache_manager_save_all_indices() {
        let dir = temp_dir("mgr_save");
        let mut mgr = CacheManager::new(dir.clone(), CacheManager::DEFAULT_MAX_BYTES);

        let c1 = mgr.get_or_create("s1", 200).unwrap();
        c1.write_at(0, &[0u8; 50]).unwrap();

        let c2 = mgr.get_or_create("s2", 200).unwrap();
        c2.write_at(0, &[0u8; 50]).unwrap();

        mgr.save_all_indices().unwrap();

        assert!(dir.join("s1.index").exists());
        assert!(dir.join("s2.index").exists());

        cleanup(&dir);
    }

    #[test]
    fn chunked_cache_data_bytes() {
        let dir = temp_dir("data_bytes");
        let cache = ChunkedCache::open(&dir, "db1", 500).unwrap();

        // File is pre-allocated to the declared size so partial downloads
        // report the correct length for the persistent catalog reconciliation.
        assert_eq!(cache.data_bytes(), 500);

        cache.write_at(0, &[0u8; 100]).unwrap();
        assert_eq!(cache.data_bytes(), 500);

        // Non-contiguous write does not extend the file past the pre-allocated
        // size.
        cache.write_at(300, &[0u8; 50]).unwrap();
        assert_eq!(cache.data_bytes(), 500);

        cleanup(&dir);
    }

    #[test]
    fn chunked_cache_last_access_updates() {
        let dir = temp_dir("last_access");
        let cache = ChunkedCache::open(&dir, "la1", 200).unwrap();

        let t0 = cache.last_access();

        std::thread::sleep(Duration::from_millis(10));

        cache.write_at(0, &[0u8; 50]).unwrap();
        let t1 = cache.last_access();
        assert!(t1 > t0);

        std::thread::sleep(Duration::from_millis(10));

        let mut buf = [0u8; 50];
        cache.read_at(0, &mut buf).unwrap();
        let t2 = cache.last_access();
        assert!(t2 > t1);

        cleanup(&dir);
    }

    #[test]
    fn read_at_times_out_when_no_data_arrives() {
        let dir = temp_dir("timeout");
        let cache = ChunkedCache::open(&dir, "timeout_test", 100).unwrap();

        let mut buf = vec![0u8; 50];
        let short_timeout = Duration::from_millis(100);
        let result = cache.read_at_with_timeout(0, &mut buf, short_timeout);

        assert!(
            matches!(result, Err(CacheError::Timeout)),
            "expected CacheError::Timeout, got {result:?}"
        );

        cleanup(&dir);
    }
}
