use crate::catalog::{StreamingImportJob, StreamingImportSession};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct CatalogState {
    pub import_session: Arc<Mutex<Option<StreamingImportSession>>>,
    /// Progress and cancel handle of the latest Streaming Import.
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
