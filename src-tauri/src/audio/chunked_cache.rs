use super::range_set::RangeSet;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

/// Errors specific to the chunked cache.
#[derive(Debug)]
pub enum CacheError {
    Io(io::Error),
    CorruptedIndex(String),
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::Io(e) => write!(f, "cache I/O error: {e}"),
            CacheError::CorruptedIndex(msg) => write!(f, "corrupted cache index: {msg}"),
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
    /// The cache data file.
    file: File,
    /// Tracks which byte ranges have been downloaded.
    downloaded: RangeSet,
    /// Total size of the remote file (known from HTTP Content-Length).
    file_size: u64,
    /// Last time this cache was read or written (for LRU eviction).
    last_access: Instant,
}

/// A chunked disk cache for a single remote file. Supports concurrent read
/// (from the decode/symphonia thread) and write (from the fetch thread) via
/// a mutex + condvar pattern.
pub struct ChunkedCache {
    path: PathBuf,
    inner: Mutex<CacheInner>,
    /// Notified when new data is written, so blocked readers can retry.
    data_available: Condvar,
}

/// Acquire the cache mutex, returning an IO error instead of panicking on poison.
fn acquire_lock(mutex: &Mutex<CacheInner>) -> std::sync::MutexGuard<'_, CacheInner> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl ChunkedCache {
    /// Open or create a chunked cache for the given file.
    ///
    /// - `cache_dir`: directory to store the cache data file and index.
    /// - `cache_key`: unique key for this file (e.g., content hash).
    /// - `file_size`: total size of the remote file.
    pub fn open(cache_dir: &Path, cache_key: &str, file_size: u64) -> Result<Self, CacheError> {
        let data_path = cache_dir.join(format!("{cache_key}.cache"));
        let index_path = cache_dir.join(format!("{cache_key}.index"));

        // Create or open the data file.
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&data_path)?;

        // Load existing index if present.
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

    /// Check if a range is fully cached.
    pub fn is_cached(&self, offset: u64, length: u64) -> bool {
        let inner = acquire_lock(&self.inner);
        inner.downloaded.contains(offset, length)
    }

    /// How many bytes starting from `offset` (up to `max_length`) are cached.
    pub fn cached_length_from(&self, offset: u64, max_length: u64) -> u64 {
        let inner = acquire_lock(&self.inner);
        inner.downloaded.contained_length_from(offset, max_length)
    }

    /// The RangeSet of downloaded ranges.
    pub fn downloaded(&self) -> RangeSet {
        let inner = acquire_lock(&self.inner);
        inner.downloaded.clone()
    }

    /// Total file size.
    pub fn file_size(&self) -> u64 {
        let inner = acquire_lock(&self.inner);
        inner.file_size
    }

    /// Whether the entire file is cached.
    pub fn is_complete(&self) -> bool {
        let inner = acquire_lock(&self.inner);
        inner.downloaded.covers_full(inner.file_size)
    }

    /// Last time this cache was accessed (read or write).
    pub fn last_access(&self) -> Instant {
        let inner = acquire_lock(&self.inner);
        inner.last_access
    }

    /// Total bytes of data stored in this cache (may include gaps filled with
    /// zeros due to `set_len` extension).
    pub fn data_bytes(&self) -> u64 {
        let inner = acquire_lock(&self.inner);
        inner.file.metadata().map(|m| m.len()).unwrap_or(0)
    }

    /// The cache file path on disk.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write data at the given offset. Updates the RangeSet and notifies
    /// any blocked readers. Extends the file if the write goes past the
    /// current end (required for non-contiguous downloads on macOS, which
    /// does not support sparse file holes).
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

    /// Read data at the given offset. Blocks (via condvar wait) if the range
    /// is not yet cached. Returns the number of bytes actually read.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, CacheError> {
        let mut inner = acquire_lock(&self.inner);
        let length = buf.len() as u64;

        // Wait until the range is available.
        while !inner.downloaded.contains(offset, length) {
            // If the range is partially available, read what we can.
            let available = inner.downloaded.contained_length_from(offset, length);
            if available > 0 {
                let to_read = available as usize;
                inner.file.seek(SeekFrom::Start(offset))?;
                inner.file.read_exact(&mut buf[..to_read])?;
                inner.last_access = Instant::now();
                return Ok(to_read);
            }
            inner = self.data_available.wait(inner).map_err(|_| {
                CacheError::Io(io::Error::new(io::ErrorKind::Other, "lock poisoned"))
            })?;
        }

        inner.file.seek(SeekFrom::Start(offset))?;
        inner.file.read_exact(buf)?;
        inner.last_access = Instant::now();
        Ok(buf.len())
    }

    /// Save the RangeSet index to disk as JSON.
    pub fn save_index(&self) -> Result<(), CacheError> {
        let inner = acquire_lock(&self.inner);
        if inner.downloaded.covers_full(inner.file_size) {
            // Complete file — remove the index file (it's equivalent to
            // a full cache, no need to track partial state).
            let index_path = self.index_path();
            if index_path.exists() {
                fs::remove_file(&index_path)?;
            }
            return Ok(());
        }

        let json = serde_json::to_string(&inner.downloaded)
            .map_err(|e| CacheError::Io(io::Error::new(io::ErrorKind::Other, e)))?;
        fs::write(self.index_path(), json)?;
        Ok(())
    }

    fn index_path(&self) -> PathBuf {
        self.path.with_extension("index")
    }
}

/// Manages multiple `ChunkedCache` instances with LRU eviction.
///
/// Tracks total disk usage across all managed caches. When the total exceeds
/// `max_bytes`, the least-recently-used cache is evicted (its data file and
/// index are deleted).
pub struct CacheManager {
    cache_dir: PathBuf,
    caches: HashMap<String, Arc<ChunkedCache>>,
    max_bytes: u64,
}

impl CacheManager {
    /// Create a new cache manager.
    ///
    /// - `cache_dir`: directory where cache files are stored.
    /// - `max_bytes`: maximum total disk usage across all caches (default 2GB).
    pub fn new(cache_dir: PathBuf, max_bytes: u64) -> Self {
        Self {
            cache_dir,
            caches: HashMap::new(),
            max_bytes,
        }
    }

    /// Default maximum cache size: 2 GB.
    pub const DEFAULT_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

    /// Get or create a cache for the given key. If the cache doesn't exist,
    /// it's created with the given `file_size`. Evicts LRU caches if needed.
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

    /// Total disk bytes used by all managed caches.
    pub fn total_bytes(&self) -> u64 {
        self.caches.values().map(|c| c.data_bytes()).sum()
    }

    /// Number of managed caches.
    pub fn len(&self) -> usize {
        self.caches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.caches.is_empty()
    }

    /// Evict least-recently-used caches until there's room for `needed_bytes`.
    fn evict_if_needed(&mut self, needed_bytes: u64) -> Result<(), CacheError> {
        let current: u64 = self.total_bytes();
        if current.saturating_add(needed_bytes) <= self.max_bytes {
            return Ok(());
        }

        // Sort by last_access (oldest first).
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
            // Delete the cache data file.
            let _ = fs::remove_file(cache.path());
            // Delete the index file.
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

    /// Remove a specific cache by key, deleting its files.
    pub fn remove(&mut self, key: &str) -> Result<(), CacheError> {
        if let Some(cache) = self.caches.remove(key) {
            cache.save_index().ok();
            let _ = fs::remove_file(cache.path());
            let index_path = cache.path().with_extension("index");
            let _ = fs::remove_file(&index_path);
        }
        Ok(())
    }

    /// Save all cache indices to disk.
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

        // Open a new cache with the same key — should load the index.
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

        // Index should be removed since the file is complete.
        let index_path = dir.join("test5.index");
        assert!(!index_path.exists());
        assert!(cache.is_complete());

        cleanup(&dir);
    }

    #[test]
    fn partial_read_returns_available_bytes() {
        let dir = temp_dir("partial_read");
        let cache = Arc::new(ChunkedCache::open(&dir, "test6", 200).unwrap());

        // Write only the first 50 bytes.
        cache.write_at(0, &[7u8; 50]).unwrap();

        // Try to read 100 bytes — should return only 50 (what's available).
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

        // Getting same key returns same cache.
        let cache2 = mgr.get_or_create("file1", 1000).unwrap();
        assert!(Arc::ptr_eq(&cache, &cache2));
        assert_eq!(mgr.len(), 1);

        cleanup(&dir);
    }

    #[test]
    fn cache_manager_evicts_lru() {
        let dir = temp_dir("mgr_evict");
        // Set max to 200 bytes — enough for one small cache but not two.
        let mut mgr = CacheManager::new(dir.clone(), 200);

        let cache1 = mgr.get_or_create("small1", 100).unwrap();
        cache1.write_at(0, &[1u8; 50]).unwrap();

        let cache2 = mgr.get_or_create("small2", 100).unwrap();
        cache2.write_at(0, &[2u8; 50]).unwrap();

        // Both should exist.
        assert_eq!(mgr.len(), 2);

        // Access cache2 to make it more recent.
        let mut buf = [0u8; 10];
        cache2.read_at(0, &mut buf).unwrap();

        // Now try to add a cache that would exceed the limit.
        // The manager should evict cache1 (LRU) to make room.
        let _cache3 = mgr.get_or_create("big", 150).unwrap();

        // cache1 should have been evicted.
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

        // Total should be 100 + 200 = 300 bytes of actual data.
        assert_eq!(mgr.total_bytes(), 300);

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

        // Index files should exist.
        assert!(dir.join("s1.index").exists());
        assert!(dir.join("s2.index").exists());

        cleanup(&dir);
    }

    #[test]
    fn chunked_cache_data_bytes() {
        let dir = temp_dir("data_bytes");
        let cache = ChunkedCache::open(&dir, "db1", 500).unwrap();

        assert_eq!(cache.data_bytes(), 0);

        cache.write_at(0, &[0u8; 100]).unwrap();
        assert_eq!(cache.data_bytes(), 100);

        // Non-contiguous write extends the file.
        cache.write_at(300, &[0u8; 50]).unwrap();
        assert_eq!(cache.data_bytes(), 350);

        cleanup(&dir);
    }

    #[test]
    fn chunked_cache_last_access_updates() {
        let dir = temp_dir("last_access");
        let cache = ChunkedCache::open(&dir, "la1", 200).unwrap();

        let t0 = cache.last_access();

        // Small delay to ensure time moves forward.
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
}
