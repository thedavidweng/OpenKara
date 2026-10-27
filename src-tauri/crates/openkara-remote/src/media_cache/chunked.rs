use super::range_set::RangeSet;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

const READ_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum CacheError {
    Io(io::Error),
    Timeout,
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::Io(e) => write!(f, "cache I/O error: {e}"),
            CacheError::Timeout => write!(f, "cache read timed out waiting for data"),
        }
    }
}

impl std::error::Error for CacheError {}

impl From<io::Error> for CacheError {
    fn from(e: io::Error) -> Self {
        CacheError::Io(e)
    }
}

struct CacheInner {
    file: File,
    downloaded: RangeSet,
    file_size_bytes: u64,
}

/// A sparse on-disk copy of one remote object. Writers add byte ranges as
/// they arrive; readers block until the range they need is present.
pub struct ChunkedCache {
    path: PathBuf,
    inner: Mutex<CacheInner>,
    data_available: Condvar,
}

fn acquire_lock(mutex: &Mutex<CacheInner>) -> std::sync::MutexGuard<'_, CacheInner> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl ChunkedCache {
    /// Open `<cache_dir>/<cache_key>.cache` with no downloaded ranges.
    pub fn open(
        cache_dir: &Path,
        cache_key: &str,
        file_size_bytes: u64,
    ) -> Result<Self, CacheError> {
        Self::open_with_ranges(cache_dir, cache_key, file_size_bytes, RangeSet::new())
    }

    /// Open a cache file whose downloaded ranges are already known, such as
    /// ranges restored from the media cache catalog after a restart.
    pub fn open_with_ranges(
        cache_dir: &Path,
        cache_key: &str,
        file_size_bytes: u64,
        ranges: RangeSet,
    ) -> Result<Self, CacheError> {
        let data_path = cache_dir.join(format!("{cache_key}.cache"));

        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&data_path)?;

        if file.metadata()?.len() < file_size_bytes {
            file.set_len(file_size_bytes)?;
        }

        Ok(Self {
            path: data_path,
            inner: Mutex::new(CacheInner {
                file,
                downloaded: ranges,
                file_size_bytes,
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
        inner.file_size_bytes
    }

    pub fn is_complete(&self) -> bool {
        let inner = acquire_lock(&self.inner);
        inner.downloaded.covers_full(inner.file_size_bytes)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

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
        self.data_available.notify_all();
        Ok(())
    }

    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, CacheError> {
        self.read_at_with_timeout(offset, buf, READ_TIMEOUT)
    }

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
        Ok(buf.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn open_and_write_data() {
        let dir = TempDir::new().unwrap();
        let cache = ChunkedCache::open(dir.path(), "test1", 200).unwrap();

        assert!(!cache.is_cached(0, 100));
        assert_eq!(cache.file_size(), 200);
        assert!(!cache.is_complete());

        cache.write_at(0, &[42u8; 100]).unwrap();
        assert!(cache.is_cached(0, 100));
        assert!(!cache.is_cached(0, 200));
        assert_eq!(cache.cached_length_from(0, 200), 100);
    }

    #[test]
    fn read_cached_data() {
        let dir = TempDir::new().unwrap();
        let cache = ChunkedCache::open(dir.path(), "test2", 100).unwrap();

        let data = (0..100u8).collect::<Vec<_>>();
        cache.write_at(0, &data).unwrap();

        let mut buf = vec![0u8; 100];
        let read = cache.read_at(0, &mut buf).unwrap();
        assert_eq!(read, 100);
        assert_eq!(buf, data);
    }

    #[test]
    fn read_blocks_until_data_available() {
        let dir = TempDir::new().unwrap();
        let cache = Arc::new(ChunkedCache::open(dir.path(), "test3", 100).unwrap());

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
    }

    #[test]
    fn reopen_with_ranges_restores_downloaded_bytes() {
        let dir = TempDir::new().unwrap();
        let cache = ChunkedCache::open(dir.path(), "test4", 200).unwrap();
        cache.write_at(0, &[1u8; 50]).unwrap();
        cache.write_at(100, &[2u8; 50]).unwrap();
        let ranges = cache.downloaded();
        drop(cache);

        let cache2 = ChunkedCache::open_with_ranges(dir.path(), "test4", 200, ranges).unwrap();
        assert!(cache2.is_cached(0, 50));
        assert!(cache2.is_cached(100, 50));
        assert!(!cache2.is_cached(0, 200));
        let mut buf = [0u8; 50];
        cache2.read_at(100, &mut buf).unwrap();
        assert!(buf.iter().all(|&b| b == 2));
    }

    #[test]
    fn writing_every_byte_completes_the_cache() {
        let dir = TempDir::new().unwrap();
        let cache = ChunkedCache::open(dir.path(), "test5", 100).unwrap();
        cache.write_at(0, &[0u8; 60]).unwrap();
        assert!(!cache.is_complete());
        cache.write_at(60, &[0u8; 40]).unwrap();
        assert!(cache.is_complete());
    }

    #[test]
    fn partial_read_returns_available_bytes() {
        let dir = TempDir::new().unwrap();
        let cache = Arc::new(ChunkedCache::open(dir.path(), "test6", 200).unwrap());

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
    }

    #[test]
    fn partial_read_stops_at_uncached_gap() {
        let dir = TempDir::new().unwrap();
        let cache = ChunkedCache::open(dir.path(), "test_gap", 200).unwrap();

        cache.write_at(0, &[1u8; 30]).unwrap();
        cache.write_at(50, &[2u8; 30]).unwrap();

        let mut buf = vec![0u8; 80];
        let read = cache.read_at(0, &mut buf).unwrap();

        assert_eq!(read, 30);
        assert!(buf[..30].iter().all(|&b| b == 1));
    }

    #[test]
    fn read_at_times_out_when_no_data_arrives() {
        let dir = TempDir::new().unwrap();
        let cache = ChunkedCache::open(dir.path(), "timeout_test", 100).unwrap();

        let mut buf = vec![0u8; 50];
        let short_timeout = Duration::from_millis(100);
        let result = cache.read_at_with_timeout(0, &mut buf, short_timeout);

        assert!(
            matches!(result, Err(CacheError::Timeout)),
            "expected CacheError::Timeout, got {result:?}"
        );
    }
}
