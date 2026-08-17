use crate::catalog::StreamingImportSession;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct CatalogState {
    pub import_session: Arc<Mutex<Option<StreamingImportSession>>>,
}

impl CatalogState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn test_fixture() -> Self {
        Self::new()
    }
}
