use crate::audio::playback::PlaybackController;
use crate::commands::cdg::CdgPlaybackState;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct PlaybackState {
    pub playback: Arc<Mutex<PlaybackController>>,
    pub cdg_state: Arc<Mutex<Option<CdgPlaybackState>>>,
    pub playback_request_id: Arc<AtomicU64>,
    pub audio_output_started: Arc<AtomicBool>,
    pub audio_output_start_lock: Arc<Mutex<()>>,
}

impl PlaybackState {
    pub fn new(playback: Arc<Mutex<PlaybackController>>) -> Self {
        Self {
            playback,
            cdg_state: Arc::new(Mutex::new(None)),
            playback_request_id: Arc::new(AtomicU64::new(0)),
            audio_output_started: Arc::new(AtomicBool::new(false)),
            audio_output_start_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn test_fixture() -> Self {
        Self {
            playback: Arc::new(Mutex::new(PlaybackController::default())),
            cdg_state: Arc::new(Mutex::new(None)),
            playback_request_id: Arc::new(AtomicU64::new(41)),
            audio_output_started: Arc::new(AtomicBool::new(false)),
            audio_output_start_lock: Arc::new(Mutex::new(())),
        }
    }
}
