use crate::commands::remote_library::{RemoteAuthSession, UploadStatusSnapshot};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct RemoteState {
    pub remote_auth_sessions: Arc<Mutex<HashMap<String, RemoteAuthSession>>>,
    pub remote_upload_statuses: Arc<Mutex<HashMap<String, UploadStatusSnapshot>>>,
}

impl RemoteState {
    pub fn new() -> Self {
        Self {
            remote_auth_sessions: Arc::new(Mutex::new(HashMap::new())),
            remote_upload_statuses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn test_fixture() -> Self {
        Self::new()
    }
}
