use super::types::{StreamingCredentials, StreamingSessionSnapshot};
use crate::system_credentials::{self, STREAMING_SOURCE_SERVICE};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredStreamingSession {
    credentials: StreamingCredentials,
    display_name: Option<String>,
}

pub fn load_credentials(
    app_data_dir: &Path,
    source_id: &str,
) -> anyhow::Result<Option<StreamingCredentials>> {
    Ok(load_session(app_data_dir, source_id)?.map(|session| session.credentials))
}

pub fn load_session_snapshot(
    app_data_dir: &Path,
    source_id: &str,
) -> anyhow::Result<StreamingSessionSnapshot> {
    match load_session(app_data_dir, source_id)? {
        Some(session) => Ok(StreamingSessionSnapshot {
            source_id: source_id.to_owned(),
            signed_in: true,
            display_name: session.display_name,
            expired: false,
        }),
        None => Ok(StreamingSessionSnapshot {
            source_id: source_id.to_owned(),
            signed_in: false,
            display_name: None,
            expired: false,
        }),
    }
}

pub fn store_session(
    app_data_dir: &Path,
    source_id: &str,
    credentials: StreamingCredentials,
    display_name: Option<String>,
) -> anyhow::Result<()> {
    system_credentials::store_json_in(
        STREAMING_SOURCE_SERVICE,
        app_data_dir,
        source_id,
        &StoredStreamingSession {
            credentials,
            display_name,
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
    use crate::system_credentials::REMOTE_LIBRARY_SERVICE;

    #[test]
    fn streaming_store_is_distinct_from_repository_store() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::env::set_var("OPENKARA_TEST_CREDENTIAL_STORE_DIR", dir.path());
        store_session(
            dir.path(),
            "netease",
            StreamingCredentials {
                music_u: "music-u".to_owned(),
                csrf: "csrf".to_owned(),
            },
            Some("Ada".to_owned()),
        )
        .expect("store streaming");
        let remote: Option<serde_json::Value> =
            system_credentials::load_json_in(REMOTE_LIBRARY_SERVICE, dir.path(), "netease")
                .expect("load remote");
        assert!(remote.is_none());
        let loaded = load_credentials(dir.path(), "netease")
            .expect("load streaming")
            .expect("present");
        assert_eq!(loaded.music_u, "music-u");
        std::env::remove_var("OPENKARA_TEST_CREDENTIAL_STORE_DIR");
    }

    #[test]
    fn disable_does_not_clear_streaming_credentials() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::env::set_var("OPENKARA_TEST_CREDENTIAL_STORE_DIR", dir.path());
        store_session(
            dir.path(),
            "netease",
            StreamingCredentials {
                music_u: "keep-me".to_owned(),
                csrf: "csrf".to_owned(),
            },
            Some("Ada".to_owned()),
        )
        .expect("store");
        let mut config = crate::config::AppConfig::default();
        crate::catalog::set_online_source_enabled(&mut config, "netease", false).expect("disable");
        let loaded = load_credentials(dir.path(), "netease")
            .expect("load")
            .expect("still present");
        assert_eq!(loaded.music_u, "keep-me");
        std::env::remove_var("OPENKARA_TEST_CREDENTIAL_STORE_DIR");
    }
}
