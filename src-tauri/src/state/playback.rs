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
    pub background_shutdown: Arc<Mutex<Arc<AtomicBool>>>,
    pub preload_shutdown: Arc<Mutex<Arc<AtomicBool>>>,
    pub preload_request_generation: Arc<AtomicU64>,
    pub command_tx: mpsc::Sender<PlaybackCommand>,
    pub peak_ring: Arc<PeakRing>,
    pub output_format: OutputFormatState,
    pub waveform_singleflight: WaveformSingleflight,
}

impl PlaybackState {
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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct WaveformKey {
    pub song_hash: String,
    pub buckets: usize,
}

pub type WaveformResult = Result<Arc<[f32]>, String>;

type Waiters = Vec<tokio::sync::oneshot::Sender<WaveformResult>>;

#[derive(Clone, Default)]
pub struct WaveformSingleflight {
    pending: Arc<Mutex<HashMap<WaveformKey, Waiters>>>,
}

impl WaveformSingleflight {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending_map(&self) -> Arc<Mutex<HashMap<WaveformKey, Waiters>>> {
        Arc::clone(&self.pending)
    }

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

pub struct SingleflightCompletionGuard {
    singleflight: WaveformSingleflight,
    key: WaveformKey,
    completed: bool,
}

impl SingleflightCompletionGuard {
    pub fn new(singleflight: WaveformSingleflight, key: WaveformKey) -> Self {
        Self {
            singleflight,
            key,
            completed: false,
        }
    }

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
        if let Some(waiters) = self.singleflight.take_waiters(&self.key) {
            for waiter in waiters {
                let _ = waiter.send(Err(SANITIZED_WAVEFORM_ERROR.to_owned()));
            }
        }
    }
}

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
