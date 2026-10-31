#[cfg(feature = "streaming-sources")]
use crate::catalog::{StreamingImportJob, StreamingImportSession};
#[cfg(feature = "streaming-sources")]
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct CatalogState {
    #[cfg(feature = "streaming-sources")]
    pub import_session: Arc<Mutex<Option<StreamingImportSession>>>,
    /// Progress and cancel handle of the latest Streaming Import.
    #[cfg(feature = "streaming-sources")]
    pub import_job: Arc<Mutex<Option<Arc<StreamingImportJob>>>>,
}

impl CatalogState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn test_fixture() -> Self {
        Self::new()
    }
}
