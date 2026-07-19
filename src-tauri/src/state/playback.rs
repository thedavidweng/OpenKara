use crate::audio::coordinator::PlaybackCommand;
use crate::audio::output_format::OutputFormatState;
use crate::audio::peaks::PeakRing;
use crate::audio::playback::PlaybackController;
use crate::commands::cdg::CdgPlaybackSlot;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{mpsc, Arc, Mutex, RwLock};

#[derive(Clone)]
pub struct PlaybackState {
    pub playback: Arc<Mutex<PlaybackController>>,
    pub cdg_state: Arc<Mutex<Option<CdgPlaybackSlot>>>,
    pub playback_request_id: Arc<AtomicU64>,
    pub audio_output_started: Arc<AtomicBool>,
    pub audio_output_start_lock: Arc<Mutex<()>>,
    /// Shutdown signal for the background decode/fetch thread. Signalled
    /// when a new `play()` starts so the old thread can bail out early instead
    /// of running to completion and wasting CPU/memory.
    /// Wrapped in Mutex so `play()` can replace the Arc with a fresh one.
    pub background_shutdown: Arc<Mutex<Arc<AtomicBool>>>,
    /// Shutdown signal for the gapless preload thread. Separate from
    /// `background_shutdown` so that a `set_preload_candidate` call (which
    /// fires whenever the queue head or current song changes) does not cancel
    /// an in-flight `play()` background decode thread. The preload effect in
    /// the frontend reacts to `currentSongId` changes, which happen during
    /// `play()` loading — sharing the flag would kill the play thread.
    pub preload_shutdown: Arc<Mutex<Arc<AtomicBool>>>,
    /// Monotonic generation bumped on every `set_preload_candidate`
    /// call. The preload thread captures this value and includes it in the
    /// `PrepareNext` command; the coordinator stamps it onto
    /// `PlaybackController::expected_preload_request_generation` via
    /// `CancelPreparedNext` so stale `PrepareNext` commands from older
    /// preload threads are rejected.
    pub preload_request_generation: Arc<AtomicU64>,
    /// Sender for the PlaybackCoordinator command queue. The coordinator worker
    /// owns the receiver; all control-plane mutations go through this channel.
    pub command_tx: mpsc::Sender<PlaybackCommand>,
    /// Process-wide lock-free peak ring shared between the CPAL output callback
    /// (single writer) and the `get_audio_peaks` command (any reader). The
    /// command reads only the ring and must not lock `PlaybackController`.
    pub peak_ring: Arc<PeakRing>,
    /// Output-format descriptor published by the CPAL output worker.
    /// The preload scheduler captures this to normalize the next track to the
    /// active device format; the coordinator validates the generation before
    /// installing a prepared track.
    pub output_format: OutputFormatState,
    /// Process-wide singleflight for waveform computation. Multiple
    /// WebViews requesting the same `(song_hash, buckets)` share one owned
    /// blocking computation task; cancellation of any caller only drops its
    /// receiver and never cancels work needed by remaining waiters.
    pub waveform_singleflight: WaveformSingleflight,
}

impl PlaybackState {
    /// Construct a `PlaybackState` and return the coordinator receiver.
    /// The receiver must be moved into `spawn_coordinator`; the sender stays
    /// in managed state for command dispatch.
    pub fn new(
        playback: Arc<Mutex<PlaybackController>>,
    ) -> (Self, mpsc::Receiver<PlaybackCommand>) {
        let (command_tx, command_rx) = mpsc::channel();
        (
            Self {
                playback,
                cdg_state: Arc::new(Mutex::new(None)),
                playback_request_id: Arc::new(AtomicU64::new(0)),
                audio_output_started: Arc::new(AtomicBool::new(false)),
                audio_output_start_lock: Arc::new(Mutex::new(())),
                background_shutdown: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
                preload_shutdown: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
                preload_request_generation: Arc::new(AtomicU64::new(0)),
                command_tx,
                peak_ring: Arc::new(PeakRing::new()),
                output_format: Arc::new(RwLock::new(None)),
                waveform_singleflight: WaveformSingleflight::new(),
            },
            command_rx,
        )
    }

    /// Test fixture with a disconnected sender. Tests that exercise commands
    /// must spawn a coordinator harness; tests that only inspect shared state
    /// may use this directly.
    pub fn test_fixture() -> Self {
        let (command_tx, _) = mpsc::channel();
        Self {
            playback: Arc::new(Mutex::new(PlaybackController::default())),
            cdg_state: Arc::new(Mutex::new(None)),
            playback_request_id: Arc::new(AtomicU64::new(41)),
            audio_output_started: Arc::new(AtomicBool::new(false)),
            audio_output_start_lock: Arc::new(Mutex::new(())),
            background_shutdown: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
            preload_shutdown: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
            preload_request_generation: Arc::new(AtomicU64::new(0)),
            command_tx,
            peak_ring: Arc::new(PeakRing::new()),
            output_format: Arc::new(RwLock::new(None)),
            waveform_singleflight: WaveformSingleflight::new(),
        }
    }
}

/// Composite key for waveform computation deduplication. Two callers
/// requesting different bucket counts for the same song hash do not share
/// a computation; the cache and singleflight both key on `(song_hash, buckets)`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct WaveformKey {
    pub song_hash: String,
    pub buckets: usize,
}

/// A shared waveform result. The `Err` variant carries a sanitized message
/// suitable for IPC — no raw absolute paths.
pub type WaveformResult = Result<Arc<[f32]>, String>;

type Waiters = Vec<tokio::sync::oneshot::Sender<WaveformResult>>;

/// Cancellation-safe singleflight for waveform computation.
///
/// Every caller creates a oneshot sender/receiver under the map lock:
/// - occupied key: append sender, release lock, await receiver;
/// - vacant key: insert a vector containing the first sender, release lock,
///   spawn one owned async computation task, then await the first receiver.
///
/// The computation task, rather than the first request future, owns
/// completion. Cancellation of any WebView/request only drops its receiver
/// and never cancels work needed by remaining waiters. A task-owned
/// completion guard always removes the key and fan-outs either the result or
/// a fixed sanitized error on ordinary failure, `JoinError`, unwind or task
/// cancellation. `Drop` recovers a poisoned standard mutex with
/// `poisoned.into_inner()`; silently skipping removal would leave a
/// permanent pending entry.
#[derive(Clone, Default)]
pub struct WaveformSingleflight {
    pending: Arc<Mutex<HashMap<WaveformKey, Waiters>>>,
}

impl WaveformSingleflight {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up the shared pending map. Exposed so the command layer can
    /// inject a test-only worker via [`with_worker`].
    pub fn pending_map(&self) -> Arc<Mutex<HashMap<WaveformKey, Waiters>>> {
        Arc::clone(&self.pending)
    }

    /// Register a new waiter for `key`. Returns `(receiver, inserted)` where
    /// `inserted == true` means the caller is the first waiter and must
    /// spawn the computation task; `false` means a computation is already
    /// in flight and the caller should simply await the receiver.
    ///
    /// Poison recovery: if the map mutex is poisoned, recover with
    /// `poisoned.into_inner()` rather than propagating the error — a
    /// poisoned lock would otherwise permanently strand the key.
    pub fn register(
        &self,
        key: WaveformKey,
    ) -> (tokio::sync::oneshot::Receiver<WaveformResult>, bool) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let mut guard = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let inserted = !guard.contains_key(&key);
        guard.entry(key).or_default().push(tx);
        (rx, inserted)
    }

    /// Remove the waiters for `key` and return them for fan-out. Returns
    /// `None` if the key is absent (e.g. another path already completed).
    /// No send occurs while holding the map lock: the guard is dropped
    /// before the caller iterates senders.
    pub fn take_waiters(&self, key: &WaveformKey) -> Option<Waiters> {
        let mut guard = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.remove(key)
    }

    #[cfg(test)]
    pub fn pending_count(&self) -> usize {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }
}

/// RAII completion guard for a singleflight computation task.
///
/// Created inside the spawned task **before** awaiting the blocking
/// computation, so that if the task is dropped at any point (cancellation
/// during runtime shutdown, `JoinError`, panic unwind), `Drop` runs and
/// removes the key from the pending map, fanning out a sanitized error to
/// any remaining waiters. Without this guard, a cancelled task between the
/// `spawn_blocking` await and `take_waiters` would permanently strand the
/// key, causing all future requests for that song to hang forever.
///
/// The guard is marked `completed` after a successful fan-out so `Drop`
/// becomes a no-op on the normal exit path.
pub struct SingleflightCompletionGuard {
    singleflight: WaveformSingleflight,
    key: WaveformKey,
    completed: bool,
}

impl SingleflightCompletionGuard {
    /// Create a guard bound to `key` on `singleflight`. The guard does not
    /// take waiters yet — that happens in [`Self::complete`].
    pub fn new(singleflight: WaveformSingleflight, key: WaveformKey) -> Self {
        Self {
            singleflight,
            key,
            completed: false,
        }
    }

    /// Take the waiters for this key, mark the guard as completed, and
    /// return the waiters for fan-out. After this call, `Drop` will not
    /// attempt a second removal. Returns `None` if the key is already
    /// absent (e.g. a prior guard or test path already cleared it).
    pub fn complete(&mut self) -> Option<Waiters> {
        self.completed = true;
        self.singleflight.take_waiters(&self.key)
    }
}

impl Drop for SingleflightCompletionGuard {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        // The task was cancelled before completing. Remove the stranded key
        // and fan out a sanitized error so waiters do not hang forever.
        if let Some(waiters) = self.singleflight.take_waiters(&self.key) {
            for waiter in waiters {
                let _ = waiter.send(Err(SANITIZED_WAVEFORM_ERROR.to_owned()));
            }
        }
    }
}

/// Sanitized error returned to waiters when the computation task fails or
/// is cancelled. The message is intentionally generic — no raw absolute
/// paths leak to IPC. Shared between the guard and the command layer.
pub const SANITIZED_WAVEFORM_ERROR: &str = "waveform computation failed";

#[cfg(test)]
mod waveform_singleflight_tests {
    use super::*;

    fn key(hash: &str, buckets: usize) -> WaveformKey {
        WaveformKey {
            song_hash: hash.to_owned(),
            buckets,
        }
    }

    #[test]
    fn simultaneous_callers_share_one_computation() {
        let sf = WaveformSingleflight::new();
        let k = key("song-1", 200);
        let (rx1, inserted1) = sf.register(k.clone());
        let (rx2, inserted2) = sf.register(k.clone());
        assert!(inserted1, "first caller inserts");
        assert!(!inserted2, "second caller appends");

        let waiters = sf.take_waiters(&k).expect("waiters exist");
        assert_eq!(waiters.len(), 2);
        let peaks: Arc<[f32]> = Arc::from(vec![0.5; 200]);
        for waiter in waiters {
            let _ = waiter.send(Ok(Arc::clone(&peaks)));
        }

        let r1 = rx1.blocking_recv().expect("rx1").expect("ok");
        let r2 = rx2.blocking_recv().expect("rx2").expect("ok");
        assert_eq!(r1.as_ref(), r2.as_ref());
        assert_eq!(r1.as_ref(), peaks.as_ref());
        assert_eq!(sf.pending_count(), 0, "key cleared after completion");
    }

    #[test]
    fn composite_keys_isolate_bucket_counts() {
        let sf = WaveformSingleflight::new();
        let k200 = key("song-1", 200);
        let k400 = key("song-1", 400);
        let (_rx200, inserted200) = sf.register(k200.clone());
        let (_rx400, inserted400) = sf.register(k400.clone());
        assert!(inserted200);
        assert!(inserted400, "different bucket count is a different key");
        assert_eq!(sf.pending_count(), 2);
    }

    #[test]
    fn ordinary_error_clears_entry_and_propagates_sanitized_message() {
        let sf = WaveformSingleflight::new();
        let k = key("song-err", 200);
        let (rx, _inserted) = sf.register(k.clone());
        let waiters = sf.take_waiters(&k).expect("waiters");
        for waiter in waiters {
            let _ = waiter.send(Err(SANITIZED_WAVEFORM_ERROR.to_owned()));
        }
        let err = rx.blocking_recv().expect("rx").expect_err("err");
        assert_eq!(err, SANITIZED_WAVEFORM_ERROR);
        assert_eq!(sf.pending_count(), 0);
    }

    #[test]
    fn dropped_first_receiver_does_not_strand_key() {
        let sf = WaveformSingleflight::new();
        let k = key("song-drop", 200);
        let (_rx1_dropped, _inserted) = sf.register(k.clone());
        let (rx2, _appended) = sf.register(k.clone());
        drop(_rx1_dropped);

        let waiters = sf.take_waiters(&k).expect("waiters");
        let peaks: Arc<[f32]> = Arc::from(vec![0.3; 200]);
        for waiter in waiters {
            let _ = waiter.send(Ok(Arc::clone(&peaks)));
        }
        let r2 = rx2.blocking_recv().expect("rx2").expect("ok");
        assert_eq!(r2.as_ref(), peaks.as_ref());
        assert_eq!(sf.pending_count(), 0);
    }

    #[test]
    fn all_receivers_dropped_clears_entry() {
        let sf = WaveformSingleflight::new();
        let k = key("song-all-dropped", 200);
        let (rx1, _inserted) = sf.register(k.clone());
        let (rx2, _appended) = sf.register(k.clone());
        drop(rx1);
        drop(rx2);

        let waiters = sf.take_waiters(&k).expect("waiters");
        let peaks: Arc<[f32]> = Arc::from(vec![0.7; 200]);
        for waiter in waiters {
            let _ = waiter.send(Ok(Arc::clone(&peaks)));
        }
        assert_eq!(sf.pending_count(), 0);
    }

    #[test]
    fn retry_after_failure_starts_one_new_computation() {
        let sf = WaveformSingleflight::new();
        let k = key("song-retry", 200);

        let (rx1, _inserted) = sf.register(k.clone());
        let waiters = sf.take_waiters(&k).expect("waiters first");
        for waiter in waiters {
            let _ = waiter.send(Err(SANITIZED_WAVEFORM_ERROR.to_owned()));
        }
        let _ = rx1.blocking_recv().expect("rx1").expect_err("first fails");
        assert_eq!(sf.pending_count(), 0);

        let (rx2, inserted2) = sf.register(k.clone());
        assert!(inserted2, "retry inserts a new computation");
        let waiters = sf.take_waiters(&k).expect("waiters second");
        let peaks: Arc<[f32]> = Arc::from(vec![0.9; 200]);
        for waiter in waiters {
            let _ = waiter.send(Ok(Arc::clone(&peaks)));
        }
        let r2 = rx2.blocking_recv().expect("rx2").expect("ok");
        assert_eq!(r2.as_ref(), peaks.as_ref());
        assert_eq!(sf.pending_count(), 0);
    }

    #[test]
    fn poisoned_mutex_is_recovered_not_propagated() {
        let sf = WaveformSingleflight::new();
        let pending = Arc::clone(&sf.pending);
        let _ = std::thread::spawn(move || {
            let _guard = pending.lock().expect("lock");
            panic!("poison");
        })
        .join();

        let k = key("song-poison", 200);
        let (rx, _inserted) = sf.register(k.clone());
        let waiters = sf.take_waiters(&k).expect("waiters after poison");
        for waiter in waiters {
            let _ = waiter.send(Ok(Arc::from(vec![0.2; 200])));
        }
        let r = rx.blocking_recv().expect("rx").expect("ok");
        assert_eq!(r.len(), 200);
        assert_eq!(sf.pending_count(), 0);
    }

    #[test]
    fn completion_guard_drop_clears_stranded_key_and_errors_waiters() {
        let sf = WaveformSingleflight::new();
        let k = key("song-cancel", 200);
        let (rx1, _inserted) = sf.register(k.clone());
        let (rx2, _appended) = sf.register(k.clone());
        assert_eq!(sf.pending_count(), 1, "one pending entry");

        {
            let _guard = SingleflightCompletionGuard::new(sf.clone(), k.clone());
            assert_eq!(sf.pending_count(), 1, "guard creation does not clear key");
        }

        assert_eq!(sf.pending_count(), 0, "drop cleared the stranded key");

        let err1 = rx1.blocking_recv().expect("rx1").expect_err("err1");
        let err2 = rx2.blocking_recv().expect("rx2").expect_err("err2");
        assert_eq!(err1, SANITIZED_WAVEFORM_ERROR);
        assert_eq!(err2, SANITIZED_WAVEFORM_ERROR);
    }

    #[test]
    fn completion_guard_complete_marks_done_so_drop_is_noop() {
        let sf = WaveformSingleflight::new();
        let k = key("song-complete", 200);
        let (rx, _inserted) = sf.register(k.clone());

        let mut guard = SingleflightCompletionGuard::new(sf.clone(), k.clone());
        let waiters = guard.complete().expect("waiters present");
        assert_eq!(sf.pending_count(), 0, "complete removed the key");

        let peaks: Arc<[f32]> = Arc::from(vec![0.4; 200]);
        for waiter in waiters {
            let _ = waiter.send(Ok(Arc::clone(&peaks)));
        }
        drop(guard);
        assert_eq!(sf.pending_count(), 0, "drop after complete is a no-op");

        let r = rx.blocking_recv().expect("rx").expect("ok");
        assert_eq!(r.as_ref(), peaks.as_ref());
    }

    #[test]
    fn completion_guard_drop_allows_retry_after_cancellation() {
        // After a guard is dropped (task cancelled), a subsequent register
        // must insert a fresh computation rather than appending to a
        // stranded dead entry.
        let sf = WaveformSingleflight::new();
        let k = key("song-retry-after-cancel", 200);

        // First attempt: register, then drop the guard (cancellation).
        let (rx1, _inserted) = sf.register(k.clone());
        {
            let _guard = SingleflightCompletionGuard::new(sf.clone(), k.clone());
        }
        let _ = rx1.blocking_recv().expect("rx1").expect_err("cancelled");
        assert_eq!(sf.pending_count(), 0);

        // Retry: must insert a new computation.
        let (rx2, inserted2) = sf.register(k.clone());
        assert!(
            inserted2,
            "retry after cancellation inserts a new computation"
        );

        let waiters = sf.take_waiters(&k).expect("waiters retry");
        let peaks: Arc<[f32]> = Arc::from(vec![0.6; 200]);
        for waiter in waiters {
            let _ = waiter.send(Ok(Arc::clone(&peaks)));
        }
        let r2 = rx2.blocking_recv().expect("rx2").expect("ok");
        assert_eq!(r2.as_ref(), peaks.as_ref());
        assert_eq!(sf.pending_count(), 0);
    }

    #[test]
    fn completion_guard_drop_with_no_waiters_is_harmless() {
        // If all waiters have already been taken (e.g. by a prior
        // complete() or take_waiters), the guard's drop finds no waiters
        // and simply ensures the key is absent — no panic, no send.
        let sf = WaveformSingleflight::new();
        let k = key("song-no-waiters", 200);
        let (_rx, _inserted) = sf.register(k.clone());
        // Take waiters first, simulating an external clear.
        let _ = sf.take_waiters(&k).expect("waiters");
        assert_eq!(sf.pending_count(), 0);

        // Now drop a guard — take_waiters returns None, no send occurs.
        let _guard = SingleflightCompletionGuard::new(sf.clone(), k.clone());
        drop(_guard);
        assert_eq!(sf.pending_count(), 0);
    }
}
