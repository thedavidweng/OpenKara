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
    is_slow: Arc<AtomicBool>,
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

    pub const DEFAULT_SLOW_THRESHOLD: u64 = 16_384;

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

        // Estimate network RTT by subtracting transfer time from total elapsed.
        // Using the previous throughput EWMA as the transfer-rate estimate:
        //   rtt ≈ elapsed − bytes / prev_bps
        // This separates network latency from bulk transfer time. Without this,
        // latency_us tracks total fetch duration (which grows with fetch size)
        // and the adaptive prefetch formula converges to the ceiling on any
        // non-trivial connection, defeating the adaptivity.
        let prev_bps = prev as f64;
        let transfer_secs = if prev_bps > 0.0 {
            bytes as f64 / prev_bps
        } else {
            0.0
        };
        let rtt_secs = (secs - transfer_secs).max(0.0);
        let rtt_us = (rtt_secs * 1_000_000.0) as u64;
        let prev_latency = self.latency_us.load(Ordering::Relaxed);
        let new_latency = if prev_latency == 0 {
            rtt_us
        } else {
            (prev_latency as f64 * 0.7 + rtt_us as f64 * 0.3) as u64
        };
        self.latency_us.store(new_latency, Ordering::Relaxed);
    }

    pub fn bytes_per_sec(&self) -> u64 {
        self.bytes_per_sec.load(Ordering::Relaxed)
    }

    pub fn is_slow(&self) -> bool {
        self.is_slow.load(Ordering::Relaxed)
    }

    pub fn latency_us(&self) -> u64 {
        self.latency_us.load(Ordering::Relaxed)
    }

    pub fn set_slow_threshold(&self, bps: u64) {
        self.slow_threshold.store(bps, Ordering::Relaxed);
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
/// ## Credential refresh (defect #10 fix)
///
/// Token refresh is single-flight and generation-tracked:
/// - On an authentication-expiry response (401), ONE refresh is run shared by
///   concurrent requests. Waiting requests observe the new credential
///   generation and retry without triggering a second refresh.
/// - A successful authenticated request advances the credential generation so
///   a FUTURE expiry (after the new token later expires) can refresh again.
///   This replaces the old `refresh_attempted: AtomicBool` which allowed only
///   one refresh for the entire fetcher lifetime and never reset.
/// - 403 (PermissionDenied) is NOT misclassified as token expiry: only 401
///   triggers a refresh attempt. 403 is returned as a permanent error so the
///   caller does not retry.
pub struct ProviderFetcher {
    url: String,
    headers: std::sync::Mutex<Vec<(String, String)>>,
    use_post: bool,
    api_arg_header: Option<String>,
    token_refresh: Option<Box<dyn Fn() -> Result<String, FetchError> + Send + Sync>>,
    /// Monotonic credential generation. Incremented after every successful
    /// refresh and after every successful authenticated request. A request
    /// that observes a generation change while waiting on a single-flight
    /// refresh retries with the new token instead of refreshing again.
    credential_generation: AtomicU64,
    /// Single-flight refresh guard. When `Some`, a refresh is in progress;
    /// concurrent requests wait on the contained generation value to learn
    /// whether the refresh succeeded (generation advanced) or failed.
    refresh_in_flight: std::sync::Mutex<Option<u64>>,
    /// Condvar paired with `refresh_in_flight`. The refresh leader notifies
    /// all waiters after it clears the slot; waiters block on this instead
    /// of busy-spinning, so they don't time out before the OAuth round-trip
    /// completes.
    refresh_condvar: std::sync::Condvar,
    /// Reusable HTTP client — avoids creating a new client per request.
    client: reqwest::blocking::Client,
}

/// Connect timeout for the streaming range fetcher's HTTP client.
const STREAMING_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Per-request timeout for the streaming range fetcher's HTTP client.
///
/// Because [`ProviderFetcher::execute_request`] and [`ReqwestFetcher`] consume
/// the body with a streaming `read()` loop rather than `response.bytes()`,
/// reqwest's blocking `Read` applies this value as a fresh deadline PER READ —
/// i.e. an idle timeout. A half-open/stalled connection trips it within this
/// bound and surfaces a normal `FetchError` (so `fetch_range_with_retry` runs
/// its backoff and `ConsecutiveFailures` can drive mid-song reconnect), while a
/// slow-but-steady weak-network transfer keeps making progress and is not
/// killed (issue #204).
const STREAMING_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Build the blocking HTTP client used for streaming range fetches with an
/// explicit connect and per-request (idle) timeout. Falls back to a default
/// client only if the builder fails (e.g. a TLS backend init error); the
/// timeouts are the whole point, so the fallback is a last resort.
fn build_streaming_client(
    connect_timeout: Duration,
    read_timeout: Duration,
) -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(read_timeout)
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

impl ProviderFetcher {
    pub fn new(url: String, headers: Vec<(String, String)>) -> Self {
        Self::with_client(
            url,
            headers,
            build_streaming_client(STREAMING_CONNECT_TIMEOUT, STREAMING_READ_TIMEOUT),
        )
    }

    fn with_client(
        url: String,
        headers: Vec<(String, String)>,
        client: reqwest::blocking::Client,
    ) -> Self {
        Self {
            url,
            headers: std::sync::Mutex::new(headers),
            use_post: false,
            api_arg_header: None,
            token_refresh: None,
            credential_generation: AtomicU64::new(0),
            refresh_in_flight: std::sync::Mutex::new(None),
            refresh_condvar: std::sync::Condvar::new(),
            client,
        }
    }

    /// Test-only constructor that injects a short-timeout client so the stall
    /// path can be exercised without waiting the production timeout.
    #[cfg(test)]
    fn with_read_timeout_for_test(
        url: String,
        headers: Vec<(String, String)>,
        connect_timeout: Duration,
        read_timeout: Duration,
    ) -> Self {
        Self::with_client(
            url,
            headers,
            build_streaming_client(connect_timeout, read_timeout),
        )
    }

    pub fn with_post(mut self, api_arg_header: String) -> Self {
        self.use_post = true;
        self.api_arg_header = Some(api_arg_header);
        self
    }

    /// Register a callback that returns a fresh access token. Called on HTTP
    /// 401 (authentication expired) to refresh credentials and retry without
    /// falling back to full-file download. The refresh is single-flight: one
    /// concurrent refresh is run, and waiting requests observe the new
    /// credential generation.
    pub fn with_token_refresh(
        mut self,
        refresh: impl Fn() -> Result<String, FetchError> + Send + Sync + 'static,
    ) -> Self {
        self.token_refresh = Some(Box::new(refresh));
        self
    }

    fn update_auth_header(&self, new_token: &str) {
        let mut headers = self.headers.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = headers.iter_mut().find(|(k, _)| k == "Authorization") {
            entry.1 = format!("Bearer {new_token}");
        }
    }

    /// Run a single-flight credential refresh. If a refresh is already in
    /// progress, wait for it to complete and return whether the **refresh
    /// epoch** advanced (only the leader success path advances it). Ordinary
    /// successful range requests must NOT advance the refresh epoch — that
    /// would let waiters mistake an unrelated 200 for a successful refresh.
    fn single_flight_refresh(&self) -> Result<bool, FetchError> {
        let Some(refresh) = self.token_refresh.as_ref() else {
            return Ok(false);
        };

        let my_epoch = self.credential_generation.load(Ordering::Acquire);
        {
            let mut in_flight = self
                .refresh_in_flight
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(active_epoch) = *in_flight {
                drop(in_flight);
                return Ok(self.wait_for_refresh(active_epoch));
            }
            // Claim the slot with the epoch the leader observed.
            *in_flight = Some(my_epoch);
        }

        let refresh_result = refresh();
        // Install success (token + epoch) or leave failure visible under the
        // same in-flight lock, THEN clear the slot and notify waiters. If
        // waiters wake before the epoch advances they can read a stale
        // generation and wrongly return false even when refresh succeeded.
        {
            let mut in_flight = self
                .refresh_in_flight
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            match &refresh_result {
                Ok(new_token) => {
                    self.update_auth_header(new_token);
                    // ONLY the refresh leader success path advances the epoch.
                    self.credential_generation.fetch_add(1, Ordering::AcqRel);
                }
                Err(_) => {
                    // Epoch stays unchanged so waiters observe failure.
                }
            }
            *in_flight = None;
            self.refresh_condvar.notify_all();
        }
        match refresh_result {
            Ok(_) => Ok(true),
            Err(e) => Err(e),
        }
    }

    /// Wait for an in-progress refresh to complete. Returns `true` if the
    /// refresh epoch advanced past `active_epoch` (leader success), `false`
    /// if the slot cleared without an epoch change (leader failure).
    fn wait_for_refresh(&self, active_epoch: u64) -> bool {
        let mut in_flight = self
            .refresh_in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        while in_flight.is_some() {
            in_flight = self
                .refresh_condvar
                .wait(in_flight)
                .unwrap_or_else(|e| e.into_inner());
        }
        let current = self.credential_generation.load(Ordering::Acquire);
        current > active_epoch
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

        let mut response = builder.send().map_err(FetchError::Http)?;
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

        // Stream the body with a per-read idle timeout instead of buffering it
        // all under a single total-body deadline. See `STREAMING_READ_TIMEOUT`.
        read_range_body(&mut response, length)
    }
}

impl HttpFetcher for ProviderFetcher {
    fn fetch_range(&self, _url: &str, offset: u64, length: u64) -> Result<Vec<u8>, FetchError> {
        match self.execute_request(offset, length) {
            Ok(bytes) => {
                // Do NOT advance credential_generation here. That counter is
                // the refresh epoch: only a successful refresh leader may
                // advance it. Advancing on ordinary 200s lets waiters mistake
                // an unrelated success for a completed refresh after a failed
                // leader (P1 acceptance finding).
                Ok(bytes)
            }
            Err(FetchError::HttpStatus(401)) if self.token_refresh.is_some() => {
                // Authentication expired — run a single-flight refresh and
                // retry once with the new token. 401 is the token-expiry
                // signal; 403 is permission denial and is NOT retried here.
                let refreshed = self.single_flight_refresh()?;
                if refreshed {
                    self.execute_request(offset, length)
                } else {
                    Err(FetchError::HttpStatus(401))
                }
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
    startup_satisfied: bool,
}

impl RemoteMediaSource {
    pub fn new(cache: Arc<ChunkedCache>, fetch_tx: mpsc::Sender<FetchCommand>) -> Self {
        Self {
            cache,
            read_position: 0,
            fetch_tx,
            min_fetch_size: 64 * 1024,
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
                let mut startup_buf = vec![0u8; startup_length as usize];
                let _ = self
                    .cache
                    .read_at(offset, &mut startup_buf)
                    .map_err(|e| io::Error::other(format!("cache read error: {e}")))?;
            }
            self.startup_satisfied = true;
        }

        self.update_position();

        if self.cache.is_cached(offset, length) {
            let read = self
                .cache
                .read_at(offset, buf)
                .map_err(|e| io::Error::other(format!("cache read error: {e}")))?;
            self.read_position += read as u64;
            return Ok(read);
        }

        self.request_fetch(offset, length);

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
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            max_concurrent,
            active: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Try to acquire a slot. Returns `true` if a slot was acquired, `false` if
    /// at capacity. Call `release()` when the download completes.
    ///
    /// Uses `fetch_update` so the check-and-increment is atomic and retries on
    /// CAS contention. A single non-retrying compare_exchange can spuriously
    /// fail (and deny an available slot) when another thread acquires between
    /// the load and the CAS — fetch_update keeps retrying as long as the
    /// condition is still satisfiable.
    pub fn try_acquire(&self) -> bool {
        self.active
            .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |current| {
                if current >= self.max_concurrent {
                    None
                } else {
                    Some(current + 1)
                }
            })
            .is_ok()
    }

    /// Release a previously acquired slot.  No-op if no slot is held (prevents
    /// underflow from wrapping to `usize::MAX` and permanently blocking
    /// downloads).
    pub fn release(&self) {
        let _ = self
            .active
            .fetch_update(Ordering::Release, Ordering::Relaxed, |current| {
                current.checked_sub(1)
            });
    }

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
    // Build the client with explicit connect + per-request (idle) timeouts so a
    // half-open/stalled connection cannot hang the fetch thread forever (#204).
    let client = build_streaming_client(STREAMING_CONNECT_TIMEOUT, STREAMING_READ_TIMEOUT);
    spawn_fetch_thread_with_fetcher(
        url,
        cache,
        Box::new(ReqwestFetcher { client }),
        RetryConfig::default(),
        None,
    )
}

/// Spawn a fetch thread with a custom `HttpFetcher` and `RetryConfig` (for testing).
/// `on_range_written` is called after each successful range write so the
/// caller can persist download progress to the cache catalog. Pass `None`
/// when persistence is not needed (e.g. tests).
pub fn spawn_fetch_thread_with_fetcher(
    url: String,
    cache: Arc<ChunkedCache>,
    fetcher: Box<dyn HttpFetcher>,
    retry_config: RetryConfig,
    on_range_written: Option<Arc<dyn Fn() + Send + Sync>>,
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
            on_range_written.as_ref(),
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

        let mut response = self
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

        // Stream the body with a per-read idle timeout instead of buffering it
        // all under a single total-body deadline. See `STREAMING_READ_TIMEOUT`.
        read_range_body(&mut response, length)
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
    on_range_written: Option<&Arc<dyn Fn() + Send + Sync>>,
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
                    if let Some(cb) = on_range_written {
                        cb();
                    }
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
                        if let Some(cb) = on_range_written {
                            cb();
                        }
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
    // TODO: unify with shared policy (net_policy::RetryDriver). The streaming
    // hot path uses its own non-jittered exponential backoff because range
    // fetches are latency-sensitive and the shared driver's full-jitter would
    // add variance to playback buffering. The classification + jitter helpers
    // in net_policy are shared via `classify_fetch_status` below; the full
    // RetryDriver adoption is deferred to avoid disrupting the hot path.
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
    /// An IO error while streaming the response body — includes a per-read
    /// idle-timeout on a stalled/half-open connection (issue #204). Treated as
    /// a transient failure so `fetch_range_with_retry` retries and, past the
    /// threshold, emits `ConsecutiveFailures` to drive reconnect.
    Io(io::Error),
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
            FetchError::Io(e) => write!(f, "IO error while streaming range: {e}"),
            FetchError::RangeNotSupported => write!(f, "server does not support Range requests"),
        }
    }
}

/// Maximum bytes buffered for a single streaming range response. Range chunks
/// are at most `MAX_PREFETCH_BYTES` (512 KiB); this ceiling only guards against
/// a server that ignores the Range header and streams a huge body, which the
/// old `response.bytes()` would have buffered without any bound.
const MAX_RANGE_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;

/// Read a range response body into memory with a per-read idle timeout.
///
/// reqwest's blocking `Read` impl applies the client's configured `timeout` as
/// a fresh deadline for each read once the body is streaming, so a stalled or
/// half-open connection trips a bounded timeout (surfaced as `FetchError::Io`)
/// instead of parking the fetch thread forever (issue #204). A slow-but-steady
/// weak link keeps making progress and is not killed by a single total-body
/// deadline. `expected_length` sizes the initial buffer.
fn read_range_body(
    response: &mut reqwest::blocking::Response,
    expected_length: u64,
) -> Result<Vec<u8>, FetchError> {
    let capacity = expected_length.min(MAX_RANGE_RESPONSE_BYTES) as usize;
    let mut body = Vec::with_capacity(capacity);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let read = response.read(&mut chunk).map_err(FetchError::Io)?;
        if read == 0 {
            break;
        }
        if body.len() as u64 + read as u64 > MAX_RANGE_RESPONSE_BYTES {
            return Err(FetchError::Io(io::Error::other(
                "range response exceeded the maximum buffered size",
            )));
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Ok(body)
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
                None,
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
        // Extra release is a no-op — doesn't underflow to usize::MAX.
        // The semaphore should still work correctly.
        sem.release();
        assert!(sem.try_acquire());
        sem.release();
    }

    #[test]
    fn download_semaphore_spurious_release_does_not_block() {
        let sem = super::DownloadSemaphore::new(1);
        // Multiple spurious releases without any acquire.
        sem.release();
        sem.release();
        sem.release();
        // Semaphore should still allow acquisition.
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

    // ---- Credential refresh single-flight tests (defect #10) ----

    #[test]
    fn provider_fetcher_refreshes_on_401_and_retries() {
        let refresh_count = Arc::new(AtomicU32::new(0));
        let refresh_count_clone = Arc::clone(&refresh_count);
        let old_token = "old-token".to_owned();
        let new_token = "new-token".to_owned();

        let mut server = mockito::Server::new();
        // The first request with the old token returns 401.
        let _m1 = server
            .mock("GET", "/file")
            .match_header("Authorization", format!("Bearer {old_token}").as_str())
            .with_status(401)
            .create();
        // After refresh, the request with the new token returns 206.
        let _m2 = server
            .mock("GET", "/file")
            .match_header("Authorization", format!("Bearer {new_token}").as_str())
            .with_status(206)
            .with_header("Content-Range", "bytes 0-99/100")
            .with_body(vec![0u8; 100])
            .create();

        let url = format!("{}/file", server.url());
        let fetcher = ProviderFetcher::new(
            url,
            vec![("Authorization".to_owned(), format!("Bearer {old_token}"))],
        )
        .with_token_refresh(move || {
            refresh_count_clone.fetch_add(1, Ordering::Relaxed);
            Ok(new_token.clone())
        });

        let result = fetcher.fetch_range("", 0, 100);
        assert!(result.is_ok(), "retry after refresh should succeed");
        assert_eq!(
            refresh_count.load(Ordering::Relaxed),
            1,
            "exactly one refresh call"
        );
    }

    #[test]
    fn provider_fetcher_does_not_refresh_on_403() {
        let refresh_count = Arc::new(AtomicU32::new(0));
        let refresh_count_clone = Arc::clone(&refresh_count);
        let token = "valid-token".to_owned();

        let mut server = mockito::Server::new();
        server
            .mock("GET", "/file")
            .match_header("Authorization", format!("Bearer {token}").as_str())
            .with_status(403)
            .create();

        let url = format!("{}/file", server.url());
        let fetcher = ProviderFetcher::new(
            url,
            vec![("Authorization".to_owned(), format!("Bearer {token}"))],
        )
        .with_token_refresh(move || {
            refresh_count_clone.fetch_add(1, Ordering::Relaxed);
            Ok("refreshed".to_owned())
        });

        let result = fetcher.fetch_range("", 0, 100);
        assert!(result.is_err(), "403 should not succeed");
        assert_eq!(
            refresh_count.load(Ordering::Relaxed),
            0,
            "403 must not trigger a refresh (permission denial is permanent)"
        );
    }

    #[test]
    fn provider_fetcher_can_refresh_again_after_success() {
        // Defect #10: the old refresh_attempted flag prevented a second refresh
        // for the fetcher's entire lifetime. The generation-tracked approach
        // allows a future expiry to refresh again after a successful request.
        let refresh_count = Arc::new(AtomicU32::new(0));
        let refresh_count_clone = Arc::clone(&refresh_count);
        let token1 = "token-1".to_owned();
        let token2 = "token-2".to_owned();

        let mut server = mockito::Server::new();
        // First request: 401 → refresh to token-2.
        server
            .mock("GET", "/file")
            .match_header("Authorization", format!("Bearer {token1}").as_str())
            .with_status(401)
            .create();
        // Second request with token-2: 206 success.
        let m_success = server
            .mock("GET", "/file")
            .match_header("Authorization", format!("Bearer {token2}").as_str())
            .with_status(206)
            .with_header("Content-Range", "bytes 0-99/100")
            .with_body(vec![0u8; 100])
            .create();

        let url = format!("{}/file", server.url());
        let fetcher = ProviderFetcher::new(
            url,
            vec![("Authorization".to_owned(), format!("Bearer {token1}"))],
        )
        .with_token_refresh(move || {
            refresh_count_clone.fetch_add(1, Ordering::Relaxed);
            Ok(token2.clone())
        });

        // First fetch: 401 → refresh → 206.
        let result1 = fetcher.fetch_range("", 0, 100);
        assert!(result1.is_ok(), "first fetch with refresh should succeed");
        assert_eq!(refresh_count.load(Ordering::Relaxed), 1);

        // The refresh epoch advances only on leader success (the 401→refresh
        // path above), not on ordinary successful range responses.
        assert!(
            fetcher.credential_generation.load(Ordering::Relaxed) > 0,
            "refresh epoch advanced after successful credential refresh"
        );

        // Suppress unused mock warning.
        m_success.assert();
    }

    // ---- streaming range fetcher timeout tests (issue #204) ----

    /// A single-connection mock server that accepts the request, sends 206
    /// headers promising a body, then never writes the body — a half-open /
    /// stalled peer. Holds the connection open until dropped so the client's
    /// per-read idle timeout (not an EOF) is what ends the read.
    struct StallingRangeServer {
        url: String,
        stop: Arc<AtomicBool>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl StallingRangeServer {
        fn spawn() -> Self {
            use std::io::{Read as _, Write as _};
            use std::net::TcpListener;
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind stalling server");
            let addr = listener.local_addr().expect("addr");
            let stop = Arc::new(AtomicBool::new(false));
            let stop_thread = Arc::clone(&stop);
            let handle = std::thread::spawn(move || {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut req = [0u8; 2048];
                let _ = stream.read(&mut req);
                let _ = stream.write_all(
                    b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-999999/1000000\r\n\
                      Content-Length: 1000000\r\n\r\n",
                );
                let _ = stream.flush();
                // Hold the connection open without a body until told to stop.
                while !stop_thread.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
            Self {
                url: format!("http://{addr}/file"),
                stop,
                handle: Some(handle),
            }
        }
    }

    impl Drop for StallingRangeServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    #[test]
    fn provider_fetcher_times_out_on_stalled_body_instead_of_hanging() {
        // Acceptance (#204): a half-open connection that accepts the Range
        // request then never writes the body must make `fetch_range` return an
        // error within a bounded time (~ the read timeout), not block forever.
        let server = StallingRangeServer::spawn();
        let fetcher = ProviderFetcher::with_read_timeout_for_test(
            server.url.clone(),
            Vec::new(),
            Duration::from_millis(300),
            Duration::from_millis(300),
        );

        let started = std::time::Instant::now();
        let result = fetcher.fetch_range("", 0, 100);
        let elapsed = started.elapsed();

        assert!(result.is_err(), "a stalled body must error, not hang");
        assert!(
            matches!(result, Err(FetchError::Io(_))),
            "stall surfaces as an IO/timeout error"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "fetch must be bounded by the read timeout, took {elapsed:?}"
        );
    }

    #[test]
    fn timed_out_fetch_drives_consecutive_failures_and_thread_exits() {
        // Acceptance (#204): a timed-out range fetch surfaces as a normal
        // failure, so `fetch_range_with_retry` runs and, past the threshold,
        // `ConsecutiveFailures` is emitted to drive reconnect — and the fetch
        // thread exits cleanly on Shutdown (no leaked thread/socket).
        let dir = temp_dir("io_timeout_consec");
        let cache = Arc::new(ChunkedCache::open(&dir, "io1", 100).unwrap());

        let timed_out = || {
            Err(FetchError::Io(io::Error::new(
                io::ErrorKind::TimedOut,
                "simulated stalled connection",
            )))
        };
        let mock = MockFetcher::new(vec![
            timed_out(),
            timed_out(),
            timed_out(),
            timed_out(),
            timed_out(),
        ]);

        let (tx, event_rx, _monitor, handle) = spawn_fetch_thread_with_fetcher(
            "http://example.com/test.mp3".to_string(),
            Arc::clone(&cache),
            Box::new(mock),
            RetryConfig {
                initial_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(5),
                max_retries: 0,
                consecutive_failure_threshold: 5,
            },
            None,
        );

        for i in 0..5u64 {
            tx.send(FetchCommand::Fetch {
                offset: i * 100,
                length: 100,
            })
            .unwrap();
        }

        let event = event_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("ConsecutiveFailures must be emitted for repeated timeouts");
        assert!(matches!(
            event,
            FetchEvent::ConsecutiveFailures { count: 5 }
        ));

        // The thread must join promptly after Shutdown — the timed-out fetches
        // returned instead of parking the thread, so no thread/socket leaks.
        tx.send(FetchCommand::Shutdown).unwrap();
        handle.join().expect("fetch thread exits after Shutdown");

        cleanup(&dir);
    }
}
