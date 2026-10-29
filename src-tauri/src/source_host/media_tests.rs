use super::test_support::{fixture_manifest, RegistryFixture};
use super::*;
use serde_json::json;
use std::{collections::BTreeMap, fs};

const MEDIA: &str = "https://streaming-fixture.openkara.test:9443";

/// Installs and enables the repo's engine fixture from a mock registry.
async fn enabled_fixture(root: &std::path::Path, edit: impl FnOnce(&mut HostConfig)) -> SourceHost {
    let mut registry = RegistryFixture::new("org.openkara").await;
    registry
        .publish(&[fixture_manifest("fixture", "1.0.0")])
        .await;
    let mut config = HostConfig::fixture(root.to_path_buf(), "0.17.0".into());
    edit(&mut config);
    let host = SourceHost::new(config).await.unwrap();
    host.add_registry(&registry.url()).await.unwrap();
    host.install_package(
        registry.registry_id(),
        &registry.package_id("fixture"),
        None,
        false,
        false,
    )
    .await
    .unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    host
}

fn descriptor(url: &str) -> ResourceDescriptor {
    ResourceDescriptor {
        url: url.into(),
        headers: BTreeMap::new(),
        expires_at: None,
    }
}

#[tokio::test]
async fn a_fresh_host_lists_nothing_and_installs_nothing() {
    let root = tempfile::tempdir().unwrap();
    let host = SourceHost::new(HostConfig::fixture(
        root.path().to_path_buf(),
        "0.17.0".into(),
    ))
    .await
    .unwrap();
    assert!(host.refresh().await.unwrap().is_empty());
    assert!(host.snapshots().is_empty());
    assert_eq!(
        host.install("fixture", None, false).await.unwrap_err().code,
        "not_found"
    );
}

#[tokio::test]
async fn corrupt_source_state_is_reported_without_being_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let config = HostConfig::fixture(root.path().to_path_buf(), "0.17.0".into());
    fs::create_dir_all(&config.data_root).unwrap();
    let path = config.data_root.join("state.json");
    fs::write(&path, b"corrupt source state").unwrap();
    let error = SourceHost::new(config.clone()).await.err().unwrap();
    let host = SourceHost::unavailable(config, error.clone());
    assert!(host.snapshots().is_empty());
    assert_eq!(host.refresh().await.unwrap_err(), error);
    assert_eq!(
        host.install("fixture", None, false).await.unwrap_err(),
        error
    );
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap_err(),
        error
    );
    assert_eq!(fs::read(path).unwrap(), b"corrupt source state");
}

#[tokio::test]
async fn media_outside_the_declared_public_origins_is_refused_before_fetch() {
    let root = tempfile::tempdir().unwrap();
    let url = format!("{MEDIA}/tone-1.wav");
    let private = format!("{MEDIA}/private.wav");
    let host = enabled_fixture(root.path(), |config| {
        config.add_fixture_resource(
            url.clone(),
            vec!["93.184.216.34".parse().unwrap()],
            b"0123456789".to_vec(),
        );
        config.add_fixture_resource(
            private.clone(),
            vec!["127.0.0.1".parse().unwrap()],
            b"private".to_vec(),
        );
    })
    .await;

    let mut response = host
        .open_media(
            "fixture",
            &descriptor(&url),
            Some((3, Some(5))),
            Cancellation::new(),
        )
        .await
        .unwrap();
    assert_eq!(response.status, 206);
    assert_eq!(response.content_range.as_deref(), Some("bytes 3-5/10"));
    assert_eq!(response.chunk().await.unwrap().unwrap(), b"345");
    assert!(response.chunk().await.unwrap().is_none());

    for refused in [
        "file:///etc/passwd",
        "http://streaming-fixture.openkara.test:9443/tone-1.wav",
        "https://127.0.0.1/tone-1.wav",
        "https://user:pass@streaming-fixture.openkara.test:9443/tone-1.wav",
        private.as_str(),
    ] {
        assert!(
            host.open_media("fixture", &descriptor(refused), None, Cancellation::new())
                .await
                .is_err(),
            "{refused}"
        );
    }
    for (name, value) in [("Authorization", "secret"), ("Host", "localhost")] {
        let mut headed = descriptor(&url);
        headed.headers.insert(name.into(), value.into());
        assert_eq!(
            host.open_media("fixture", &headed, None, Cancellation::new())
                .await
                .err()
                .unwrap()
                .code,
            "invalid_resource",
            "{name}"
        );
    }
}

#[tokio::test]
async fn known_expired_media_is_refused_before_transport() {
    let root = tempfile::tempdir().unwrap();
    let url = format!("{MEDIA}/tone-1.wav");
    let host = enabled_fixture(root.path(), |config| {
        config.add_fixture_resource(
            url.clone(),
            vec!["93.184.216.34".parse().unwrap()],
            b"media".to_vec(),
        );
        config.set_fixture_status(url.clone(), 410);
    })
    .await;
    let mut expired = descriptor(&url);
    expired.expires_at = Some("2000-01-01T00:00:00Z".into());
    assert_eq!(
        host.open_media("fixture", &expired, None, Cancellation::new())
            .await
            .err()
            .unwrap()
            .code,
        "media_expired"
    );
    assert_eq!(
        host.load_resource("fixture", expired, Cancellation::new())
            .await
            .unwrap_err()
            .code,
        "media_expired"
    );
    assert_eq!(
        host.open_media("fixture", &descriptor(&url), None, Cancellation::new())
            .await
            .unwrap()
            .status,
        410
    );
    assert_eq!(
        host.download_media(
            "fixture",
            descriptor(&url),
            root.path().join("expired.wav"),
            Cancellation::new()
        )
        .await
        .unwrap_err()
        .code,
        "media_expired"
    );
}
