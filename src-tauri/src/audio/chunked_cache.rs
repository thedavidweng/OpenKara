use super::range_set::RangeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

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
            }),
            data_available: Condvar::new(),
        })
    }

    /// Check if a range is fully cached.
    pub fn is_cached(&self, offset: u64, length: u64) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.downloaded.contains(offset, length)
    }

    /// How many bytes starting from `offset` (up to `max_length`) are cached.
    pub fn cached_length_from(&self, offset: u64, max_length: u64) -> u64 {
        let inner = self.inner.lock().unwrap();
        inner.downloaded.contained_length_from(offset, max_length)
    }

    /// The RangeSet of downloaded ranges.
    pub fn downloaded(&self) -> RangeSet {
        let inner = self.inner.lock().unwrap();
        inner.downloaded.clone()
    }

    /// Total file size.
    pub fn file_size(&self) -> u64 {
        let inner = self.inner.lock().unwrap();
        inner.file_size
    }

    /// Whether the entire file is cached.
    pub fn is_complete(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.downloaded.covers_full(inner.file_size)
    }

    /// Write data at the given offset. Updates the RangeSet and notifies
    /// any blocked readers. Extends the file if the write goes past the
    /// current end (required for non-contiguous downloads on macOS, which
    /// does not support sparse file holes).
    pub fn write_at(&self, offset: u64, data: &[u8]) -> Result<(), CacheError> {
        let mut inner = self.inner.lock().unwrap();
        let write_end = offset + data.len() as u64;
        let current_len = inner.file.metadata()?.len();
        if write_end > current_len {
            inner.file.set_len(write_end)?;
        }
        inner.file.seek(SeekFrom::Start(offset))?;
        inner.file.write_all(data)?;
        inner.downloaded.add_range(offset, data.len() as u64);
        self.data_available.notify_all();
        Ok(())
    }

    /// Read data at the given offset. Blocks (via condvar wait) if the range
    /// is not yet cached. Returns the number of bytes actually read.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, CacheError> {
        let mut inner = self.inner.lock().unwrap();
        let length = buf.len() as u64;

        // Wait until the range is available.
        while !inner.downloaded.contains(offset, length) {
            // If the range is partially available, read what we can.
            let available = inner.downloaded.contained_length_from(offset, length);
            if available > 0 {
                let to_read = available as usize;
                inner.file.seek(SeekFrom::Start(offset))?;
                inner.file.read_exact(&mut buf[..to_read])?;
                return Ok(to_read);
            }
            inner = self.data_available.wait(inner).map_err(|_| {
                CacheError::Io(io::Error::new(io::ErrorKind::Other, "lock poisoned"))
            })?;
        }

        inner.file.seek(SeekFrom::Start(offset))?;
        inner.file.read_exact(buf)?;
        Ok(buf.len())
    }

    /// Save the RangeSet index to disk as JSON.
    pub fn save_index(&self) -> Result<(), CacheError> {
        let inner = self.inner.lock().unwrap();
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
}
