use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::error::{kind_from_http_status_code, kind_from_io_error, RemoteError, RemoteErrorKind};

/// Maximum `Retry-After` value honored (seconds). Unbounded values are capped
/// so a misbehaving server cannot stall an operation indefinitely.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_ATTEMPT_DEADLINE: Duration = Duration::from_secs(120);
const DEFAULT_MAX_RETRIES: u32 = 4;
const DEFAULT_INITIAL_DELAY: Duration = Duration::from_secs(1);
const DEFAULT_MAX_DELAY: Duration = Duration::from_secs(30);

pub trait JitterRng: Send + Sync {
    fn next_bounded(&self, max: u64) -> u64;
}

pub struct SeededJitter {
    state: Mutex<u64>,
}

impl SeededJitter {
    pub fn new(seed: u64) -> Self {
        Self {
            state: Mutex::new(seed.max(1)),
        }
    }
}

impl JitterRng for SeededJitter {
    fn next_bounded(&self, max: u64) -> u64 {
        if max == 0 {
            return 0;
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x % max
    }
}

pub struct ThreadJitter;

impl JitterRng for ThreadJitter {
    fn next_bounded(&self, max: u64) -> u64 {
        if max == 0 {
            return 0;
        }
        use rand::RngExt;
        rand::rng().random_range(0..max)
    }
}

#[derive(Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub connect_timeout: Duration,
    /// Reqwest blocking uses a single per-request deadline, so production
    /// clients apply `max(read_timeout, attempt_deadline)`.
    pub read_timeout: Duration,
    pub attempt_deadline: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            initial_delay: DEFAULT_INITIAL_DELAY,
            max_delay: DEFAULT_MAX_DELAY,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            read_timeout: DEFAULT_READ_TIMEOUT,
            attempt_deadline: DEFAULT_ATTEMPT_DEADLINE,
        }
    }
}

pub enum AttemptOutcome<T> {
    Ok(T),
    Err(RemoteError),
}

pub fn classify_reqwest_error(error: &reqwest::Error) -> RemoteErrorKind {
    use std::error::Error;
    if error.is_connect() || error.is_timeout() {
        return RemoteErrorKind::NetworkUnavailable;
    }
    if let Some(source) = error.source() {
        if let Some(io) = source.downcast_ref::<std::io::Error>() {
            let kind = kind_from_io_error(io);
            if kind == RemoteErrorKind::DiskFull {
                return kind;
            }
            return RemoteErrorKind::NetworkUnavailable;
        }
    }
    RemoteErrorKind::NetworkUnavailable
}

pub fn classify_status(status: reqwest::StatusCode) -> RemoteErrorKind {
    kind_from_http_status_code(status.as_u16())
}

/// Full jitter (`uniform(0, min(max_delay, initial_delay * 2^attempt))`)
/// spreads concurrent retriers across the backoff window.
pub fn full_jitter_delay(policy: &RetryPolicy, attempt: u32, rng: &dyn JitterRng) -> Duration {
    let cap = cap_for_attempt(policy, attempt);
    if cap.is_zero() {
        return Duration::ZERO;
    }
    let millis = cap.as_millis() as u64;
    Duration::from_millis(rng.next_bounded(millis))
}

fn cap_for_attempt(policy: &RetryPolicy, attempt: u32) -> Duration {
    let mut cap = policy.initial_delay;
    for _ in 0..attempt {
        cap = cap.saturating_mul(2);
        if cap >= policy.max_delay {
            return policy.max_delay;
        }
    }
    cap
}

pub fn parse_retry_after(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    if let Ok(secs) = trimmed.parse::<u64>() {
        if secs > MAX_RETRY_AFTER.as_secs() {
            return Some(MAX_RETRY_AFTER);
        }
        return Some(Duration::from_secs(secs));
    }
    if let Ok(parsed) = httpdate::parse_http_date(trimmed) {
        let now = SystemTime::now();
        if let Ok(delta) = parsed.duration_since(now) {
            if delta > MAX_RETRY_AFTER {
                return Some(MAX_RETRY_AFTER);
            }
            return Some(delta);
        }
    }
    None
}

pub trait RetryProgress: Send + Sync {
    fn record_attempt(&self, attempt_count: i64, next_attempt_at_ms: Option<i64>);
}

pub struct NoopProgress;

impl RetryProgress for NoopProgress {
    fn record_attempt(&self, _attempt_count: i64, _next_attempt_at_ms: Option<i64>) {}
}

pub struct RetryDriver<'a> {
    pub policy: &'a RetryPolicy,
    pub rng: &'a dyn JitterRng,
    pub cancel: Option<&'a Arc<AtomicBool>>,
    pub progress: Option<&'a dyn RetryProgress>,
    pub sleep_fn: &'a dyn Fn(Duration),
    pub now_ms: &'a dyn Fn() -> i64,
}

impl<'a> RetryDriver<'a> {
    pub fn run<T, F>(&self, mut operation_fn: F) -> Result<T, RemoteError>
    where
        F: FnMut() -> AttemptOutcome<T>,
    {
        let mut attempt: u32 = 0;
        loop {
            if self.is_cancelled() {
                return Err(RemoteError::from_kind(RemoteErrorKind::OperationCancelled));
            }

            let outcome = operation_fn();
            match outcome {
                AttemptOutcome::Ok(value) => return Ok(value),
                AttemptOutcome::Err(err) => {
                    if !err.retryable {
                        return Err(err);
                    }
                    if attempt >= self.policy.max_retries {
                        if let Some(progress) = self.progress {
                            progress.record_attempt(attempt as i64 + 1, Some((self.now_ms)()));
                        }
                        return Err(err);
                    }

                    let delay = err
                        .retry_after
                        .unwrap_or_else(|| full_jitter_delay(self.policy, attempt, self.rng));

                    if let Some(progress) = self.progress {
                        let next_ms = (self.now_ms)() + delay.as_millis() as i64;
                        progress.record_attempt(attempt as i64 + 1, Some(next_ms));
                    }

                    if !delay.is_zero() {
                        (self.sleep_fn)(delay);
                    }
                    if self.is_cancelled() {
                        return Err(RemoteError::from_kind(RemoteErrorKind::OperationCancelled));
                    }
                    attempt += 1;
                }
            }
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.is_some_and(|flag| flag.load(Ordering::Relaxed))
    }
}

pub fn production_sleep(d: Duration) {
    std::thread::sleep(d);
}

pub fn build_http_client(policy: &RetryPolicy) -> Result<reqwest::blocking::Client, String> {
    let request_timeout = policy.read_timeout.max(policy.attempt_deadline);
    reqwest::blocking::Client::builder()
        .connect_timeout(policy.connect_timeout)
        .timeout(request_timeout)
        .build()
        .map_err(|error| format!("failed to build remote HTTP client: {error}"))
}

pub fn shared_http_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        build_http_client(&RetryPolicy::default())
            .expect("default remote HTTP client must construct")
    })
}

const DOWNLOAD_STREAM_BUFFER: usize = 64 * 1024;
const DOWNLOAD_STREAM_TOTAL_BUDGET: Duration = Duration::from_secs(60 * 60);

/// Reading in chunks makes reqwest's blocking `Read` apply the client
/// `timeout` per read rather than across the whole body (issue #205), and
/// leaves received bytes in `writer` if the transfer drops mid-stream.
pub fn stream_response_body_capped<W: Write + ?Sized>(
    response: &mut reqwest::blocking::Response,
    writer: &mut W,
    max_bytes: u64,
) -> std::io::Result<u64> {
    let started = Instant::now();
    let mut total: u64 = 0;
    let mut buffer = [0u8; DOWNLOAD_STREAM_BUFFER];
    loop {
        if started.elapsed() > DOWNLOAD_STREAM_TOTAL_BUDGET {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "download exceeded the total time budget",
            ));
        }
        let read = response.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > max_bytes {
            return Err(std::io::Error::other(format!(
                "download body exceeded the maximum of {max_bytes} bytes"
            )));
        }
        writer.write_all(&buffer[..read])?;
    }
    Ok(total)
}

pub fn stream_response_body<W: Write + ?Sized>(
    response: &mut reqwest::blocking::Response,
    writer: &mut W,
) -> std::io::Result<u64> {
    stream_response_body_capped(response, writer, u64::MAX)
}

pub fn run_with_default_retry<T, F>(operation: F) -> Result<T, RemoteError>
where
    F: FnMut() -> AttemptOutcome<T>,
{
    let policy = RetryPolicy::default();
    let rng = ThreadJitter;
    let progress = NoopProgress;
    let driver = RetryDriver {
        policy: &policy,
        rng: &rng,
        cancel: None,
        progress: Some(&progress),
        sleep_fn: &production_sleep,
        now_ms: &current_unix_time_ms,
    };
    driver.run(operation)
}

pub fn remote_error_with_retry_after(
    kind: RemoteErrorKind,
    detail: impl Into<String>,
    retry_after: Option<Duration>,
) -> RemoteError {
    let mut err = RemoteError::new(kind, detail);
    err.retry_after = retry_after;
    err
}

fn current_unix_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::atomic::Ordering as AtomicOrdering;

    fn fixed_now(ms: i64) -> Arc<Mutex<i64>> {
        Arc::new(Mutex::new(ms))
    }

    fn now_fn(clock: Arc<Mutex<i64>>) -> Box<dyn Fn() -> i64 + Send + Sync> {
        Box::new(move || {
            let mut current = clock.lock().unwrap();
            let v = *current;
            *current += 1000;
            v
        })
    }

    #[test]
    fn full_jitter_is_within_bounds_and_deterministic() {
        let policy = RetryPolicy {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            ..RetryPolicy::default()
        };
        let rng = SeededJitter::new(42);
        let d0 = full_jitter_delay(&policy, 0, &rng);
        assert!(d0 <= Duration::from_secs(1));
        let d3 = full_jitter_delay(&policy, 3, &rng);
        assert!(d3 <= Duration::from_secs(8));
        let rng2 = SeededJitter::new(42);
        let d0b = full_jitter_delay(&policy, 0, &rng2);
        assert_eq!(d0, d0b);
    }

    #[test]
    fn cap_for_attempt_doubles_until_max() {
        let policy = RetryPolicy {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            ..RetryPolicy::default()
        };
        assert_eq!(cap_for_attempt(&policy, 0), Duration::from_secs(1));
        assert_eq!(cap_for_attempt(&policy, 1), Duration::from_secs(2));
        assert_eq!(cap_for_attempt(&policy, 2), Duration::from_secs(4));
        assert_eq!(cap_for_attempt(&policy, 5), Duration::from_secs(30));
    }

    #[test]
    fn parse_retry_after_seconds_capped() {
        assert_eq!(parse_retry_after("5"), Some(Duration::from_secs(5)));
        assert_eq!(parse_retry_after("120"), Some(MAX_RETRY_AFTER));
        assert!(parse_retry_after("not-a-number").is_none());
    }

    #[test]
    fn driver_retries_then_succeeds() {
        let policy = RetryPolicy {
            max_retries: 3,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(100),
            ..RetryPolicy::default()
        };
        let rng = SeededJitter::new(1);
        let sleep_calls = Mutex::new(Vec::new());
        let sleep = |d: Duration| {
            sleep_calls.lock().unwrap().push(d);
        };
        let clock = fixed_now(1000);
        let now = now_fn(clock);
        let driver = RetryDriver {
            policy: &policy,
            rng: &rng,
            cancel: None,
            progress: None,
            sleep_fn: &sleep,
            now_ms: &*now,
        };

        let mut calls = 0;
        let result: Result<i32, RemoteError> = driver.run(|| {
            calls += 1;
            if calls < 3 {
                AttemptOutcome::Err(RemoteError::from_kind(RemoteErrorKind::NetworkUnavailable))
            } else {
                AttemptOutcome::Ok(42)
            }
        });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls, 3);
        assert_eq!(sleep_calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn driver_does_not_retry_permanent_error() {
        let policy = RetryPolicy::default();
        let rng = SeededJitter::new(1);
        let sleep = |_| {};
        let clock = fixed_now(1000);
        let now = now_fn(clock);
        let driver = RetryDriver {
            policy: &policy,
            rng: &rng,
            cancel: None,
            progress: None,
            sleep_fn: &sleep,
            now_ms: &*now,
        };
        let mut calls = 0;
        let result: Result<i32, RemoteError> = driver.run(|| {
            calls += 1;
            AttemptOutcome::Err(RemoteError::from_kind(RemoteErrorKind::PermissionDenied))
        });
        assert_eq!(calls, 1);
        let err = result.unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::PermissionDenied);
        assert!(!err.retryable);
    }

    #[test]
    fn driver_aborts_on_cancel() {
        let policy = RetryPolicy {
            max_retries: 5,
            initial_delay: Duration::from_millis(10),
            ..RetryPolicy::default()
        };
        let rng = SeededJitter::new(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_ref = cancel.clone();
        let sleep = move |_d: Duration| {
            cancel_ref.store(true, Ordering::Relaxed);
        };
        let clock = fixed_now(1000);
        let now = now_fn(clock);
        let driver = RetryDriver {
            policy: &policy,
            rng: &rng,
            cancel: Some(&cancel),
            progress: None,
            sleep_fn: &sleep,
            now_ms: &*now,
        };
        let mut calls = 0;
        let result: Result<i32, RemoteError> = driver.run(|| {
            calls += 1;
            AttemptOutcome::Err(RemoteError::from_kind(RemoteErrorKind::NetworkUnavailable))
        });
        assert_eq!(calls, 1);
        let err = result.unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::OperationCancelled);
    }

    #[test]
    fn driver_persists_attempt_progress() {
        let policy = RetryPolicy {
            max_retries: 2,
            initial_delay: Duration::from_millis(10),
            ..RetryPolicy::default()
        };
        let rng = SeededJitter::new(1);
        let sleep = |_d: Duration| {};
        let clock = fixed_now(5000);
        let now = now_fn(clock);
        let recorded = Mutex::new(Vec::new());
        struct Recorder<'a>(&'a Mutex<Vec<(i64, Option<i64>)>>);
        impl RetryProgress for Recorder<'_> {
            fn record_attempt(&self, count: i64, next: Option<i64>) {
                self.0.lock().unwrap().push((count, next));
            }
        }
        let recorder = Recorder(&recorded);
        let driver = RetryDriver {
            policy: &policy,
            rng: &rng,
            cancel: None,
            progress: Some(&recorder),
            sleep_fn: &sleep,
            now_ms: &*now,
        };
        let mut calls = 0;
        let _result: Result<i32, RemoteError> = driver.run(|| {
            calls += 1;
            AttemptOutcome::Err(RemoteError::from_kind(RemoteErrorKind::NetworkUnavailable))
        });
        let recorded = recorded.lock().unwrap();
        assert_eq!(recorded.len(), 3);
        assert!(recorded[0].1.unwrap() > 5000);
    }

    #[test]
    fn driver_honors_retry_after_header() {
        let policy = RetryPolicy {
            max_retries: 2,
            initial_delay: Duration::from_millis(1),
            ..RetryPolicy::default()
        };
        let rng = SeededJitter::new(1);
        let sleep_calls = Mutex::new(Vec::new());
        let sleep = |d: Duration| {
            sleep_calls.lock().unwrap().push(d);
        };
        let clock = fixed_now(1000);
        let now = now_fn(clock);
        let driver = RetryDriver {
            policy: &policy,
            rng: &rng,
            cancel: None,
            progress: None,
            sleep_fn: &sleep,
            now_ms: &*now,
        };
        let mut calls = 0;
        let result: Result<i32, RemoteError> = driver.run(|| {
            calls += 1;
            if calls == 1 {
                AttemptOutcome::Err(remote_error_with_retry_after(
                    RemoteErrorKind::RateLimited,
                    "rate limited",
                    Some(Duration::from_millis(50)),
                ))
            } else {
                AttemptOutcome::Ok(7)
            }
        });
        assert_eq!(result.unwrap(), 7);
        assert_eq!(sleep_calls.lock().unwrap()[0], Duration::from_millis(50));
    }

    #[test]
    fn build_http_client_applies_policy_timeouts() {
        let policy = RetryPolicy {
            connect_timeout: Duration::from_secs(3),
            read_timeout: Duration::from_secs(5),
            attempt_deadline: Duration::from_secs(10),
            ..RetryPolicy::default()
        };
        let client = build_http_client(&policy).expect("client builds");
        drop(client);
        let shared = shared_http_client();
        assert!(std::ptr::eq(shared, shared_http_client()));
    }

    #[test]
    fn classify_status_maps_correctly() {
        assert_eq!(
            classify_status(reqwest::StatusCode::TOO_MANY_REQUESTS),
            RemoteErrorKind::RateLimited
        );
        assert_eq!(
            classify_status(reqwest::StatusCode::FORBIDDEN),
            RemoteErrorKind::PermissionDenied
        );
        assert_eq!(
            classify_status(reqwest::StatusCode::CONFLICT),
            RemoteErrorKind::RemoteConflict
        );
        assert_eq!(
            classify_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR),
            RemoteErrorKind::NetworkUnavailable
        );
    }

    struct MockBodyServer {
        url: String,
        stop: Arc<AtomicBool>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl MockBodyServer {
        fn spawn(
            content_length: usize,
            first_body: Vec<u8>,
            trailing_body: Vec<u8>,
            chunk_size: usize,
            gap: Duration,
            stall: bool,
        ) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
            let addr = listener.local_addr().expect("addr");
            let stop = Arc::new(AtomicBool::new(false));
            let stop_thread = Arc::clone(&stop);
            let handle = std::thread::spawn(move || {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut req = [0u8; 2048];
                let _ = stream.read(&mut req);
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {content_length}\r\nAccept-Ranges: bytes\r\n\r\n"
                );
                if stream.write_all(headers.as_bytes()).is_err() {
                    return;
                }
                let _ = stream.write_all(&first_body);
                let _ = stream.flush();
                if stall {
                    while !stop_thread.load(AtomicOrdering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    return;
                }
                for piece in trailing_body.chunks(chunk_size.max(1)) {
                    std::thread::sleep(gap);
                    if stream.write_all(piece).is_err() {
                        return;
                    }
                    let _ = stream.flush();
                }
                while !stop_thread.load(AtomicOrdering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
            Self {
                url: format!("http://{addr}/body"),
                stop,
                handle: Some(handle),
            }
        }
    }

    impl Drop for MockBodyServer {
        fn drop(&mut self) {
            self.stop.store(true, AtomicOrdering::Relaxed);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn short_timeout_client(read_timeout: Duration) -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(read_timeout)
            .build()
            .expect("client builds")
    }

    #[test]
    fn stream_response_body_succeeds_on_slow_but_steady_trickle() {
        let total = 128 * 1024usize;
        let body: Vec<u8> = (0..total).map(|i| (i % 251) as u8).collect();
        let read_timeout = Duration::from_millis(300);
        let server = MockBodyServer::spawn(
            total,
            Vec::new(),
            body.clone(),
            16 * 1024,
            Duration::from_millis(120),
            false,
        );
        let client = short_timeout_client(read_timeout);
        let mut response = client.get(&server.url).send().expect("send");

        let started = Instant::now();
        let mut sink: Vec<u8> = Vec::new();
        let written = stream_response_body(&mut response, &mut sink).expect("stream succeeds");
        let elapsed = started.elapsed();

        assert_eq!(written, total as u64);
        assert_eq!(sink, body);
        assert!(
            elapsed > read_timeout,
            "transfer ({elapsed:?}) should exceed the client timeout ({read_timeout:?})"
        );
    }

    #[test]
    fn stream_response_body_times_out_on_stall_and_keeps_partial() {
        let prefix = vec![7u8; 4096];
        let server = MockBodyServer::spawn(
            1_000_000,
            prefix.clone(),
            Vec::new(),
            0,
            Duration::ZERO,
            true,
        );
        let client = short_timeout_client(Duration::from_millis(250));
        let mut response = client.get(&server.url).send().expect("send");

        let started = Instant::now();
        let mut sink: Vec<u8> = Vec::new();
        let result = stream_response_body(&mut response, &mut sink);
        let elapsed = started.elapsed();

        assert!(result.is_err(), "a stalled body must error, not hang");
        assert!(
            elapsed < Duration::from_secs(5),
            "stall must be bounded, took {elapsed:?}"
        );
        assert_eq!(sink, prefix, "bytes received before the stall are durable");
    }

    #[test]
    fn stream_response_body_capped_rejects_oversized_body() {
        let body = vec![1u8; 4096];
        let server = MockBodyServer::spawn(
            body.len(),
            body.clone(),
            Vec::new(),
            0,
            Duration::ZERO,
            false,
        );
        let client = short_timeout_client(Duration::from_secs(2));
        let mut response = client.get(&server.url).send().expect("send");
        let mut sink: Vec<u8> = Vec::new();
        let result = stream_response_body_capped(&mut response, &mut sink, 1024);
        assert!(result.is_err(), "body over the cap must error");
    }
}
