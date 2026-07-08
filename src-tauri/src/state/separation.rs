use crate::services::separation::SeparationStatusSnapshot;
use crate::separator::model::LoadedModel;
use crate::separator::model_cache::ModelCache;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct SeparationState {
    pub separation_statuses: Arc<Mutex<HashMap<String, SeparationStatusSnapshot>>>,
    pub separator_model_cache: Arc<Mutex<ModelCache<LoadedModel>>>,
    pub batch_running: Arc<AtomicBool>,
    pub batch_cancel: Arc<AtomicBool>,
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
        }
    }

    pub fn test_fixture() -> Self {
        Self::new()
    }
}
