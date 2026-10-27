use super::types::StreamingCredentials;
use crate::system_credentials::{self, STREAMING_SOURCE_SERVICE};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredStreamingSession {
    owner_binding: String,
    credentials: StreamingCredentials,
}

/// Loads the session that belongs to the package identity. Sessions predate
/// the registry identity when they sit under the source id account; the first
/// matching read moves such a record to the identity account.
pub fn load_credentials(
    app_data_dir: &Path,
    source_id: &str,
    owner_binding: &str,
) -> anyhow::Result<Option<StreamingCredentials>> {
    if let Some(session) = load_session(app_data_dir, owner_binding)? {
        return Ok((session.owner_binding == owner_binding).then_some(session.credentials));
    }
    let Some(session) = load_session(app_data_dir, source_id)?
        .filter(|session| session.owner_binding == owner_binding)
    else {
        return Ok(None);
    };
    if store_session(app_data_dir, owner_binding, session.credentials.clone()).is_ok() {
        let _ = system_credentials::delete_in(STREAMING_SOURCE_SERVICE, app_data_dir, source_id);
    }
    Ok(Some(session.credentials))
}

/// Stores a session under the package's owner binding: `registry:<registry_id>/<package_id>`
/// for a registry package, or `publisher/package_id` for a signed-index release.
pub fn store_session(
    app_data_dir: &Path,
    owner_binding: &str,
    credentials: StreamingCredentials,
) -> anyhow::Result<()> {
    system_credentials::store_json_in(
        STREAMING_SOURCE_SERVICE,
        app_data_dir,
        owner_binding,
        &StoredStreamingSession {
            owner_binding: owner_binding.to_owned(),
            credentials,
        },
    )
}

/// Removes the source's session from both the identity account and the legacy
/// source id account.
pub fn clear_session(
    app_data_dir: &Path,
    source_id: &str,
    owner_binding: Option<&str>,
) -> anyhow::Result<()> {
    if let Some(owner_binding) = owner_binding {
        system_credentials::delete_in(STREAMING_SOURCE_SERVICE, app_data_dir, owner_binding)?;
    }
    system_credentials::delete_in(STREAMING_SOURCE_SERVICE, app_data_dir, source_id)
}

/// Writes a session under the legacy source id account, as releases before the
/// registry identity did.
#[cfg(test)]
pub(crate) fn store_legacy_session(
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

fn load_session(
    app_data_dir: &Path,
    account: &str,
) -> anyhow::Result<Option<StoredStreamingSession>> {
    system_credentials::load_json_in(STREAMING_SOURCE_SERVICE, app_data_dir, account)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_live_under_the_registry_identity_and_stay_separate() {
        let dir = tempfile::tempdir().expect("temp dir");
        let credentials =
            StreamingCredentials::from([("opaque_key".into(), "private-value".into())]);
        store_session(
            dir.path(),
            "registry:org.example/fixture",
            credentials.clone(),
        )
        .expect("save verified session");
        assert_eq!(
            load_credentials(dir.path(), "fixture", "registry:org.example/fixture").expect("load"),
            Some(credentials)
        );
        assert_eq!(
            load_credentials(dir.path(), "fixture", "registry:org.example/other")
                .expect("other identity"),
            None
        );
        clear_session(dir.path(), "fixture", Some("registry:org.example/fixture"))
            .expect("sign out");
        assert_eq!(
            load_credentials(dir.path(), "fixture", "registry:org.example/fixture")
                .expect("removed"),
            None
        );
    }

    #[test]
    fn the_legacy_source_id_key_is_read_once_and_migrated() {
        let dir = tempfile::tempdir().expect("temp dir");
        let credentials =
            StreamingCredentials::from([("opaque_key".into(), "legacy-value".into())]);
        store_legacy_session(
            dir.path(),
            "fixture",
            "registry:org.example/fixture",
            credentials.clone(),
        )
        .expect("legacy save");

        assert_eq!(
            load_credentials(dir.path(), "fixture", "registry:org.example/other")
                .expect("other identity"),
            None,
            "a legacy record owned by another identity is not served"
        );
        assert!(
            load_session(dir.path(), "fixture")
                .expect("legacy kept")
                .is_some(),
            "a refused read keeps the legacy record"
        );

        assert_eq!(
            load_credentials(dir.path(), "fixture", "registry:org.example/fixture")
                .expect("migrated load"),
            Some(credentials.clone())
        );
        assert!(
            load_session(dir.path(), "fixture")
                .expect("legacy gone")
                .is_none(),
            "the legacy key is retired after the read"
        );
        assert_eq!(
            load_credentials(dir.path(), "fixture", "registry:org.example/fixture")
                .expect("identity load"),
            Some(credentials),
            "later reads come from the identity key"
        );
    }

    #[test]
    fn clearing_a_session_removes_the_legacy_key_too() {
        let dir = tempfile::tempdir().expect("temp dir");
        let credentials =
            StreamingCredentials::from([("opaque_key".into(), "private-value".into())]);
        store_session(
            dir.path(),
            "registry:org.example/fixture",
            credentials.clone(),
        )
        .expect("save");
        store_legacy_session(
            dir.path(),
            "fixture",
            "registry:org.example/fixture",
            credentials,
        )
        .expect("legacy save");
        clear_session(dir.path(), "fixture", Some("registry:org.example/fixture"))
            .expect("sign out");
        assert_eq!(
            load_credentials(dir.path(), "fixture", "registry:org.example/fixture").expect("load"),
            None
        );
        assert!(load_session(dir.path(), "fixture")
            .expect("legacy gone")
            .is_none());
    }
}
