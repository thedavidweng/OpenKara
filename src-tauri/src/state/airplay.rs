use crate::airplay_stream::{AirPlayAudioTap, AirPlayHttpServer};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AirPlayState {
    pub airplay_audio_tap: Arc<AirPlayAudioTap>,
    pub airplay_stream_generation: Arc<AtomicU64>,
    pub airplay_audience_active: Arc<AtomicBool>,
    pub airplay_control_refresh_token: Arc<AtomicU64>,
    pub airplay_http_server: Arc<Mutex<Option<AirPlayHttpServer>>>,
    pub airplay_local_output_suppressed: Arc<AtomicBool>,
}

impl AirPlayState {
    pub fn new(airplay_audio_tap: Arc<AirPlayAudioTap>) -> Self {
        Self {
            airplay_audio_tap,
            airplay_stream_generation: Arc::new(AtomicU64::new(1)),
            airplay_audience_active: Arc::new(AtomicBool::new(false)),
            airplay_control_refresh_token: Arc::new(AtomicU64::new(0)),
            airplay_http_server: Arc::new(Mutex::new(None)),
            airplay_local_output_suppressed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn test_fixture() -> Self {
        Self::new(Arc::new(AirPlayAudioTap::new(4)))
    }
}
