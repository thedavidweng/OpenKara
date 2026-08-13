use crate::{audio::error::PlaybackError, state::PlaybackState};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

/// Answers "is this request still the one the coordinator will honour?".
/// Cloning shares the atomic the coordinator adjudicates against, so a clone
/// handed to a background step observes exactly the same verdict.
#[derive(Clone)]
pub(crate) struct StalenessGuard {
    latest: Arc<AtomicU64>,
    request_id: u64,
}

impl StalenessGuard {
    /// Adopt the request already in flight instead of superseding it.
    pub(crate) fn current(playback: &PlaybackState) -> Self {
        Self {
            request_id: playback.playback_request_id.load(Ordering::SeqCst),
            latest: Arc::clone(&playback.playback_request_id),
        }
    }

    pub(crate) fn request_id(&self) -> u64 {
        self.request_id
    }

    pub(crate) fn is_current(&self) -> bool {
        self.latest.load(Ordering::SeqCst) == self.request_id
    }

    /// Closure form for the loaders that take `impl Fn() -> bool`.
    pub(crate) fn predicate(&self) -> impl Fn() -> bool + Send + 'static {
        let guard = self.clone();
        move || guard.is_current()
    }
}

/// The lifecycle of one track-load request: allocation of the request id,
/// retirement of the previous request's background worker, and the staleness
/// guard every step of the load is measured against.
#[derive(Clone)]
pub(crate) struct PlaybackRequest {
    guard: StalenessGuard,
    shutdown: Arc<AtomicBool>,
}

impl PlaybackRequest {
    /// Supersede whatever request is in flight. The controller lock is held
    /// across the id bump so a coordinator handler already inside that lock
    /// finishes adjudicating against the id it read.
    pub(crate) fn begin(playback: &PlaybackState) -> Result<Self, PlaybackError> {
        let request_id = {
            let _controller = playback.playback.lock().map_err(|_| {
                PlaybackError::Internal("playback controller lock was poisoned".to_owned())
            })?;
            playback.playback_request_id.fetch_add(1, Ordering::SeqCst) + 1
        };

        let shutdown = {
            let mut current = playback.background_shutdown.lock().map_err(|_| {
                PlaybackError::Internal("background_shutdown lock was poisoned".to_owned())
            })?;
            current.store(true, Ordering::Relaxed);
            let replacement = Arc::new(AtomicBool::new(false));
            *current = Arc::clone(&replacement);
            replacement
        };

        Ok(Self {
            guard: StalenessGuard {
                latest: Arc::clone(&playback.playback_request_id),
                request_id,
            },
            shutdown,
        })
    }

    pub(crate) fn id(&self) -> u64 {
        self.guard.request_id()
    }

    pub(crate) fn guard(&self) -> StalenessGuard {
        self.guard.clone()
    }

    /// `true` once a newer request has retired this one's background worker.
    pub(crate) fn is_cancelled(&self) -> bool {
        self.shutdown.load(Ordering::Relaxed)
    }
}
