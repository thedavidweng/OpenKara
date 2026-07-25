use crate::separator::model::LoadedModel;
use crate::separator::model_cache::ModelCache;
use crate::services::separation::SeparationStatusSnapshot;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct SeparationState {
    pub separation_statuses: Arc<Mutex<HashMap<String, SeparationStatusSnapshot>>>,
    pub separator_model_cache: Arc<Mutex<ModelCache<LoadedModel>>>,
    pub batch_running: Arc<AtomicBool>,
    pub batch_cancel: Arc<AtomicBool>,
    /// Per-song cancellation flags (song_hash → flag). A running job registers
    /// its flag on start and removes it on exit; `cancel_separation` sets a
    /// flag to request an early return from the chunk loop.
    pub separation_cancels: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    /// The song currently being processed by the batch loop, so batch cancel
    /// can flag it and stop mid-song instead of after the current track.
    pub batch_current_song: Arc<Mutex<Option<String>>>,
}

impl Default for SeparationState {
    fn default() -> Self {
        Self::new()
    }
}

impl SeparationState {
    pub fn new() -> Self {
        Self {
            separation_statuses: Arc::new(Mutex::new(HashMap::new())),
            separator_model_cache: Arc::new(Mutex::new(ModelCache::default())),
            batch_running: Arc::new(AtomicBool::new(false)),
            batch_cancel: Arc::new(AtomicBool::new(false)),
            separation_cancels: Arc::new(Mutex::new(HashMap::new())),
            batch_current_song: Arc::new(Mutex::new(None)),
        }
    }

    pub fn test_fixture() -> Self {
        Self::new()
    }
}
