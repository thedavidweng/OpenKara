use super::types::StreamingCredentials;
use crate::system_credentials::{self, STREAMING_SOURCE_SERVICE};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredStreamingSession {
    owner_binding: String,
    credentials: StreamingCredentials,
}

pub fn load_credentials(
    app_data_dir: &Path,
    source_id: &str,
    owner_binding: &str,
) -> anyhow::Result<Option<StreamingCredentials>> {
    Ok(load_session(app_data_dir, source_id)?
        .filter(|session| session.owner_binding == owner_binding)
        .map(|session| session.credentials))
}

pub fn store_session(
    app_data_dir: &Path,
    source_id: &str,
    owner_binding: &str,
    credentials: StreamingCredentials,
) -> anyhow::Result<()> {
    system_credentials::store_json_in(
        STREAMING_SOURCE_SERVICE,
        app_data_dir,
        source_id,
        &StoredStreamingSession {
            owner_binding: owner_binding.to_owned(),
            credentials,
        },
    )
}

pub fn clear_session(app_data_dir: &Path, source_id: &str) -> anyhow::Result<()> {
    system_credentials::delete_in(STREAMING_SOURCE_SERVICE, app_data_dir, source_id)
}

fn load_session(
    app_data_dir: &Path,
    source_id: &str,
) -> anyhow::Result<Option<StoredStreamingSession>> {
    system_credentials::load_json_in(STREAMING_SOURCE_SERVICE, app_data_dir, source_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_session_is_not_available_to_another_verified_owner() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::env::set_var("OPENKARA_TEST_CREDENTIAL_STORE_DIR", dir.path());
        let credentials =
            StreamingCredentials::from([("opaque_key".into(), "private-value".into())]);
        store_session(
            dir.path(),
            "fixture",
            "publisher/package",
            credentials.clone(),
        )
        .expect("save verified session");
        assert_eq!(
            load_credentials(dir.path(), "fixture", "publisher/package").expect("load"),
            Some(credentials)
        );
        assert_eq!(
            load_credentials(dir.path(), "fixture", "other/package").expect("other owner"),
            None
        );
        clear_session(dir.path(), "fixture").expect("sign out");
        assert_eq!(
            load_credentials(dir.path(), "fixture", "publisher/package").expect("removed"),
            None
        );
        std::env::remove_var("OPENKARA_TEST_CREDENTIAL_STORE_DIR");
    }
}
