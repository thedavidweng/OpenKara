use super::chunked_cache::{self, ChunkedCache};
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{mpsc, Arc};
use symphonia::core::io::MediaSource;

/// Commands sent from the `RemoteMediaSource` to the background fetch thread.
pub enum FetchCommand {
    /// Fetch the given byte range from the remote URL.
    Fetch { offset: u64, length: u64 },
    /// Shut down the fetch thread.
    Shutdown,
}

/// A media source that fetches byte ranges on demand from a remote URL.
/// Implements `Read + Seek + MediaSource` so symphonia can use it directly.
///
/// Data is fetched via HTTP Range requests through a background fetch thread,
/// cached in a `ChunkedCache` on disk. Reads block until the requested range
/// is available.
pub struct RemoteMediaSource {
    cache: Arc<ChunkedCache>,
    read_position: u64,
    fetch_tx: mpsc::Sender<FetchCommand>,
    /// Minimum block size for fetch requests (bytes).
    min_fetch_size: u64,
}

impl RemoteMediaSource {
    pub fn new(cache: Arc<ChunkedCache>, fetch_tx: mpsc::Sender<FetchCommand>) -> Self {
        Self {
            cache,
            read_position: 0,
            fetch_tx,
            min_fetch_size: 64 * 1024, // 64 KB minimum fetch block
        }
    }

    /// Request a fetch for the range covering `offset..offset+length`.
    /// Coalesces small requests into minimum block size.
    fn request_fetch(&self, offset: u64, length: u64) {
        let fetch_length = length.max(self.min_fetch_size);
        let _ = self.fetch_tx.send(FetchCommand::Fetch {
            offset,
            length: fetch_length,
        });
    }
}

impl Read for RemoteMediaSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let offset = self.read_position;
        let length = buf.len() as u64;

        if length == 0 {
            return Ok(0);
        }

        // Check if data is already cached.
        if self.cache.is_cached(offset, length) {
            let read = self.cache.read_at(offset, buf).map_err(|e| {
                io::Error::new(io::ErrorKind::Other, format!("cache read error: {e}"))
            })?;
            self.read_position += read as u64;
            return Ok(read);
        }

        // Request a fetch for the missing range.
        self.request_fetch(offset, length);

        // Block until at least some data is available.
        let read = self
            .cache
            .read_at(offset, buf)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("cache read error: {e}")))?;
        self.read_position += read as u64;
        Ok(read)
    }
}

impl Seek for RemoteMediaSource {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let file_size = self.cache.file_size();
        self.read_position = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::End(offset) => {
                if offset > 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "seek from end with positive offset",
                    ));
                }
                file_size.saturating_sub((-offset) as u64)
            }
            SeekFrom::Current(offset) => {
                if offset >= 0 {
                    self.read_position.saturating_add(offset as u64)
                } else {
                    self.read_position.saturating_sub((-offset) as u64)
                }
            }
        };
        Ok(self.read_position)
    }
}

impl MediaSource for RemoteMediaSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.cache.file_size())
    }
}

/// Spawn a background fetch thread that receives `FetchCommand`s and fulfills
/// them via HTTP Range requests. Returns the command sender and the join handle.
///
/// The fetch thread runs until it receives `FetchCommand::Shutdown` or the
/// sender is dropped.
pub fn spawn_fetch_thread(
    url: String,
    cache: Arc<ChunkedCache>,
) -> (mpsc::Sender<FetchCommand>, std::thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel();

    let handle = std::thread::spawn(move || {
        let client = reqwest::blocking::Client::new();
        fetch_loop(&client, &url, &cache, &rx);
    });

    (tx, handle)
}

fn fetch_loop(
    client: &reqwest::blocking::Client,
    url: &str,
    cache: &ChunkedCache,
    rx: &mpsc::Receiver<FetchCommand>,
) {
    loop {
        match rx.recv() {
            Ok(FetchCommand::Fetch { offset, length }) => {
                if let Err(e) = fetch_range(client, url, cache, offset, length) {
                    eprintln!("fetch range {offset}+{length} failed: {e}");
                    // TODO: retry with backoff, emit playback-error after N failures.
                }
            }
            Ok(FetchCommand::Shutdown) | Err(_) => break,
        }
    }
}

fn fetch_range(
    client: &reqwest::blocking::Client,
    url: &str,
    cache: &ChunkedCache,
    offset: u64,
    length: u64,
) -> Result<(), FetchError> {
    let end = offset + length - 1;
    let range_header = format!("bytes={offset}-{end}");

    let response = client
        .get(url)
        .header("Range", &range_header)
        .send()
        .map_err(FetchError::Http)?;

    if !response.status().is_success() && response.status().as_u16() != 206 {
        return Err(FetchError::HttpStatus(response.status().as_u16()));
    }

    let bytes = response.bytes().map_err(FetchError::Http)?;
    cache
        .write_at(offset, &bytes)
        .map_err(|e| FetchError::Cache(e.to_string()))?;

    Ok(())
}

#[derive(Debug)]
enum FetchError {
    Http(reqwest::Error),
    HttpStatus(u16),
    Cache(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Http(e) => write!(f, "HTTP error: {e}"),
            FetchError::HttpStatus(code) => write!(f, "HTTP status {code}"),
            FetchError::Cache(msg) => write!(f, "cache error: {msg}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

    fn temp_dir(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "remote_source_test_{}_{suffix}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn read_from_preloaded_cache() {
        let dir = temp_dir("preloaded");
        let cache = Arc::new(ChunkedCache::open(&dir, "src", 100).unwrap());
        let data: Vec<u8> = (0..100).collect();
        cache.write_at(0, &data).unwrap();

        let (tx, _rx) = mpsc::channel();
        let mut source = RemoteMediaSource::new(Arc::clone(&cache), tx);

        let mut buf = vec![0u8; 100];
        let read = source.read(&mut buf).unwrap();
        assert_eq!(read, 100);
        assert_eq!(buf, data);
        assert_eq!(source.read_position, 100);

        cleanup(&dir);
    }

    #[test]
    fn read_triggers_fetch_and_blocks() {
        let dir = temp_dir("fetch_block");
        let cache = Arc::new(ChunkedCache::open(&dir, "src2", 100).unwrap());
        let (tx, rx) = mpsc::channel();

        let cache_clone = Arc::clone(&cache);
        let fetcher = std::thread::spawn(move || {
            // Simulate fetch: wait for command, then write data.
            if let Ok(FetchCommand::Fetch { offset, length: _ }) =
                rx.recv_timeout(Duration::from_secs(2))
            {
                let data = vec![42u8; 50];
                cache_clone.write_at(offset, &data).unwrap();
            }
        });

        let mut source = RemoteMediaSource::new(Arc::clone(&cache), tx);
        let mut buf = vec![0u8; 50];
        let read = source.read(&mut buf).unwrap();
        assert_eq!(read, 50);
        assert!(buf.iter().all(|&b| b == 42));

        fetcher.join().unwrap();
        cleanup(&dir);
    }

    #[test]
    fn seek_updates_position() {
        let dir = temp_dir("seek");
        let cache = Arc::new(ChunkedCache::open(&dir, "src3", 200).unwrap());
        let (tx, _rx) = mpsc::channel();
        let mut source = RemoteMediaSource::new(cache, tx);

        assert_eq!(source.seek(SeekFrom::Start(100)).unwrap(), 100);
        assert_eq!(source.seek(SeekFrom::Current(50)).unwrap(), 150);
        assert_eq!(source.seek(SeekFrom::Current(-20)).unwrap(), 130);
        assert_eq!(source.seek(SeekFrom::End(-10)).unwrap(), 190);

        cleanup(&dir);
    }

    #[test]
    fn media_source_traits() {
        let dir = temp_dir("media_traits");
        let cache = Arc::new(ChunkedCache::open(&dir, "src4", 500).unwrap());
        let (tx, _rx) = mpsc::channel();
        let source = RemoteMediaSource::new(cache, tx);

        assert!(source.is_seekable());
        assert_eq!(source.byte_len(), Some(500));

        cleanup(&dir);
    }

    #[test]
    fn partial_read_then_seek_and_read() {
        let dir = temp_dir("partial_seek");
        let cache = Arc::new(ChunkedCache::open(&dir, "src5", 200).unwrap());
        let data: Vec<u8> = (0..200).collect();
        cache.write_at(0, &data).unwrap();

        let (tx, _rx) = mpsc::channel();
        let mut source = RemoteMediaSource::new(cache, tx);

        // Read 50 bytes from start.
        let mut buf = vec![0u8; 50];
        source.read(&mut buf).unwrap();
        assert_eq!(buf, data[..50]);
        assert_eq!(source.read_position, 50);

        // Seek to 100.
        source.seek(SeekFrom::Start(100)).unwrap();

        // Read 50 bytes from 100.
        source.read(&mut buf).unwrap();
        assert_eq!(buf, data[100..150]);
        assert_eq!(source.read_position, 150);

        cleanup(&dir);
    }
}
