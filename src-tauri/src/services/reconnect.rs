//! Playback reconnect coordinator for remote streaming sources (PR #7,
//! issue #151 defects #8, #11, #12).
//!
//! When the active remote streaming source's range fetch fails with a
//! transient error mid-playback, the coordinator re-resolves the source,
//! swaps it in atomically, and preserves the playback timeline. It is
//! implemented as a testable unit driven by injected closures so the live
//! playback loop (in `services::playback`) and the test harness share the
//! same retry/classification/event-emission logic.
//!
//! ## What is retried
//!
//! Transient failures only, classified via [`ReconnectError`] (which mirrors
//! `remote::errors::RemoteErrorKind`):
//! - network unavailable / timeout (`ReconnectError::Transient`)
//! - provider 5xx (`ReconnectError::Transient`)
//! - credential expiry (`ReconnectError::CredentialExpired`) — triggers a
//!   single-flight credential refresh before the next attempt
//!
//! ## What is NOT retried
//!
//! Permanent failures abort immediately and surface a terminal error:
//! - not found / forbidden (`ReconnectError::NotFound`)
//! - stale request (`ReconnectError::Stale`) — the user skipped past the song
//!
//! ## Events
//!
//! The coordinator emits three events through the injected [`EventSink`]:
//! - [`ReconnectEvent::Reconnecting`] before each re-resolve attempt (PR #8
//!   renders a "reconnecting…" state from this)
//! - [`ReconnectEvent::Resync`] when the new source could not seek to the
//!   exact requested position and snapped to a preceding boundary
//! - [`ReconnectEvent::Failed`] after the attempt budget is exhausted or a
//!   permanent error occurs

use crate::audio::remote_source::FetchEvent;
use crate::remote::cache_catalog::CachePinGuard;
#[cfg(test)]
use crate::remote::net_policy::SeededJitter;
use crate::remote::net_policy::{full_jitter_delay, production_sleep, RetryPolicy};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

/// RAII container for the remote streaming source's cache pin guard and
/// fetch event receiver. The caller must keep this alive for the lifetime
/// of the reconnected playback (until the track is skipped, stopped, or
/// replaced). Dropping it unpins the cache entry and stops consuming fetch
/// events.
///
/// This replaces the previous approach of leaking the pin guard into a
/// detached `thread::park()` thread, which permanently leaked a thread and
/// pinned the cache entry for the process lifetime.
pub(crate) struct RemoteStreamingRuntime {
    /// RAII pin guard. Unpins the cache entry on drop.
    pub(crate) cache_pin_guard: Option<CachePinGuard>,
    /// Fetch event receiver. The caller should drain this in a dedicated
    /// listener thread to handle ConsecutiveFailures, UrlExpired, etc.
    /// `None` for cache-fast-path sources that have no fetch thread.
    pub(crate) fetch_event_rx: Option<mpsc::Receiver<FetchEvent>>,
}

impl std::fmt::Debug for RemoteStreamingRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteStreamingRuntime")
            .field("cache_pin_guard", &self.cache_pin_guard.is_some())
            .field("fetch_event_rx", &self.fetch_event_rx.is_some())
            .finish()
    }
}

/// Maximum reconnect attempts. A small cap keeps the user from waiting
/// indefinitely on a dead provider while still tolerating a brief outage.
/// Reuses the shared `RetryPolicy` for backoff; this cap is independent of
/// `policy.max_retries` because reconnect is a higher-level concern than a
/// single range fetch.
pub(crate) const DEFAULT_MAX_RECONNECT_ATTEMPTS: u32 = 3;

/// Classification of a re-resolve attempt's failure. Mirrors the subset of
/// `RemoteErrorKind` relevant to playback reconnect so the coordinator can
/// branch without depending on the full remote error taxonomy at the call
/// site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReconnectError {
    /// Transient network/server failure — retryable.
    Transient,
    /// Credentials expired — retryable after a single-flight refresh.
    CredentialExpired,
    /// The remote object is gone or access is denied — permanent.
    NotFound,
    /// The playback request is no longer current (user skipped) — permanent.
    Stale,
    /// Any other permanent failure.
    Permanent,
}

impl ReconnectError {
    /// Whether the coordinator should attempt another reconnect for this
    /// error. `Transient` and `CredentialExpired` are retryable; everything
    /// else aborts immediately.
    pub(crate) fn retryable(&self) -> bool {
        matches!(
            self,
            ReconnectError::Transient | ReconnectError::CredentialExpired
        )
    }
}

/// The position the new source seeked to after a reconnect, used to decide
/// whether a resync event is needed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SeekOutcome {
    /// The position the coordinator asked the new source to seek to (ms).
    pub requested_ms: u64,
    /// The position the new source actually seeked to (ms). Equal to
    /// `requested_ms` when the source supports exact seek.
    pub actual_ms: u64,
}

impl SeekOutcome {
    /// `true` when the new source could not seek to the exact requested
    /// position (it snapped to a preceding resumable boundary).
    pub(crate) fn is_resync(&self) -> bool {
        self.actual_ms != self.requested_ms
    }
}

/// Events emitted by the coordinator. The live playback loop forwards these
/// to the frontend as IPC events; the test harness records them directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReconnectEvent {
    /// Emitted before each re-resolve attempt. `attempt` is 1-based.
    /// Payload mirrors the `remote_playback_reconnect` IPC event.
    Reconnecting {
        song_id: String,
        request_id: u64,
        attempt: u32,
        max_attempts: u32,
        reason: String,
    },
    /// Emitted when the new source could not seek to the exact requested
    /// position. Payload mirrors the `remote_playback_resync` IPC event.
    /// `actual_ms` is always `<= requested_ms` (nearest preceding boundary).
    Resync {
        song_id: String,
        requested_position_ms: u64,
        actual_position_ms: u64,
    },
    /// Emitted after the attempt budget is exhausted or a permanent error
    /// occurs. Payload mirrors the `remote_playback_failed` IPC event.
    Failed {
        song_id: String,
        request_id: u64,
        reason: String,
    },
}

/// Sink for coordinator events. The live loop emits IPC events; tests record
/// into a `Vec`.
pub(crate) trait EventSink {
    fn emit(&self, event: ReconnectEvent);
}

/// A re-resolved source plus whether it came from the cache catalog fast path
/// (no network fetch). The cache fast path is the common case after a partial
/// download completes in the background during the reconnect delay.
#[derive(Debug)]
pub(crate) struct ReresolvedSource<S> {
    /// The new source. The coordinator seeks it to the preserved position
    /// before handing it back for atomic swap.
    pub source: S,
    /// `true` when the source was served from the complete + verified cache
    /// catalog entry with no network fetch.
    pub from_cache: bool,
    /// RAII runtime: cache pin guard and fetch event receiver. The caller
    /// must keep this alive for the lifetime of the reconnected playback.
    pub runtime: RemoteStreamingRuntime,
}

/// Outcome of a successful reconnect: the new source (already seeked to the
/// preserved position) and the seek outcome (for resync event emission by
/// the caller).
#[derive(Debug)]
pub(crate) struct ReconnectSuccess<S> {
    pub source: S,
    /// `true` when the source came from the cache catalog fast path.
    ///
    /// Carried for a reconnect UI that was never built. The coordinator
    /// already emits `Resync` when the seek was inexact, so nothing downstream
    /// reads either field today.
    #[allow(dead_code)]
    pub from_cache: bool,
    /// The seek outcome the source reported after the reconnect.
    #[allow(dead_code)]
    pub seek: SeekOutcome,
    /// RAII runtime: cache pin guard and fetch event receiver. The caller
    /// must keep this alive for the lifetime of the reconnected playback.
    pub runtime: RemoteStreamingRuntime,
}

/// Configuration for the reconnect coordinator.
pub(crate) struct ReconnectConfig {
    /// Maximum reconnect attempts before surfacing a terminal error.
    pub max_attempts: u32,
    /// Shared retry policy used for backoff between attempts.
    pub policy: RetryPolicy,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_RECONNECT_ATTEMPTS,
            policy: RetryPolicy {
                // Short backoff for playback reconnect: the user is waiting
                // in real time, so cap the ceiling low.
                max_retries: DEFAULT_MAX_RECONNECT_ATTEMPTS,
                initial_delay: Duration::from_millis(500),
                max_delay: Duration::from_secs(4),
                ..RetryPolicy::default()
            },
        }
    }
}

/// Drive a playback reconnect with bounded retry, timeline preservation, and
/// cache-catalog fast path.
///
/// Type parameters:
/// - `S`: the streaming source type produced by re-resolve.
/// - `R`: the re-resolve closure. Returns either a [`ReresolvedSource`] or a
///   [`ReconnectError`]. It should check the cache catalog first (fast path)
///   and only re-fetch over the network on a cache miss.
/// - `K`: the seek closure. Seeks the new source to `position_ms` and returns
///   the [`SeekOutcome`]. When the source supports exact seek, `actual_ms`
///   equals `requested_ms`; otherwise it snaps to the nearest preceding
///   resumable boundary and the coordinator emits a `Resync` event.
/// - `C`: the credential-refresh trigger. Called only when an attempt fails
///   with `CredentialExpired`, before the next attempt. Returns whether the
///   refresh succeeded (the coordinator proceeds regardless; a failed refresh
///   simply means the next attempt will likely fail the same way and exhaust
///   the budget).
/// - `G`: the stale guard. Returns `true` while `request_id` is still the
///   active playback request. When it reports stale, the coordinator aborts
///   immediately (no terminal event — the user has moved on).
/// - `E`: the event sink.
/// - `Sl`: the sleep function (injectable for tests).
///
/// The coordinator reuses the shared `net_policy` full-jitter backoff so
/// reconnect timing is consistent with the rest of the remote retry story.
pub(crate) fn run_reconnect<S, R, K, C, G, E, Sl>(
    song_id: &str,
    request_id: u64,
    position_ms: u64,
    config: &ReconnectConfig,
    rng: &dyn crate::remote::net_policy::JitterRng,
    cancel: Option<&Arc<AtomicBool>>,
    mut re_resolve: R,
    mut seek_source: K,
    mut refresh_credentials: C,
    is_current: G,
    event_sink: &E,
    sleep_fn: &Sl,
) -> Result<ReconnectSuccess<S>, ReconnectError>
where
    R: FnMut() -> Result<ReresolvedSource<S>, ReconnectError>,
    K: FnMut(&mut S, u64) -> SeekOutcome,
    C: FnMut() -> bool,
    G: Fn() -> bool,
    E: EventSink,
    Sl: Fn(Duration),
{
    let mut attempt: u32 = 0;
    loop {
        // Stale guard: if the user skipped past this song, abort silently.
        // No terminal event — the new song's load owns the UI now.
        if !is_current() {
            return Err(ReconnectError::Stale);
        }
        if let Some(flag) = cancel {
            if flag.load(Ordering::Relaxed) {
                return Err(ReconnectError::Stale);
            }
        }

        attempt += 1;
        // Emit a reconnecting event before the attempt so PR #8 can show a
        // "reconnecting…" state. The reason is derived from the previous
        // failure (transient / credential expired) — on the first attempt
        // there is no prior failure, so the trigger was the fetch-event
        // thread's transient-failure signal.
        let reason = if attempt == 1 {
            "transient fetch failure".to_owned()
        } else {
            "retry after transient failure".to_owned()
        };
        event_sink.emit(ReconnectEvent::Reconnecting {
            song_id: song_id.to_owned(),
            request_id,
            attempt,
            max_attempts: config.max_attempts,
            reason,
        });

        match re_resolve() {
            Ok(mut resolved) => {
                // Timeline preservation (defect #12): seek the new source to
                // the position the old source was at when the failure
                // occurred. The seek closure reports the actual position;
                // when it cannot seek exactly, it snaps to the nearest
                // preceding boundary and the coordinator emits a Resync
                // event.
                let outcome = seek_source(&mut resolved.source, position_ms);
                if outcome.is_resync() {
                    event_sink.emit(ReconnectEvent::Resync {
                        song_id: song_id.to_owned(),
                        requested_position_ms: outcome.requested_ms,
                        actual_position_ms: outcome.actual_ms,
                    });
                }
                return Ok(ReconnectSuccess {
                    source: resolved.source,
                    from_cache: resolved.from_cache,
                    seek: outcome,
                    runtime: resolved.runtime,
                });
            }
            Err(error) => {
                // Non-retryable errors abort immediately and surface a
                // terminal event.
                if !error.retryable() {
                    event_sink.emit(ReconnectEvent::Failed {
                        song_id: song_id.to_owned(),
                        request_id,
                        reason: format!("{error:?}"),
                    });
                    return Err(error);
                }

                // Credential expiry: trigger the single-flight refresh
                // before the next attempt. Non-credential errors do not
                // refresh (defect #10 / PR #5 single-flight invariant).
                if error == ReconnectError::CredentialExpired {
                    refresh_credentials();
                }

                // Budget exhausted: surface a terminal error.
                if attempt >= config.max_attempts {
                    event_sink.emit(ReconnectEvent::Failed {
                        song_id: song_id.to_owned(),
                        request_id,
                        reason: format!("exhausted {attempt} reconnect attempts"),
                    });
                    return Err(error);
                }

                // Backoff between attempts using the shared full-jitter
                // policy so reconnect timing is consistent with the rest of
                // the remote retry story.
                let delay = full_jitter_delay(&config.policy, attempt - 1, rng);
                if !delay.is_zero() {
                    sleep_fn(delay);
                }
            }
        }
    }
}

/// A simple event sink that records events into a `Mutex<Vec>`. Used by tests
/// and by the live playback loop when it wants to inspect the sequence.
#[cfg(test)]
pub(crate) struct RecordingEventSink {
    pub events: std::sync::Mutex<Vec<ReconnectEvent>>,
}

#[cfg(test)]
impl RecordingEventSink {
    pub(crate) fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
impl EventSink for RecordingEventSink {
    fn emit(&self, event: ReconnectEvent) {
        self.events.lock().unwrap().push(event);
    }
}

/// Production reconnect entry point used by `services::playback`. Wraps
/// [`run_reconnect`] with the production jitter RNG and sleep function.
/// Exposed so the playback loop does not need to thread the RNG/sleep itself.
pub(crate) fn reconnect_production<S, R, K, C, G, E>(
    song_id: &str,
    request_id: u64,
    position_ms: u64,
    config: &ReconnectConfig,
    cancel: Option<&Arc<AtomicBool>>,
    re_resolve: R,
    seek_source: K,
    refresh_credentials: C,
    is_current: G,
    event_sink: &E,
) -> Result<ReconnectSuccess<S>, ReconnectError>
where
    R: FnMut() -> Result<ReresolvedSource<S>, ReconnectError>,
    K: FnMut(&mut S, u64) -> SeekOutcome,
    C: FnMut() -> bool,
    G: Fn() -> bool,
    E: EventSink,
{
    let rng = crate::remote::net_policy::ThreadJitter;
    run_reconnect(
        song_id,
        request_id,
        position_ms,
        config,
        &rng,
        cancel,
        re_resolve,
        seek_source,
        refresh_credentials,
        is_current,
        event_sink,
        &production_sleep,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A fake streaming source carrying the position it was seeked to.
    #[derive(Debug)]
    struct FakeSource {
        seeked_to_ms: u64,
        /// When `Some(block_ms)`, the source can only seek to multiples of
        /// `block_ms` (simulates block-boundary-only seek).
        block_ms: Option<u64>,
    }

    fn seek_fake(source: &mut FakeSource, position_ms: u64) -> SeekOutcome {
        let actual = match source.block_ms {
            Some(block) => (position_ms / block) * block,
            None => position_ms,
        };
        source.seeked_to_ms = actual;
        SeekOutcome {
            requested_ms: position_ms,
            actual_ms: actual,
        }
    }

    #[test]
    fn reconnect_succeeds_on_transient_error_after_retry() {
        let sink = RecordingEventSink::new();
        let calls = Arc::new(AtomicU32::new(0));
        let calls_clone = Arc::clone(&calls);
        let re_resolve = move || {
            let n = calls_clone.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Err(ReconnectError::Transient)
            } else {
                Ok(ReresolvedSource {
                    source: FakeSource {
                        seeked_to_ms: 0,
                        block_ms: None,
                    },
                    from_cache: false,
                    runtime: RemoteStreamingRuntime {
                        cache_pin_guard: None,
                        fetch_event_rx: None,
                    },
                })
            }
        };
        let config = ReconnectConfig {
            max_attempts: 3,
            policy: RetryPolicy {
                initial_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(2),
                ..RetryPolicy::default()
            },
        };
        let rng = SeededJitter::new(1);
        let result = run_reconnect(
            "song-a",
            1,
            5000,
            &config,
            &rng,
            None,
            re_resolve,
            seek_fake,
            || false,
            || true,
            &sink,
            &|_| {},
        );
        assert!(result.is_ok(), "reconnect should succeed");
        let success = result.unwrap();
        assert_eq!(success.seek.actual_ms, 5000);
        assert_eq!(success.source.seeked_to_ms, 5000);
        let events = sink.events.lock().unwrap();
        // First Reconnecting (attempt 1), then second Reconnecting (attempt 2)
        // before the successful resolve.
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0],
            ReconnectEvent::Reconnecting { attempt: 1, .. }
        ));
        assert!(matches!(
            events[1],
            ReconnectEvent::Reconnecting { attempt: 2, .. }
        ));
    }

    #[test]
    fn reconnect_exhausts_attempts_and_surfaces_terminal_error() {
        let sink = RecordingEventSink::new();
        let config = ReconnectConfig {
            max_attempts: 2,
            policy: RetryPolicy {
                initial_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(2),
                ..RetryPolicy::default()
            },
        };
        let rng = SeededJitter::new(1);
        let result = run_reconnect(
            "song-a",
            1,
            1000,
            &config,
            &rng,
            None,
            || Err(ReconnectError::Transient),
            seek_fake,
            || false,
            || true,
            &sink,
            &|_| {},
        );
        let err = result.expect_err("should exhaust attempts");
        assert_eq!(err, ReconnectError::Transient);
        let events = sink.events.lock().unwrap();
        // 2 Reconnecting + 1 Failed
        assert_eq!(events.len(), 3);
        assert!(matches!(events[2], ReconnectEvent::Failed { .. }));
    }

    #[test]
    fn non_transient_error_does_not_trigger_reconnect() {
        let sink = RecordingEventSink::new();
        let config = ReconnectConfig::default();
        let rng = SeededJitter::new(1);
        let result = run_reconnect(
            "song-a",
            1,
            1000,
            &config,
            &rng,
            None,
            || Err(ReconnectError::NotFound),
            seek_fake,
            || false,
            || true,
            &sink,
            &|_| {},
        );
        let err = result.expect_err("permanent error should abort");
        assert_eq!(err, ReconnectError::NotFound);
        let events = sink.events.lock().unwrap();
        // 1 Reconnecting + 1 Failed, no retry.
        assert_eq!(events.len(), 2);
        assert!(matches!(events[1], ReconnectEvent::Failed { .. }));
    }

    #[test]
    fn credential_refresh_triggered_on_credential_expired_error() {
        let sink = RecordingEventSink::new();
        let refresh_calls = Arc::new(AtomicU32::new(0));
        let refresh_calls_clone = Arc::clone(&refresh_calls);
        let config = ReconnectConfig {
            max_attempts: 3,
            policy: RetryPolicy {
                initial_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(2),
                ..RetryPolicy::default()
            },
        };
        let rng = SeededJitter::new(1);
        let result = run_reconnect(
            "song-a",
            1,
            1000,
            &config,
            &rng,
            None,
            || Err(ReconnectError::CredentialExpired),
            seek_fake,
            move || {
                refresh_calls_clone.fetch_add(1, Ordering::SeqCst);
                true
            },
            || true,
            &sink,
            &|_| {},
        );
        // Exhausts attempts (all CredentialExpired) → returns CredentialExpired.
        let err = result.expect_err("should exhaust");
        assert_eq!(err, ReconnectError::CredentialExpired);
        // Refresh triggered once per retryable failure (attempts 1 and 2; the
        // 3rd attempt exhausts the budget but still refreshes before the
        // budget check). The coordinator refreshes on every
        // CredentialExpired failure, so 3 refreshes for 3 attempts.
        assert_eq!(refresh_calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn resync_event_when_exact_seek_unavailable() {
        let sink = RecordingEventSink::new();
        let config = ReconnectConfig {
            max_attempts: 3,
            policy: RetryPolicy {
                initial_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(2),
                ..RetryPolicy::default()
            },
        };
        let rng = SeededJitter::new(1);
        // Source can only seek to 100ms block boundaries.
        let result = run_reconnect(
            "song-a",
            1,
            1250,
            &config,
            &rng,
            None,
            || {
                Ok(ReresolvedSource {
                    source: FakeSource {
                        seeked_to_ms: 0,
                        block_ms: Some(100),
                    },
                    from_cache: false,
                    runtime: RemoteStreamingRuntime {
                        cache_pin_guard: None,
                        fetch_event_rx: None,
                    },
                })
            },
            seek_fake,
            || false,
            || true,
            &sink,
            &|_| {},
        );
        let success = result.expect("should succeed");
        assert_eq!(success.seek.requested_ms, 1250);
        assert_eq!(success.seek.actual_ms, 1200);
        assert!(success.seek.is_resync());
        let events = sink.events.lock().unwrap();
        // Reconnecting + Resync
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[1],
            ReconnectEvent::Resync {
                requested_position_ms: 1250,
                actual_position_ms: 1200,
                ..
            }
        ));
    }

    #[test]
    fn stale_guard_aborts_silently() {
        let sink = RecordingEventSink::new();
        let config = ReconnectConfig::default();
        let rng = SeededJitter::new(1);
        let result = run_reconnect(
            "song-a",
            1,
            1000,
            &config,
            &rng,
            None,
            || {
                Ok(ReresolvedSource {
                    source: FakeSource {
                        seeked_to_ms: 0,
                        block_ms: None,
                    },
                    from_cache: false,
                    runtime: RemoteStreamingRuntime {
                        cache_pin_guard: None,
                        fetch_event_rx: None,
                    },
                })
            },
            seek_fake,
            || false,
            || false, // stale immediately
            &sink,
            &|_| {},
        );
        let err = result.expect_err("stale guard should abort");
        assert_eq!(err, ReconnectError::Stale);
        // No events emitted — the user has moved on.
        assert!(sink.events.lock().unwrap().is_empty());
    }

    #[test]
    fn cache_fast_path_succeeds_without_network() {
        let sink = RecordingEventSink::new();
        let config = ReconnectConfig::default();
        let rng = SeededJitter::new(1);
        let result = run_reconnect(
            "song-a",
            1,
            1000,
            &config,
            &rng,
            None,
            || {
                Ok(ReresolvedSource {
                    source: FakeSource {
                        seeked_to_ms: 0,
                        block_ms: None,
                    },
                    from_cache: true,
                    runtime: RemoteStreamingRuntime {
                        cache_pin_guard: None,
                        fetch_event_rx: None,
                    },
                })
            },
            seek_fake,
            || false,
            || true,
            &sink,
            &|_| {},
        );
        let success = result.expect("cache fast path should succeed");
        assert!(success.from_cache);
        assert_eq!(success.seek.actual_ms, 1000);
    }
}
