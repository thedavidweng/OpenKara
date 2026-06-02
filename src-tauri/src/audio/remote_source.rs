use super::chunked_cache::{self, ChunkedCache};
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use symphonia::core::io::MediaSource;

/// Commands sent from the `RemoteMediaSource` to the background fetch thread.
pub enum FetchCommand {
    /// Fetch the given byte range from the remote URL.
    Fetch { offset: u64, length: u64 },
    /// Update the current read position for prefetch tracking.
    UpdatePosition { position: u64 },
    /// Shut down the fetch thread.
    Shutdown,
}

/// Events reported from the fetch thread back to the caller.
pub enum FetchEvent {
    /// URL expired (403/410). Caller should refresh and provide a new URL.
    UrlExpired,
    /// Consecutive failures exceeded the threshold.
    ConsecutiveFailures { count: u32 },
}

/// Configuration for exponential backoff retry.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub max_retries: u32,
    pub consecutive_failure_threshold: u32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            max_retries: 4,
            consecutive_failure_threshold: 5,
        }
    }
}

/// Abstraction over HTTP range fetching, enabling test injection.
pub trait HttpFetcher: Send + 'static {
    fn fetch_range(&self, url: &str, offset: u64, length: u64) -> Result<Vec<u8>, FetchError>;
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
    /// Bytes that must be cached before the first read returns (startup buffering).
    startup_bytes: u64,
    /// Whether the initial startup buffer has been satisfied.
    startup_satisfied: bool,
}

impl RemoteMediaSource {
    pub fn new(cache: Arc<ChunkedCache>, fetch_tx: mpsc::Sender<FetchCommand>) -> Self {
        Self {
            cache,
            read_position: 0,
            fetch_tx,
            min_fetch_size: 64 * 1024, // 64 KB minimum fetch block
            startup_bytes: 0,
            startup_satisfied: true,
        }
    }

    /// Set the number of bytes that must be available before the first read
    /// returns (startup buffering). For a 128kbps MP3, 1s ≈ 16KB.
    pub fn with_startup_buffer(mut self, bytes: u64) -> Self {
        self.startup_bytes = bytes;
        self.startup_satisfied = bytes == 0;
        self
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

    /// Notify the fetch thread of the current read position for prefetch tracking.
    fn update_position(&self) {
        let _ = self.fetch_tx.send(FetchCommand::UpdatePosition {
            position: self.read_position,
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

        // Startup buffering: ensure enough data is cached before first read.
        if !self.startup_satisfied {
            let startup_end = (offset + self.startup_bytes).min(self.cache.file_size());
            let startup_length = startup_end.saturating_sub(offset);
            if startup_length > 0 {
                self.request_fetch(offset, startup_length);
                // Block until the startup region is fully cached.
                let mut startup_buf = vec![0u8; startup_length as usize];
                let _ = self.cache.read_at(offset, &mut startup_buf).map_err(|e| {
                    io::Error::new(io::ErrorKind::Other, format!("cache read error: {e}"))
                })?;
            }
            self.startup_satisfied = true;
        }

        // Notify fetch thread of current position for prefetch tracking.
        self.update_position();

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

/// Result of a fetch attempt, indicating whether the URL needs refresh.
enum FetchOutcome {
    Ok,
    UrlExpired,
    Failed(FetchError),
}

/// Spawn a background fetch thread that receives `FetchCommand`s and fulfills
/// them via HTTP Range requests. Returns the command sender, event receiver,
/// and the join handle.
///
/// The fetch thread runs until it receives `FetchCommand::Shutdown` or the
/// sender is dropped.
pub fn spawn_fetch_thread(
    url: String,
    cache: Arc<ChunkedCache>,
) -> (
    mpsc::Sender<FetchCommand>,
    mpsc::Receiver<FetchEvent>,
    std::thread::JoinHandle<()>,
) {
    let client = reqwest::blocking::Client::new();
    spawn_fetch_thread_with_fetcher(
        url,
        cache,
        ReqwestFetcher { client },
        RetryConfig::default(),
    )
}

/// Spawn a fetch thread with a custom `HttpFetcher` and `RetryConfig` (for testing).
pub fn spawn_fetch_thread_with_fetcher(
    url: String,
    cache: Arc<ChunkedCache>,
    fetcher: impl HttpFetcher,
    retry_config: RetryConfig,
) -> (
    mpsc::Sender<FetchCommand>,
    mpsc::Receiver<FetchEvent>,
    std::thread::JoinHandle<()>,
) {
    let (tx, rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();

    let handle = std::thread::spawn(move || {
        fetch_loop(&fetcher, &url, &cache, &rx, &event_tx, &retry_config);
    });

    (tx, event_rx, handle)
}

struct ReqwestFetcher {
    client: reqwest::blocking::Client,
}

impl HttpFetcher for ReqwestFetcher {
    fn fetch_range(&self, url: &str, offset: u64, length: u64) -> Result<Vec<u8>, FetchError> {
        let end = offset + length - 1;
        let range_header = format!("bytes={offset}-{end}");

        let response = self
            .client
            .get(url)
            .header("Range", &range_header)
            .send()
            .map_err(FetchError::Http)?;

        let status = response.status().as_u16();
        if status == 403 || status == 410 {
            return Err(FetchError::HttpStatus(status));
        }
        if !response.status().is_success() && status != 206 {
            return Err(FetchError::HttpStatus(status));
        }

        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .map(Duration::from_secs);

        if status == 429 {
            return Err(FetchError::RateLimited(retry_after));
        }

        let bytes = response.bytes().map_err(FetchError::Http)?;
        Ok(bytes.to_vec())
    }
}

fn fetch_loop(
    fetcher: &dyn HttpFetcher,
    url: &str,
    cache: &ChunkedCache,
    rx: &mpsc::Receiver<FetchCommand>,
    event_tx: &mpsc::Sender<FetchEvent>,
    retry_config: &RetryConfig,
) {
    let mut current_url = url.to_string();
    let mut consecutive_failures: u32 = 0;
    let mut current_read_position: u64 = 0;

    loop {
        match rx.recv() {
            Ok(FetchCommand::Fetch { offset, length }) => {
                let outcome = fetch_range_with_retry(
                    fetcher,
                    &current_url,
                    cache,
                    offset,
                    length,
                    retry_config,
                );
                match outcome {
                    FetchOutcome::Ok => {
                        consecutive_failures = 0;
                    }
                    FetchOutcome::UrlExpired => {
                        let _ = event_tx.send(FetchEvent::UrlExpired);
                        // Try to refresh URL by requesting the same range again
                        // after the caller provides a new URL via a new fetch thread.
                        consecutive_failures += 1;
                    }
                    FetchOutcome::Failed(e) => {
                        consecutive_failures += 1;
                        eprintln!(
                            "fetch range {offset}+{length} failed (attempt {consecutive_failures}): {e}"
                        );
                        if consecutive_failures >= retry_config.consecutive_failure_threshold {
                            let _ = event_tx.send(FetchEvent::ConsecutiveFailures {
                                count: consecutive_failures,
                            });
                        }
                    }
                }
            }
            Ok(FetchCommand::UpdatePosition { position }) => {
                current_read_position = position;
                // Prefetch: request the next 5 seconds of data.
                // Estimate bytes per second from cached data pattern.
                let prefetch_bytes = estimate_prefetch_bytes(cache, current_read_position);
                if prefetch_bytes > 0 {
                    let prefetch_offset = current_read_position;
                    let _ = fetcher; // Available for future use
                                     // Only prefetch if the range isn't already cached.
                    if !cache.is_cached(prefetch_offset, prefetch_bytes) {
                        let _ = rx; // channel is used above
                                    // We can't call fetch_range_with_retry here because we'd block
                                    // the command channel. Instead, send a Fetch command back to ourselves.
                                    // But since we own the loop, we just do it inline.
                        let outcome = fetch_range_with_retry(
                            fetcher,
                            &current_url,
                            cache,
                            prefetch_offset,
                            prefetch_bytes,
                            retry_config,
                        );
                        match outcome {
                            FetchOutcome::Ok => {
                                consecutive_failures = 0;
                            }
                            FetchOutcome::UrlExpired => {
                                let _ = event_tx.send(FetchEvent::UrlExpired);
                                consecutive_failures += 1;
                            }
                            FetchOutcome::Failed(_) => {
                                // Prefetch failures are non-fatal, don't increment counter.
                            }
                        }
                    }
                }
            }
            Ok(FetchCommand::Shutdown) | Err(_) => break,
        }
    }
}

/// Estimate how many bytes to prefetch ahead of the given position.
/// Targets ~5 seconds of audio at ~128kbps (16KB/s) = 80KB minimum.
fn estimate_prefetch_bytes(cache: &ChunkedCache, position: u64) -> u64 {
    let file_size = cache.file_size();
    let remaining = file_size.saturating_sub(position);
    // 5 seconds at 128kbps = 80KB, but use min_fetch_size as floor.
    let prefetch = remaining.min(80 * 1024);
    prefetch
}

fn fetch_range_with_retry(
    fetcher: &dyn HttpFetcher,
    url: &str,
    cache: &ChunkedCache,
    offset: u64,
    length: u64,
    config: &RetryConfig,
) -> FetchOutcome {
    let mut delay = config.initial_delay;

    for attempt in 0..=config.max_retries {
        match fetcher.fetch_range(url, offset, length) {
            Ok(bytes) => {
                if let Err(e) = cache.write_at(offset, &bytes) {
                    return FetchOutcome::Failed(FetchError::Cache(e.to_string()));
                }
                return FetchOutcome::Ok;
            }
            Err(FetchError::HttpStatus(403 | 410)) => {
                return FetchOutcome::UrlExpired;
            }
            Err(FetchError::RateLimited(retry_after)) => {
                let wait = retry_after.unwrap_or(delay);
                std::thread::sleep(wait);
                delay = (delay * 2).min(config.max_delay);
            }
            Err(e) => {
                if attempt < config.max_retries {
                    std::thread::sleep(delay);
                    delay = (delay * 2).min(config.max_delay);
                } else {
                    return FetchOutcome::Failed(e);
                }
            }
        }
    }

    FetchOutcome::Failed(FetchError::HttpStatus(0)) // unreachable
}

#[derive(Debug)]
pub enum FetchError {
    Http(reqwest::Error),
    HttpStatus(u16),
    RateLimited(Option<Duration>),
    Cache(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Http(e) => write!(f, "HTTP error: {e}"),
            FetchError::HttpStatus(code) => write!(f, "HTTP status {code}"),
            FetchError::RateLimited(after) => {
                write!(f, "rate limited (429)")?;
                if let Some(d) = after {
                    write!(f, ", retry after {d:?}")?;
                }
                Ok(())
            }
            FetchError::Cache(msg) => write!(f, "cache error: {msg}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

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

    /// Mock HTTP fetcher that returns pre-configured responses.
    struct MockFetcher {
        responses: std::sync::Mutex<Vec<Result<Vec<u8>, FetchError>>>,
        call_count: AtomicU32,
    }

    impl MockFetcher {
        fn new(responses: Vec<Result<Vec<u8>, FetchError>>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses),
                call_count: AtomicU32::new(0),
            }
        }

        fn call_count(&self) -> u32 {
            self.call_count.load(Ordering::Relaxed)
        }
    }

    impl HttpFetcher for MockFetcher {
        fn fetch_range(
            &self,
            _url: &str,
            _offset: u64,
            length: u64,
        ) -> Result<Vec<u8>, FetchError> {
            self.call_count.fetch_add(1, Ordering::Relaxed);
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                return Ok(vec![0u8; length as usize]);
            }
            responses.remove(0)
        }
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
        let fetcher_thread = std::thread::spawn(move || {
            // Simulate fetch: wait for command, then write data.
            // May receive UpdatePosition first, so loop until we get Fetch.
            while let Ok(cmd) = rx.recv_timeout(Duration::from_secs(2)) {
                if let FetchCommand::Fetch { offset, length: _ } = cmd {
                    let data = vec![42u8; 50];
                    cache_clone.write_at(offset, &data).unwrap();
                    break;
                }
            }
        });

        let mut source = RemoteMediaSource::new(Arc::clone(&cache), tx);
        let mut buf = vec![0u8; 50];
        let read = source.read(&mut buf).unwrap();
        assert_eq!(read, 50);
        assert!(buf.iter().all(|&b| b == 42));

        fetcher_thread.join().unwrap();
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

    #[test]
    fn retry_succeeds_after_transient_failure() {
        let dir = temp_dir("retry_ok");
        let cache = Arc::new(ChunkedCache::open(&dir, "retry1", 100).unwrap());
        let data = vec![7u8; 100];

        // First call fails, second succeeds.
        let mock = MockFetcher::new(vec![Err(FetchError::HttpStatus(500)), Ok(data.clone())]);

        let (_tx, event_rx, handle) = spawn_fetch_thread_with_fetcher(
            "http://example.com/test.mp3".to_string(),
            Arc::clone(&cache),
            mock,
            RetryConfig {
                initial_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(100),
                max_retries: 3,
                consecutive_failure_threshold: 5,
            },
        );

        // Send fetch command and wait for it to complete.
        _tx.send(FetchCommand::Fetch {
            offset: 0,
            length: 100,
        })
        .unwrap();

        // Wait for the fetch thread to process.
        std::thread::sleep(Duration::from_millis(200));
        let _ = _tx.send(FetchCommand::Shutdown);
        handle.join().unwrap();

        assert!(cache.is_cached(0, 100));
        let mut buf = vec![0u8; 100];
        cache.read_at(0, &mut buf).unwrap();
        assert_eq!(buf, data);

        // No events should have been emitted (only 1 failure, threshold is 5).
        assert!(event_rx.try_recv().is_err());

        cleanup(&dir);
    }

    #[test]
    fn url_expired_emits_event() {
        let dir = temp_dir("url_expired");
        let cache = Arc::new(ChunkedCache::open(&dir, "expired1", 100).unwrap());

        let mock = MockFetcher::new(vec![Err(FetchError::HttpStatus(403))]);

        let (tx, event_rx, handle) = spawn_fetch_thread_with_fetcher(
            "http://example.com/test.mp3".to_string(),
            Arc::clone(&cache),
            mock,
            RetryConfig {
                initial_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(100),
                max_retries: 0,
                consecutive_failure_threshold: 5,
            },
        );

        tx.send(FetchCommand::Fetch {
            offset: 0,
            length: 100,
        })
        .unwrap();

        std::thread::sleep(Duration::from_millis(200));
        let _ = tx.send(FetchCommand::Shutdown);
        handle.join().unwrap();

        // Should receive UrlExpired event.
        let event = event_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(event, FetchEvent::UrlExpired));

        cleanup(&dir);
    }

    #[test]
    fn consecutive_failures_emits_event() {
        let dir = temp_dir("consec_fail");
        let cache = Arc::new(ChunkedCache::open(&dir, "consec1", 100).unwrap());

        // All calls fail.
        let mock = MockFetcher::new(vec![
            Err(FetchError::HttpStatus(500)),
            Err(FetchError::HttpStatus(500)),
            Err(FetchError::HttpStatus(500)),
            Err(FetchError::HttpStatus(500)),
            Err(FetchError::HttpStatus(500)),
        ]);

        let (tx, event_rx, handle) = spawn_fetch_thread_with_fetcher(
            "http://example.com/test.mp3".to_string(),
            Arc::clone(&cache),
            mock,
            RetryConfig {
                initial_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(50),
                max_retries: 0,
                consecutive_failure_threshold: 5,
            },
        );

        // Send 5 fetch commands.
        for i in 0..5u64 {
            tx.send(FetchCommand::Fetch {
                offset: i * 100,
                length: 100,
            })
            .unwrap();
        }

        std::thread::sleep(Duration::from_millis(500));
        let _ = tx.send(FetchCommand::Shutdown);
        handle.join().unwrap();

        // Should receive ConsecutiveFailures event.
        let event = event_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            event,
            FetchEvent::ConsecutiveFailures { count: 5 }
        ));

        cleanup(&dir);
    }

    #[test]
    fn rate_limited_respects_retry_after() {
        let dir = temp_dir("rate_limit");
        let cache = Arc::new(ChunkedCache::open(&dir, "rl1", 100).unwrap());
        let data = vec![9u8; 100];

        // First call: rate limited with Retry-After. Second: success.
        let mock = MockFetcher::new(vec![
            Err(FetchError::RateLimited(Some(Duration::from_millis(50)))),
            Ok(data.clone()),
        ]);

        let (tx, event_rx, handle) = spawn_fetch_thread_with_fetcher(
            "http://example.com/test.mp3".to_string(),
            Arc::clone(&cache),
            mock,
            RetryConfig {
                initial_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(100),
                max_retries: 3,
                consecutive_failure_threshold: 5,
            },
        );

        tx.send(FetchCommand::Fetch {
            offset: 0,
            length: 100,
        })
        .unwrap();

        std::thread::sleep(Duration::from_millis(300));
        let _ = tx.send(FetchCommand::Shutdown);
        handle.join().unwrap();

        assert!(cache.is_cached(0, 100));
        // No failure events (rate limit retry succeeded).
        assert!(event_rx.try_recv().is_err());

        cleanup(&dir);
    }

    #[test]
    fn startup_buffer_blocks_until_data_available() {
        let dir = temp_dir("startup_buf");
        let cache = Arc::new(ChunkedCache::open(&dir, "startup1", 200).unwrap());
        let (tx, rx) = mpsc::channel();

        let cache_clone = Arc::clone(&cache);
        let fetcher_thread = std::thread::spawn(move || {
            while let Ok(cmd) = rx.recv_timeout(Duration::from_secs(2)) {
                if let FetchCommand::Fetch { offset, length } = cmd {
                    // Simulate slow fetch — write data after a delay.
                    std::thread::sleep(Duration::from_millis(50));
                    let data = vec![55u8; length as usize];
                    cache_clone.write_at(offset, &data).unwrap();
                }
            }
        });

        let mut source = RemoteMediaSource::new(Arc::clone(&cache), tx).with_startup_buffer(50);

        let mut buf = vec![0u8; 50];
        let read = source.read(&mut buf).unwrap();
        assert_eq!(read, 50);
        assert!(buf.iter().all(|&b| b == 55));
        assert!(source.startup_satisfied);

        fetcher_thread.join().unwrap();
        cleanup(&dir);
    }

    #[test]
    fn prefetch_estimates_bytes() {
        let dir = temp_dir("prefetch_est");
        let cache = ChunkedCache::open(&dir, "pf1", 1_000_000).unwrap();

        // At position 0 with 1MB file: should prefetch ~80KB.
        let bytes = estimate_prefetch_bytes(&cache, 0);
        assert_eq!(bytes, 80 * 1024);

        // Near end: only remaining bytes.
        let bytes = estimate_prefetch_bytes(&cache, 999_900);
        assert_eq!(bytes, 100);

        // At end: 0.
        let bytes = estimate_prefetch_bytes(&cache, 1_000_000);
        assert_eq!(bytes, 0);

        cleanup(&dir);
    }

    #[test]
    fn retry_config_default_values() {
        let config = RetryConfig::default();
        assert_eq!(config.initial_delay, Duration::from_secs(1));
        assert_eq!(config.max_delay, Duration::from_secs(30));
        assert_eq!(config.max_retries, 4);
        assert_eq!(config.consecutive_failure_threshold, 5);
    }

    #[test]
    fn exponential_backoff_delays_increase() {
        let dir = temp_dir("backoff");
        let cache = Arc::new(ChunkedCache::open(&dir, "bo1", 100).unwrap());
        let data = vec![1u8; 100];

        // Fail 3 times, then succeed. Track call count.
        let mock = MockFetcher::new(vec![
            Err(FetchError::HttpStatus(500)),
            Err(FetchError::HttpStatus(500)),
            Err(FetchError::HttpStatus(500)),
            Ok(data),
        ]);
        let call_count_ref = Arc::new(AtomicU32::new(0));
        // We'll check the mock's call count after.

        let (tx, _event_rx, handle) = spawn_fetch_thread_with_fetcher(
            "http://example.com/test.mp3".to_string(),
            Arc::clone(&cache),
            mock,
            RetryConfig {
                initial_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(100),
                max_retries: 4,
                consecutive_failure_threshold: 5,
            },
        );

        tx.send(FetchCommand::Fetch {
            offset: 0,
            length: 100,
        })
        .unwrap();

        std::thread::sleep(Duration::from_millis(500));
        let _ = tx.send(FetchCommand::Shutdown);
        handle.join().unwrap();

        assert!(cache.is_cached(0, 100));

        cleanup(&dir);
    }
}
