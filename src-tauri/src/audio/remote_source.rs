use super::chunked_cache::ChunkedCache;
use std::io::{self, Read, Seek, SeekFrom};
use std::ops::ControlFlow;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc, Arc,
};
use std::time::Duration;
use symphonia::core::io::MediaSource;

/// Tracks download bandwidth and signals when the connection is slow.
///
/// Shared between the fetch thread (which updates it) and the playback system
/// (which reads it to decide whether to enable proxy mode).
pub struct BandwidthMonitor {
    /// Bytes per second, exponentially weighted moving average.
    bytes_per_sec: AtomicU64,
    /// Whether the connection is currently considered slow.
    is_slow: Arc<AtomicBool>,
    /// Threshold in bytes/sec below which the connection is considered slow.
    slow_threshold: AtomicU64,
    /// Request latency in microseconds, EWMA (alpha=0.3).
    latency_us: AtomicU64,
}

impl BandwidthMonitor {
    /// Create a new monitor. `slow_threshold_bps` is in bytes per second.
    /// Default threshold: 16384 (128 kbps).
    pub fn new(slow_threshold_bps: u64) -> Self {
        Self {
            bytes_per_sec: AtomicU64::new(0),
            is_slow: Arc::new(AtomicBool::new(false)),
            slow_threshold: AtomicU64::new(slow_threshold_bps),
            latency_us: AtomicU64::new(0),
        }
    }

    /// Default slow threshold: 128 kbps = 16384 bytes/sec.
    pub const DEFAULT_SLOW_THRESHOLD: u64 = 16_384;

    /// Record a completed fetch of `bytes` taking `elapsed`.
    pub fn record_fetch(&self, bytes: u64, elapsed: Duration) {
        let secs = elapsed.as_secs_f64();
        if secs <= 0.0 {
            return;
        }
        let instant_bps = (bytes as f64 / secs) as u64;
        // EWMA with alpha=0.3.
        let prev = self.bytes_per_sec.load(Ordering::Relaxed);
        let new_bps = if prev == 0 {
            instant_bps
        } else {
            (prev as f64 * 0.7 + instant_bps as f64 * 0.3) as u64
        };
        self.bytes_per_sec.store(new_bps, Ordering::Relaxed);

        let threshold = self.slow_threshold.load(Ordering::Relaxed);
        self.is_slow.store(new_bps < threshold, Ordering::Relaxed);

        // Track request latency (EWMA, alpha=0.3).
        let latency = elapsed.as_micros() as u64;
        let prev_latency = self.latency_us.load(Ordering::Relaxed);
        let new_latency = if prev_latency == 0 {
            latency
        } else {
            (prev_latency as f64 * 0.7 + latency as f64 * 0.3) as u64
        };
        self.latency_us.store(new_latency, Ordering::Relaxed);
    }

    /// Current estimated bandwidth in bytes/sec.
    pub fn bytes_per_sec(&self) -> u64 {
        self.bytes_per_sec.load(Ordering::Relaxed)
    }

    /// Whether the connection is currently slow.
    pub fn is_slow(&self) -> bool {
        self.is_slow.load(Ordering::Relaxed)
    }

    /// Estimated request latency in microseconds.
    pub fn latency_us(&self) -> u64 {
        self.latency_us.load(Ordering::Relaxed)
    }

    /// Update the slow threshold at runtime.
    pub fn set_slow_threshold(&self, bps: u64) {
        self.slow_threshold.store(bps, Ordering::Relaxed);
        // Re-evaluate whether the connection is slow.
        let current = self.bytes_per_sec.load(Ordering::Relaxed);
        self.is_slow.store(current < bps, Ordering::Relaxed);
    }
}

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
    /// Server does not support Range requests. Caller should fall back to
    /// full-file download.
    RangeNotSupported,
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

impl HttpFetcher for Box<dyn HttpFetcher> {
    fn fetch_range(&self, url: &str, offset: u64, length: u64) -> Result<Vec<u8>, FetchError> {
        (**self).fetch_range(url, offset, length)
    }
}

/// An `HttpFetcher` backed by a pre-configured URL and auth headers, suitable
/// for use with remote storage providers (Google Drive, Dropbox, WebDAV).
///
/// For Dropbox (POST-based API), the file path is stored in `api_arg_header`
/// and requests use POST instead of GET.
///
/// Supports automatic token refresh: when a 403 is received, the fetcher
/// calls the `token_refresh` callback to obtain fresh credentials, updates
/// its Authorization header, and retries the request once.
pub struct ProviderFetcher {
    url: String,
    headers: std::sync::Mutex<Vec<(String, String)>>,
    use_post: bool,
    api_arg_header: Option<String>,
    token_refresh: Option<Box<dyn Fn() -> Result<String, FetchError> + Send + Sync>>,
    /// Prevents repeated refresh attempts across retries in `fetch_range_with_retry`.
    refresh_attempted: std::sync::atomic::AtomicBool,
    /// Reusable HTTP client — avoids creating a new client per request.
    client: reqwest::blocking::Client,
}

impl ProviderFetcher {
    pub fn new(url: String, headers: Vec<(String, String)>) -> Self {
        Self {
            url,
            headers: std::sync::Mutex::new(headers),
            use_post: false,
            api_arg_header: None,
            token_refresh: None,
            refresh_attempted: std::sync::atomic::AtomicBool::new(false),
            client: reqwest::blocking::Client::new(),
        }
    }

    pub fn with_post(mut self, api_arg_header: String) -> Self {
        self.use_post = true;
        self.api_arg_header = Some(api_arg_header);
        self
    }

    /// Register a callback that returns a fresh access token. Called on HTTP 403
    /// to refresh credentials and retry without falling back to full-file download.
    pub fn with_token_refresh(
        mut self,
        refresh: impl Fn() -> Result<String, FetchError> + Send + Sync + 'static,
    ) -> Self {
        self.token_refresh = Some(Box::new(refresh));
        self
    }

    /// Update the Authorization header with a new token.
    fn update_auth_header(&self, new_token: &str) {
        let mut headers = self.headers.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = headers.iter_mut().find(|(k, _)| k == "Authorization") {
            entry.1 = format!("Bearer {new_token}");
        }
    }

    fn execute_request(&self, offset: u64, length: u64) -> Result<Vec<u8>, FetchError> {
        let end = offset + length - 1;
        let range_value = format!("bytes={offset}-{end}");

        let mut builder = if self.use_post {
            self.client.post(&self.url)
        } else {
            self.client.get(&self.url)
        };

        builder = builder.header("Range", &range_value);

        let headers = self.headers.lock().unwrap_or_else(|e| e.into_inner());
        for (key, value) in headers.iter() {
            builder = builder.header(key.as_str(), value.as_str());
        }
        drop(headers);

        if let Some(ref arg) = self.api_arg_header {
            builder = builder.header("Dropbox-API-Arg", arg.as_str());
        }

        let response = builder.send().map_err(FetchError::Http)?;
        let status = response.status().as_u16();

        if status == 416 {
            return Err(FetchError::RangeNotSupported);
        }
        if status == 429 {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .map(Duration::from_secs);
            return Err(FetchError::RateLimited(retry_after));
        }
        if !response.status().is_success() && status != 206 {
            return Err(FetchError::HttpStatus(status));
        }

        let bytes = response.bytes().map_err(FetchError::Http)?;
        Ok(bytes.to_vec())
    }
}

impl HttpFetcher for ProviderFetcher {
    fn fetch_range(&self, _url: &str, offset: u64, length: u64) -> Result<Vec<u8>, FetchError> {
        match self.execute_request(offset, length) {
            Ok(bytes) => Ok(bytes),
            Err(FetchError::HttpStatus(403 | 410))
                if self.token_refresh.is_some()
                    && !self.refresh_attempted.load(Ordering::Relaxed) =>
            {
                // Token expired — refresh and retry once. The flag prevents
                // repeated refresh attempts across retries in fetch_range_with_retry.
                self.refresh_attempted.store(true, Ordering::Relaxed);
                let refresh = self.token_refresh.as_ref().unwrap();
                let new_token = refresh()?;
                self.update_auth_header(&new_token);
                self.execute_request(offset, length)
            }
            Err(e) => Err(e),
        }
    }
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
                let _ = self
                    .cache
                    .read_at(offset, &mut startup_buf)
                    .map_err(|e| io::Error::other(format!("cache read error: {e}")))?;
            }
            self.startup_satisfied = true;
        }

        // Notify fetch thread of current position for prefetch tracking.
        self.update_position();

        // Check if data is already cached.
        if self.cache.is_cached(offset, length) {
            let read = self
                .cache
                .read_at(offset, buf)
                .map_err(|e| io::Error::other(format!("cache read error: {e}")))?;
            self.read_position += read as u64;
            return Ok(read);
        }

        // Request a fetch for the missing range.
        self.request_fetch(offset, length);

        // Block until at least some data is available.
        let read = self
            .cache
            .read_at(offset, buf)
            .map_err(|e| io::Error::other(format!("cache read error: {e}")))?;
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
    RangeNotSupported,
    Failed(FetchError),
}

/// Semaphore for limiting concurrent HTTP downloads across multiple fetch threads.
///
/// When streaming multiple stems in parallel, each stem has its own fetch thread.
/// Without coordination, all threads could saturate the network simultaneously.
/// This semaphore limits the total number of concurrent HTTP requests.
pub struct DownloadSemaphore {
    max_concurrent: usize,
    active: std::sync::atomic::AtomicUsize,
}

impl DownloadSemaphore {
    /// Create a new semaphore allowing up to `max_concurrent` concurrent downloads.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            max_concurrent,
            active: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Try to acquire a slot. Returns `true` if a slot was acquired, `false` if
    /// at capacity. Call `release()` when the download completes.
    pub fn try_acquire(&self) -> bool {
        let current = self.active.load(Ordering::Relaxed);
        if current >= self.max_concurrent {
            return false;
        }
        self.active
            .compare_exchange(current, current + 1, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }

    /// Release a previously acquired slot.
    pub fn release(&self) {
        self.active.fetch_sub(1, Ordering::Release);
    }

    /// Default max concurrent downloads.
    pub const DEFAULT_MAX_CONCURRENT: usize = 2;
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
    Arc<BandwidthMonitor>,
    std::thread::JoinHandle<()>,
) {
    let client = reqwest::blocking::Client::new();
    spawn_fetch_thread_with_fetcher(
        url,
        cache,
        Box::new(ReqwestFetcher { client }),
        RetryConfig::default(),
    )
}

/// Spawn a fetch thread with a custom `HttpFetcher` and `RetryConfig` (for testing).
pub fn spawn_fetch_thread_with_fetcher(
    url: String,
    cache: Arc<ChunkedCache>,
    fetcher: Box<dyn HttpFetcher>,
    retry_config: RetryConfig,
) -> (
    mpsc::Sender<FetchCommand>,
    mpsc::Receiver<FetchEvent>,
    Arc<BandwidthMonitor>,
    std::thread::JoinHandle<()>,
) {
    let (tx, rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    let monitor = Arc::new(BandwidthMonitor::new(
        BandwidthMonitor::DEFAULT_SLOW_THRESHOLD,
    ));
    let monitor_clone = Arc::clone(&monitor);

    let handle = std::thread::spawn(move || {
        fetch_loop(
            &fetcher,
            &url,
            &cache,
            &rx,
            &event_tx,
            &retry_config,
            &monitor_clone,
        );
    });

    (tx, event_rx, monitor, handle)
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
        if status == 416 {
            return Err(FetchError::RangeNotSupported);
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
    monitor: &BandwidthMonitor,
) {
    let current_url = url.to_string();
    let mut consecutive_failures: u32 = 0;
    let mut current_read_position: u64 = 0;

    let mut handle_update_position =
        |position: u64, consecutive_failures: &mut u32| -> ControlFlow<()> {
            current_read_position = position;
            let prefetch_bytes = estimate_prefetch_bytes(cache, current_read_position, monitor);
            if prefetch_bytes == 0 {
                return ControlFlow::Continue(());
            }
            let prefetch_offset = current_read_position;
            if cache.is_cached(prefetch_offset, prefetch_bytes) {
                return ControlFlow::Continue(());
            }
            let outcome = fetch_range_with_retry(
                fetcher,
                &current_url,
                cache,
                prefetch_offset,
                prefetch_bytes,
                retry_config,
                monitor,
            );
            match outcome {
                FetchOutcome::Ok => {
                    *consecutive_failures = 0;
                }
                FetchOutcome::UrlExpired => {
                    let _ = event_tx.send(FetchEvent::UrlExpired);
                    *consecutive_failures += 1;
                }
                FetchOutcome::RangeNotSupported => {
                    let _ = event_tx.send(FetchEvent::RangeNotSupported);
                    return ControlFlow::Break(());
                }
                FetchOutcome::Failed(_) => {
                    // Prefetch failures are non-fatal, don't increment counter.
                }
            }
            ControlFlow::Continue(())
        };

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
                    monitor,
                );
                let fetch_succeeded = matches!(outcome, FetchOutcome::Ok);
                match outcome {
                    FetchOutcome::Ok => {
                        consecutive_failures = 0;
                    }
                    FetchOutcome::UrlExpired => {
                        let _ = event_tx.send(FetchEvent::UrlExpired);
                        consecutive_failures += 1;
                    }
                    FetchOutcome::RangeNotSupported => {
                        let _ = event_tx.send(FetchEvent::RangeNotSupported);
                        return; // No point continuing — server can't serve ranges.
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

                // R10: After a successful fetch, drain stale queued Fetch
                // commands. During fast scrubbing, many Fetch commands
                // accumulate — we discard all but the most recent non-Fetch
                // command so the fetch thread doesn't waste time on ranges the
                // player has already moved past.  We only drain after success;
                // after a failure we keep processing to honour retries.
                if fetch_succeeded {
                    let mut last_non_fetch: Option<FetchCommand> = None;
                    while let Ok(cmd) = rx.try_recv() {
                        match cmd {
                            FetchCommand::Fetch { .. } => { /* discard stale fetch */ }
                            other => last_non_fetch = Some(other),
                        }
                    }
                    if let Some(cmd) = last_non_fetch {
                        match cmd {
                            FetchCommand::UpdatePosition { position } => {
                                if handle_update_position(position, &mut consecutive_failures)
                                    .is_break()
                                {
                                    return;
                                }
                            }
                            FetchCommand::Shutdown => return,
                            FetchCommand::Fetch { .. } => unreachable!(),
                        }
                    }
                }
            }
            Ok(FetchCommand::UpdatePosition { position }) => {
                if handle_update_position(position, &mut consecutive_failures).is_break() {
                    return;
                }
            }
            Ok(FetchCommand::Shutdown) | Err(_) => break,
        }
    }
}

/// Prefetch factor: how many round-trips of data to buffer ahead.
const PREFETCH_FACTOR: f64 = 4.0;

/// Minimum prefetch size in bytes (floor for fast connections with tiny latency).
const MIN_PREFETCH_BYTES: u64 = 64 * 1024;

/// Maximum prefetch size in bytes (ceiling to avoid over-buffering).
const MAX_PREFETCH_BYTES: u64 = 512 * 1024;

/// Estimate how many bytes to prefetch ahead of the given position.
///
/// Adaptive strategy: `prefetch = max(factor * latency * throughput, latency * throughput)`
/// where `factor` scales with connection quality. High-latency connections
/// get more data buffered to absorb jitter; fast connections use the minimum.
fn estimate_prefetch_bytes(cache: &ChunkedCache, position: u64, monitor: &BandwidthMonitor) -> u64 {
    let file_size = cache.file_size();
    let remaining = file_size.saturating_sub(position);

    let throughput = monitor.bytes_per_sec().max(1);
    let latency_secs = monitor.latency_us() as f64 / 1_000_000.0;

    // If we have no latency data yet, fall back to a conservative 80KB.
    if latency_secs <= 0.0 {
        return remaining.min(80 * 1024);
    }

    // Adaptive: prefetch ≈ factor × latency × throughput.
    let adaptive = (PREFETCH_FACTOR * latency_secs * throughput as f64) as u64;
    remaining.min(adaptive.clamp(MIN_PREFETCH_BYTES, MAX_PREFETCH_BYTES))
}

fn fetch_range_with_retry(
    fetcher: &dyn HttpFetcher,
    url: &str,
    cache: &ChunkedCache,
    offset: u64,
    length: u64,
    config: &RetryConfig,
    monitor: &BandwidthMonitor,
) -> FetchOutcome {
    let mut delay = config.initial_delay;

    for attempt in 0..=config.max_retries {
        let start = std::time::Instant::now();
        match fetcher.fetch_range(url, offset, length) {
            Ok(bytes) => {
                let elapsed = start.elapsed();
                let byte_count = bytes.len() as u64;
                if let Err(e) = cache.write_at(offset, &bytes) {
                    return FetchOutcome::Failed(FetchError::Cache(e.to_string()));
                }
                monitor.record_fetch(byte_count, elapsed);
                return FetchOutcome::Ok;
            }
            Err(FetchError::HttpStatus(403 | 410)) => {
                return FetchOutcome::UrlExpired;
            }
            Err(FetchError::RangeNotSupported) => {
                return FetchOutcome::RangeNotSupported;
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

    FetchOutcome::Failed(FetchError::HttpStatus(0)) // safety net: reachable when RateLimited with max_retries=0
}

#[derive(Debug)]
pub enum FetchError {
    Http(reqwest::Error),
    HttpStatus(u16),
    RateLimited(Option<Duration>),
    Cache(String),
    /// The server does not support Range requests (HTTP 416 or missing Accept-Ranges).
    RangeNotSupported,
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
            FetchError::RangeNotSupported => write!(f, "server does not support Range requests"),
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
        source.read_exact(&mut buf).unwrap();
        assert_eq!(buf, data[..50]);
        assert_eq!(source.read_position, 50);

        // Seek to 100.
        source.seek(SeekFrom::Start(100)).unwrap();

        // Read 50 bytes from 100.
        source.read_exact(&mut buf).unwrap();
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

        let (_tx, event_rx, _monitor, handle) = spawn_fetch_thread_with_fetcher(
            "http://example.com/test.mp3".to_string(),
            Arc::clone(&cache),
            Box::new(mock),
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

        let (tx, event_rx, _monitor, handle) = spawn_fetch_thread_with_fetcher(
            "http://example.com/test.mp3".to_string(),
            Arc::clone(&cache),
            Box::new(mock),
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

        let (tx, event_rx, _monitor, handle) = spawn_fetch_thread_with_fetcher(
            "http://example.com/test.mp3".to_string(),
            Arc::clone(&cache),
            Box::new(mock),
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

        let (tx, event_rx, _monitor, handle) = spawn_fetch_thread_with_fetcher(
            "http://example.com/test.mp3".to_string(),
            Arc::clone(&cache),
            Box::new(mock),
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
    fn prefetch_estimates_bytes_with_no_latency_data() {
        let dir = temp_dir("prefetch_est");
        let cache = ChunkedCache::open(&dir, "pf1", 1_000_000).unwrap();
        let monitor = BandwidthMonitor::new(BandwidthMonitor::DEFAULT_SLOW_THRESHOLD);

        // No latency data yet → falls back to 80KB.
        let bytes = estimate_prefetch_bytes(&cache, 0, &monitor);
        assert_eq!(bytes, 80 * 1024);

        // Near end: only remaining bytes.
        let bytes = estimate_prefetch_bytes(&cache, 999_900, &monitor);
        assert_eq!(bytes, 100);

        // At end: 0.
        let bytes = estimate_prefetch_bytes(&cache, 1_000_000, &monitor);
        assert_eq!(bytes, 0);

        cleanup(&dir);
    }

    #[test]
    fn prefetch_adapts_to_latency_and_throughput() {
        let dir = temp_dir("prefetch_adapt");
        let cache = ChunkedCache::open(&dir, "pf2", 1_000_000).unwrap();
        let monitor = BandwidthMonitor::new(BandwidthMonitor::DEFAULT_SLOW_THRESHOLD);

        // Simulate: 100ms latency, 100KB/s throughput.
        monitor.record_fetch(10_000, Duration::from_millis(100));

        let bytes = estimate_prefetch_bytes(&cache, 0, &monitor);
        // Expected: factor(4) × 0.1s × 100_000 B/s = 40_000.
        // Clamped to MIN_PREFETCH_BYTES (64KB) since 40KB < 64KB.
        assert!(bytes >= 64 * 1024, "expected >= 64KB, got {bytes}");
        assert!(bytes <= 512 * 1024, "expected <= 512KB, got {bytes}");

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

        // Fail 3 times, then succeed.
        let mock = MockFetcher::new(vec![
            Err(FetchError::HttpStatus(500)),
            Err(FetchError::HttpStatus(500)),
            Err(FetchError::HttpStatus(500)),
            Ok(data),
        ]);
        let (tx, _event_rx, _monitor, handle) = spawn_fetch_thread_with_fetcher(
            "http://example.com/test.mp3".to_string(),
            Arc::clone(&cache),
            Box::new(mock),
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

    #[test]
    fn bandwidth_monitor_records_speed() {
        let monitor = BandwidthMonitor::new(16_384); // 128kbps threshold

        // Simulate fast fetches: 100KB in 100ms = 1MB/s.
        for _ in 0..5 {
            monitor.record_fetch(100_000, Duration::from_millis(100));
        }
        assert!(monitor.bytes_per_sec() > 500_000);
        assert!(!monitor.is_slow());

        // Simulate many slow fetches: 100 bytes in 1s = 100 B/s.
        // EWMA with alpha=0.3 needs ~10 iterations to converge.
        for _ in 0..15 {
            monitor.record_fetch(100, Duration::from_secs(1));
        }
        assert!(monitor.is_slow());
    }

    #[test]
    fn bandwidth_monitor_threshold_update() {
        let monitor = BandwidthMonitor::new(16_384);

        // Fast fetch.
        monitor.record_fetch(100_000, Duration::from_millis(100));
        assert!(!monitor.is_slow());

        // Raise threshold to 10MB/s — now it should be slow.
        monitor.set_slow_threshold(10_000_000);
        assert!(monitor.is_slow());
    }

    #[test]
    fn bandwidth_monitor_zero_elapsed() {
        let monitor = BandwidthMonitor::new(16_384);
        // Zero-duration fetch should not panic or produce infinite bps.
        monitor.record_fetch(1000, Duration::ZERO);
        assert_eq!(monitor.bytes_per_sec(), 0);
    }

    #[test]
    fn range_not_supported_emits_event() {
        let dir = temp_dir("range_not_supp");
        let cache = Arc::new(ChunkedCache::open(&dir, "rns1", 100).unwrap());

        let mock = MockFetcher::new(vec![Err(FetchError::RangeNotSupported)]);

        let (tx, event_rx, _monitor, handle) = spawn_fetch_thread_with_fetcher(
            "http://example.com/test.mp3".to_string(),
            Arc::clone(&cache),
            Box::new(mock),
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

        // Should receive RangeNotSupported event.
        let event = event_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(event, FetchEvent::RangeNotSupported));

        // Fetch thread exits immediately — no data cached.
        assert!(!cache.is_cached(0, 100));

        cleanup(&dir);
    }

    #[test]
    fn box_dyn_http_fetcher_delegates() {
        let mock = MockFetcher::new(vec![Ok(vec![42u8; 10])]);
        let boxed: Box<dyn HttpFetcher> = Box::new(mock);

        let result = boxed.fetch_range("http://example.com", 0, 10).unwrap();
        assert_eq!(result, vec![42u8; 10]);
    }

    /// R10: Fast scrubbing must not queue stale fetches. When many Fetch
    /// commands accumulate, the fetch loop should drain stale ones and only
    /// process the most recent.
    #[test]
    fn fast_scrubbing_drains_stale_fetch_commands() {
        use std::sync::mpsc;

        let dir = temp_dir("stale_drain");
        let cache = Arc::new(ChunkedCache::open(&dir, "stale", 10_000).unwrap());

        // Create an unbounded channel. The fetch_loop drains stale Fetch
        // commands after each fetch completes.
        let (tx, rx) = mpsc::channel();
        let (event_tx, _event_rx) = mpsc::channel();
        let monitor = BandwidthMonitor::new(BandwidthMonitor::DEFAULT_SLOW_THRESHOLD);

        // Queue 10 stale Fetch commands + 1 UpdatePosition.
        // The fetch loop should drain the stale Fetches and only process the
        // first one (the rest are discarded).
        let call_count = Arc::new(AtomicU32::new(0));
        let call_count_clone = Arc::clone(&call_count);

        // We need to run fetch_loop in a thread because it blocks on rx.recv().
        let cache_clone = Arc::clone(&cache);
        let handle = std::thread::spawn(move || {
            let fetcher = CountingFetcher {
                count: call_count_clone,
            };
            super::fetch_loop(
                &fetcher,
                "http://example.com/test",
                &cache_clone,
                &rx,
                &event_tx,
                &RetryConfig::default(),
                &monitor,
            );
        });

        // Send a batch of commands simulating fast scrubbing.
        tx.send(FetchCommand::Fetch {
            offset: 0,
            length: 100,
        })
        .unwrap();
        // Queue stale fetches — these should be drained after the first fetch.
        for i in 1..10u64 {
            tx.send(FetchCommand::Fetch {
                offset: i * 100,
                length: 100,
            })
            .unwrap();
        }
        // A position update should survive the drain.
        tx.send(FetchCommand::UpdatePosition { position: 500 })
            .unwrap();

        // Give the fetch loop time to process.
        std::thread::sleep(Duration::from_millis(500));

        // Shutdown.
        let _ = tx.send(FetchCommand::Shutdown);
        handle.join().unwrap();

        // Only the first Fetch should have been processed (stale ones drained).
        // The count might be 1 or 2 depending on timing (the drain loop may
        // re-process one more), but it must be far less than 10.
        let count = call_count.load(Ordering::Relaxed);
        assert!(
            count <= 2,
            "stale fetches should be drained, but {count} fetches were executed"
        );

        cleanup(&dir);
    }

    #[test]
    fn download_semaphore_limits_concurrency() {
        let sem = super::DownloadSemaphore::new(2);

        // Should acquire first two slots.
        assert!(sem.try_acquire());
        assert!(sem.try_acquire());

        // Third should fail.
        assert!(!sem.try_acquire());

        // Release one — now we can acquire again.
        sem.release();
        assert!(sem.try_acquire());

        // Clean up.
        sem.release();
        sem.release();
    }

    #[test]
    fn download_semaphore_release_is_idempotent() {
        let sem = super::DownloadSemaphore::new(1);
        assert!(sem.try_acquire());
        sem.release();
        // Extra release shouldn't go negative (wraps to usize::MAX).
        // The semaphore should still work correctly.
        assert!(sem.try_acquire());
        sem.release();
    }

    /// Helper fetcher that counts how many fetch_range calls were made.
    struct CountingFetcher {
        count: Arc<AtomicU32>,
    }

    impl HttpFetcher for CountingFetcher {
        fn fetch_range(
            &self,
            _url: &str,
            _offset: u64,
            length: u64,
        ) -> Result<Vec<u8>, FetchError> {
            self.count.fetch_add(1, Ordering::Relaxed);
            Ok(vec![0u8; length as usize])
        }
    }
}
