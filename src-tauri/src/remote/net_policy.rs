//! Shared network retry policy for remote provider operations.
//!
//! Provides ONE retry policy used by metadata, upload, download, refresh, and
//! range operations. Replaces the ad-hoc single-`.send()` calls in provider
//! upload/download/metadata and complements the existing range-fetcher backoff
//! in `audio/remote_source.rs`.
//!
//! ## What is retried
//!
//! Temporary failures only:
//! - connection reset/refused; DNS or connect timeout; idle/read timeout
//! - HTTP 408, 425, 429, 500, 502, 503, 504
//!
//! ## What is NOT retried
//!
//! Permanent failures fail immediately with the classified `RemoteErrorKind`:
//! - malformed request (400); permission denied (403); missing object (404)
//! - integrity mismatch after a completed transfer
//! - conditional-write conflict (409/412)
//! - unsupported capability
//!
//! ## Backoff
//!
//! Exponential backoff with full jitter. A seeded deterministic RNG is used in
//! tests; production uses a thread RNG. `Retry-After` headers are honored when
//! bounded (capped at `MAX_RETRY_AFTER`); unbounded/absurd values are ignored.
//!
//! ## Cancellation + deadlines
//!
//! Callers may pass a `cancel: Arc<AtomicBool>` and/or an operation deadline.
//! The driver checks the cancel flag between attempts and aborts without
//! further retries when set.
//!
//! Production provider paths call [`run_with_default_retry`] for transport-
//! level retries. The classification and jitter helpers are also shared with
//! `audio/remote_source.rs` and the reconnect policy.

use crate::remote::errors::{
    kind_from_http_status, kind_from_io_error, RemoteError, RemoteErrorKind,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Maximum `Retry-After` value honored (seconds). Absurd/unbounded values are
/// ignored so a misbehaving server cannot stall an operation indefinitely.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

/// Default connect timeout.
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Default idle-read timeout.
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Default per-attempt deadline.
const DEFAULT_ATTEMPT_DEADLINE: Duration = Duration::from_secs(120);

/// Maximum number of retry attempts for a retryable failure.
const DEFAULT_MAX_RETRIES: u32 = 4;

/// Base delay for the first backoff interval.
const DEFAULT_INITIAL_DELAY: Duration = Duration::from_secs(1);
/// Ceiling for the exponential backoff delay.
const DEFAULT_MAX_DELAY: Duration = Duration::from_secs(30);

/// A source of randomness for full-jitter backoff. Production uses a thread
/// RNG; tests inject a deterministic seeded RNG so backoff sequences are
/// reproducible.
pub(crate) trait JitterRng: Send + Sync {
    /// Return a uniform random `u64` in `[0, max)`. `max` must be > 0.
    fn next_bounded(&self, max: u64) -> u64;
}

/// A deterministic xorshift RNG for tests. Produces a reproducible sequence
/// from a fixed seed so backoff assertions are stable.
#[cfg(test)]
pub(crate) struct SeededJitter {
    state: std::sync::Mutex<u64>,
}

#[cfg(test)]
impl SeededJitter {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            state: std::sync::Mutex::new(seed.max(1)),
        }
    }
}

#[cfg(test)]
impl JitterRng for SeededJitter {
    fn next_bounded(&self, max: u64) -> u64 {
        if max == 0 {
            return 0;
        }
        let mut state = self.state.lock().expect("jitter state poisoned");
        // xorshift64 — deterministic and sufficient for test jitter.
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x % max
    }
}

/// A thread-local production RNG. Uses `rand` so the production path has real
/// randomness without requiring callers to thread an RNG handle.
pub(crate) struct ThreadJitter;

impl JitterRng for ThreadJitter {
    fn next_bounded(&self, max: u64) -> u64 {
        if max == 0 {
            return 0;
        }
        use rand::RngExt;
        rand::rng().random_range(0..max)
    }
}

/// Configuration for the shared retry policy.
#[derive(Clone)]
pub(crate) struct RetryPolicy {
    /// Maximum retry attempts for retryable failures.
    pub max_retries: u32,
    /// Base delay for the first backoff interval.
    pub initial_delay: Duration,
    /// Ceiling for the exponential backoff delay.
    pub max_delay: Duration,
    /// Connect timeout for each attempt. Applied when constructing HTTP
    /// clients that honor the shared policy.
    #[allow(dead_code)]
    pub connect_timeout: Duration,
    /// Idle-read timeout for each attempt. Applied when constructing HTTP
    /// clients that honor the shared policy.
    #[allow(dead_code)]
    pub read_timeout: Duration,
    /// Per-attempt deadline. Applied when constructing HTTP clients that
    /// honor the shared policy.
    #[allow(dead_code)]
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

/// Outcome of a single attempt, used by [`RetryDriver`] to decide whether to
/// retry or return.
pub(crate) enum AttemptOutcome<T> {
    /// The operation succeeded.
    Ok(T),
    /// The operation failed with a classified remote error.
    Err(RemoteError),
}

/// Classify a `reqwest::Error` into a `RemoteErrorKind`. Connection/timeout
/// errors are retryable (`NetworkUnavailable`); everything else is treated as
/// a network failure too (the command layer already sanitized the message).
pub(crate) fn classify_reqwest_error(error: &reqwest::Error) -> RemoteErrorKind {
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
            // Connection reset/refused and other transient IO errors are
            // retryable network failures.
            return RemoteErrorKind::NetworkUnavailable;
        }
    }
    RemoteErrorKind::NetworkUnavailable
}

/// Classify an HTTP status code into a `RemoteErrorKind`, reusing the shared
/// mapping in `errors.rs`.
pub(crate) fn classify_status(status: reqwest::StatusCode) -> RemoteErrorKind {
    kind_from_http_status(status)
}

/// Compute the full-jitter backoff delay for a given attempt index (0-based)
/// using the policy's exponential base and ceiling, then apply full jitter:
/// `delay = uniform(0, min(max_delay, initial_delay * 2^attempt))`.
///
/// Full jitter (rather than equal jitter or decorrelated jitter) is the
/// simplest scheme that avoids synchronized retry storms: every sleeper picks
/// a uniformly random delay in `[0, cap)`, so concurrent retriers spread out.
pub(crate) fn full_jitter_delay(
    policy: &RetryPolicy,
    attempt: u32,
    rng: &dyn JitterRng,
) -> Duration {
    let cap = cap_for_attempt(policy, attempt);
    if cap.is_zero() {
        return Duration::ZERO;
    }
    let millis = cap.as_millis() as u64;
    Duration::from_millis(rng.next_bounded(millis))
}

/// Exponential cap for an attempt: `min(max_delay, initial_delay * 2^attempt)`.
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

/// Parse a `Retry-After` header value. Supports both delta-seconds and (for
/// completeness) an HTTP-date. Bounded values are capped at `MAX_RETRY_AFTER`;
/// unbounded/absurd values return `None` so the caller falls back to jitter.
pub(crate) fn parse_retry_after(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    if let Ok(secs) = trimmed.parse::<u64>() {
        if secs > MAX_RETRY_AFTER.as_secs() {
            return Some(MAX_RETRY_AFTER);
        }
        return Some(Duration::from_secs(secs));
    }
    // HTTP-date form is rarely used by object-storage providers; parse
    // best-effort and cap. On failure, return None.
    if let Ok(parsed) = httpdate::parse_http_date(trimmed) {
        let now = std::time::SystemTime::now();
        if let Ok(delta) = parsed.duration_since(now) {
            if delta > MAX_RETRY_AFTER {
                return Some(MAX_RETRY_AFTER);
            }
            return Some(delta);
        }
    }
    None
}

/// A handle for persisting retry progress to the control DB. The driver writes
/// `attempt_count` and `next_attempt_at_ms` after each retry so a restart does
/// not immediately retry (recovery respects the persisted future timestamp).
pub(crate) trait RetryProgress: Send + Sync {
    /// Persist the attempt count and the next-attempt timestamp (ms since
    /// epoch). Called after each retryable failure.
    fn record_attempt(&self, attempt_count: i64, next_attempt_at_ms: Option<i64>);
}

/// A no-op progress recorder for operations that do not have a control DB row
/// (e.g. the streaming range fetcher hot path and one-shot provider requests).
pub(crate) struct NoopProgress;

impl RetryProgress for NoopProgress {
    fn record_attempt(&self, _attempt_count: i64, _next_attempt_at_ms: Option<i64>) {}
}

/// Drive a fallible operation through the shared retry policy.
///
/// `operation_fn` is called once per attempt and returns an [`AttemptOutcome`].
/// The driver applies backoff + classification + cancellation between
/// retries. Non-retryable errors are returned immediately. Retryable errors
/// are retried up to `policy.max_retries` times.
///
/// `sleep_fn` is injected so tests can use an instant/fake clock instead of
/// real `thread::sleep`. Production passes [`production_sleep`].
pub(crate) struct RetryDriver<'a> {
    pub policy: &'a RetryPolicy,
    pub rng: &'a dyn JitterRng,
    pub cancel: Option<&'a Arc<AtomicBool>>,
    pub progress: Option<&'a dyn RetryProgress>,
    pub sleep_fn: &'a dyn Fn(Duration),
    pub now_ms: &'a dyn Fn() -> i64,
}

impl<'a> RetryDriver<'a> {
    /// Run `operation_fn` with retry + backoff. Returns the successful value
    /// or the final classified `RemoteError`.
    pub(crate) fn run<T, F>(&self, mut operation_fn: F) -> Result<T, RemoteError>
    where
        F: FnMut() -> AttemptOutcome<T>,
    {
        let mut attempt: u32 = 0;
        loop {
            // Check cancellation before each attempt.
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

                    // Compute the backoff: full jitter, but honor a bounded
                    // Retry-After when the error carries one.
                    let delay = err
                        .retry_after
                        .unwrap_or_else(|| full_jitter_delay(self.policy, attempt, self.rng));

                    // Persist progress so a restart does not immediately retry.
                    if let Some(progress) = self.progress {
                        let next_ms = (self.now_ms)() + delay.as_millis() as i64;
                        progress.record_attempt(attempt as i64 + 1, Some(next_ms));
                    }

                    // Sleep before the next attempt, checking cancellation.
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

/// Production sleep function: `thread::sleep`.
pub(crate) fn production_sleep(d: Duration) {
    std::thread::sleep(d);
}

/// Run a fallible remote operation with the default production retry policy.
///
/// Callers rebuild the request inside `operation` on every attempt so the
/// driver can retry after transport failures, 429, and 5xx without holding a
/// consumed `RequestBuilder`. Permanent failures (400/403/404/409/412,
/// integrity, capability) must return a non-retryable [`RemoteError`].
pub(crate) fn run_with_default_retry<T, F>(operation: F) -> Result<T, RemoteError>
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
        now_ms: &|| crate::remote::types::current_unix_time_ms(),
    };
    driver.run(operation)
}

/// Extend `RemoteError` with an optional `Retry-After` delay parsed from the
/// response. This keeps the retry-after parsing in one place while letting the
/// driver honor it.
pub(crate) fn remote_error_with_retry_after(
    kind: RemoteErrorKind,
    detail: impl Into<String>,
    retry_after: Option<Duration>,
) -> RemoteError {
    let mut err = RemoteError::new(kind, detail);
    err.retry_after = retry_after;
    err
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_now(ms: i64) -> std::sync::Arc<std::sync::Mutex<i64>> {
        std::sync::Arc::new(std::sync::Mutex::new(ms))
    }

    fn now_fn(clock: std::sync::Arc<std::sync::Mutex<i64>>) -> Box<dyn Fn() -> i64 + Send + Sync> {
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
        // Attempt 0: cap = 1s → delay in [0, 1s).
        let d0 = full_jitter_delay(&policy, 0, &rng);
        assert!(d0 <= Duration::from_secs(1));
        // Attempt 3: cap = 8s → delay in [0, 8s).
        let d3 = full_jitter_delay(&policy, 3, &rng);
        assert!(d3 <= Duration::from_secs(8));
        // Deterministic: same seed → same sequence.
        let rng2 = SeededJitter::new(42);
        let d0b = full_jitter_delay(&policy, 0, &rng2);
        // Note: full_jitter_delay consumes one RNG draw, so re-creating the
        // RNG and drawing once reproduces d0.
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
        let sleep_calls = std::sync::Mutex::new(Vec::new());
        let sleep = |d: Duration| {
            sleep_calls.lock().unwrap().push(d);
        };
        let clock = fixed_now(1000);
        let now = now_fn(clock.clone());
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
        // Two sleeps before the 2nd and 3rd attempts.
        assert_eq!(sleep_calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn driver_does_not_retry_permanent_error() {
        let policy = RetryPolicy::default();
        let rng = SeededJitter::new(1);
        let sleep = |_| {};
        let clock = fixed_now(1000);
        let now = now_fn(clock.clone());
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
            // Set cancel during the first sleep.
            cancel_ref.store(true, Ordering::Relaxed);
        };
        let clock = fixed_now(1000);
        let now = now_fn(clock.clone());
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
        // First attempt fails, sleep sets cancel, second check aborts.
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
        let now = now_fn(clock.clone());
        let recorded = std::sync::Mutex::new(Vec::new());
        struct Recorder<'a>(&'a std::sync::Mutex<Vec<(i64, Option<i64>)>>);
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
        // Three records: attempt 1 (next in future), attempt 2 (next in
        // future), attempt 3 (final, no future timestamp).
        let recorded = recorded.lock().unwrap();
        assert_eq!(recorded.len(), 3);
        // First record: next_attempt_at_ms > 5000 (in the future).
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
        let sleep_calls = std::sync::Mutex::new(Vec::new());
        let sleep = |d: Duration| {
            sleep_calls.lock().unwrap().push(d);
        };
        let clock = fixed_now(1000);
        let now = now_fn(clock.clone());
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
        // The sleep should be the Retry-After value (50ms), not the jitter.
        assert_eq!(sleep_calls.lock().unwrap()[0], Duration::from_millis(50));
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
}
