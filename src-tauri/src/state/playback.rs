use crate::audio::coordinator::PlaybackCommand;
use crate::audio::peaks::PeakRing;
use crate::audio::playback::PlaybackController;
use crate::commands::cdg::CdgPlaybackState;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{mpsc, Arc, Mutex};

#[derive(Clone)]
pub struct PlaybackState {
    pub playback: Arc<Mutex<PlaybackController>>,
    pub cdg_state: Arc<Mutex<Option<CdgPlaybackState>>>,
    pub playback_request_id: Arc<AtomicU64>,
    pub audio_output_started: Arc<AtomicBool>,
    pub audio_output_start_lock: Arc<Mutex<()>>,
    /// R9: Shutdown signal for the background decode/fetch thread. Signalled
    /// when a new `play()` starts so the old thread can bail out early instead
    /// of running to completion and wasting CPU/memory.
    /// Wrapped in Mutex so `play()` can replace the Arc with a fresh one.
    pub background_shutdown: Arc<Mutex<Arc<AtomicBool>>>,
    /// Sender for the PlaybackCoordinator command queue. The coordinator worker
    /// owns the receiver; all control-plane mutations go through this channel.
    pub command_tx: mpsc::Sender<PlaybackCommand>,
    /// Process-wide lock-free peak ring shared between the CPAL output callback
    /// (single writer) and the `get_audio_peaks` command (any reader). The
    /// command reads only the ring and must not lock `PlaybackController`.
    pub peak_ring: Arc<PeakRing>,
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
                command_tx,
                peak_ring: Arc::new(PeakRing::new()),
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
            command_tx,
            peak_ring: Arc::new(PeakRing::new()),
        }
    }
}
