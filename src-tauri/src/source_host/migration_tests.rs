use super::test_support::{
    fixture_manifest, seed_node_install, write_package_tree, RegistryFixture, NODE_ERA_OWNER,
};
use super::*;
use serde_json::json;
use std::{fs, path::Path};

fn config(root: &Path) -> HostConfig {
    HostConfig::fixture(root.to_path_buf(), "0.17.0".into())
}

fn engine_manifest(registry: &RegistryFixture) -> SourceManifest {
    let mut manifest = fixture_manifest("fixture", "2.0.0");
    manifest.package_id = registry.package_id("fixture");
    manifest
}

#[tokio::test]
async fn the_first_launch_marks_node_installs_as_needing_replacement_and_deletes_their_files() {
    let root = tempfile::tempdir().unwrap();
    let data_root = root.path().join("streaming-sources");
    let library = root.path().join("library.db");
    fs::write(&library, b"songs, identities and origin stamps").unwrap();
    let node = seed_node_install(&data_root);
    let registry = RegistryFixture::new("org.example.registry").await;
    let registry_tree = data_root.join("packages").join("d".repeat(64));
    write_package_tree(&registry_tree, &engine_manifest(&registry));

    let host = SourceHost::new(config(root.path())).await.unwrap();

    let snapshot = host.snapshot("fixture").unwrap();
    assert_eq!(snapshot.state, SourceState::UpdateRequired);
    assert_eq!(snapshot.source_id, "fixture");
    assert_eq!(snapshot.name, "Deterministic Streaming Source");
    assert_eq!(snapshot.installed_version, None);
    assert_eq!(snapshot.staged_version, None);
    assert_eq!(snapshot.pinned_version, None);
    assert_eq!(snapshot.available_version, None);
    assert!(!snapshot.enabled);
    assert!(!snapshot.loaded);
    assert_eq!(
        snapshot.last_error.map(|error| error.code).as_deref(),
        Some("replacement_required")
    );
    assert!(!node.runtimes.exists(), "the Node runtime is deleted");
    assert!(
        !node.signed_index.exists(),
        "the cached signed index is deleted"
    );
    for package in &node.packages {
        assert!(!package.exists(), "Node package trees are deleted");
    }
    assert!(registry_tree.exists(), "embedded-engine packages stay");
    assert_eq!(
        fs::read(&library).unwrap(),
        b"songs, identities and origin stamps",
        "the migration never touches the library"
    );
    assert_eq!(
        host.replacement_notice(),
        vec![RetiredSource {
            source_id: "fixture".into(),
            name: "Deterministic Streaming Source".into(),
            version: "1.0.0".into(),
        }]
    );
    assert_eq!(
        host.install("fixture", None, false).await.unwrap_err().code,
        "replacement_required",
        "the old path never reinstalls a retired source"
    );
    assert_eq!(
        host.retry("fixture").await.unwrap_err().code,
        "replacement_required"
    );
}

#[tokio::test]
async fn the_migration_runs_once_and_the_notice_shows_until_dismissed() {
    let root = tempfile::tempdir().unwrap();
    seed_node_install(&root.path().join("streaming-sources"));
    let host = SourceHost::new(config(root.path())).await.unwrap();
    let mut removed = Vec::new();
    host.remove_retired_credentials(|id, owner| {
        removed.push((id.to_owned(), owner.to_owned()));
        Ok::<_, std::io::Error>(())
    })
    .unwrap();
    assert_eq!(
        removed,
        vec![("fixture".to_owned(), NODE_ERA_OWNER.to_owned())]
    );
    let state_file = root.path().join("streaming-sources/state.json");
    let migrated = fs::read(&state_file).unwrap();
    drop(host);

    let host = SourceHost::new(config(root.path())).await.unwrap();
    host.remove_retired_credentials(|id, _| -> Result<(), std::io::Error> {
        panic!("credentials of {id} were removed twice")
    })
    .unwrap();
    assert_eq!(
        fs::read(&state_file).unwrap(),
        migrated,
        "a second launch changes nothing"
    );
    assert_eq!(
        host.snapshot("fixture").unwrap().state,
        SourceState::UpdateRequired
    );
    assert_eq!(
        host.replacement_notice().len(),
        1,
        "the notice waits for the user"
    );

    host.dismiss_replacement_notice().unwrap();
    assert!(host.replacement_notice().is_empty());
    drop(host);
    let host = SourceHost::new(config(root.path())).await.unwrap();
    assert!(
        host.replacement_notice().is_empty(),
        "the notice shows once"
    );
    assert_eq!(
        host.snapshot("fixture").unwrap().state,
        SourceState::UpdateRequired,
        "dismissing the notice keeps the source marked"
    );
}

#[tokio::test]
async fn a_failed_credential_removal_is_retried_on_the_next_launch() {
    let root = tempfile::tempdir().unwrap();
    seed_node_install(&root.path().join("streaming-sources"));
    let host = SourceHost::new(config(root.path())).await.unwrap();
    host.remove_retired_credentials(|_, _| Err(std::io::Error::other("keychain locked")))
        .unwrap();
    drop(host);

    let host = SourceHost::new(config(root.path())).await.unwrap();
    let mut removed = Vec::new();
    host.remove_retired_credentials(|id, _| {
        removed.push(id.to_owned());
        Ok::<_, std::io::Error>(())
    })
    .unwrap();
    assert_eq!(removed, ["fixture"]);
}

#[tokio::test]
async fn a_retired_source_is_replaced_from_a_registry_under_the_same_source_id() {
    let root = tempfile::tempdir().unwrap();
    seed_node_install(&root.path().join("streaming-sources"));
    let mut registry = RegistryFixture::new("org.example.registry").await;
    registry.publish(&[engine_manifest(&registry)]).await;
    let host = SourceHost::new(config(root.path())).await.unwrap();
    host.add_registry(&registry.url()).await.unwrap();

    let listed = host.market();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].source_id, "fixture");
    assert_eq!(listed[0].installed, None);

    let replaced = host
        .install_package(
            registry.registry_id(),
            &registry.package_id("fixture"),
            None,
            false,
            true,
        )
        .await
        .unwrap();
    assert_eq!(
        replaced.source_id, "fixture",
        "library identities keep matching"
    );
    assert_eq!(replaced.installed_version.as_deref(), Some("2.0.0"));
    assert_eq!(
        replaced.owner_binding,
        "registry:org.example.registry/org.example.registry.fixture"
    );
    assert_eq!(replaced.last_error, None);
    assert!(host.replacement_notice().is_empty());
    host.set_enabled("fixture", true, false).await.unwrap();
    let description = host
        .request("fixture", "describe", json!({}))
        .await
        .unwrap();
    assert_eq!(description["source_id"], "fixture");
    assert_eq!(host.snapshot("fixture").unwrap().state, SourceState::Ready);

    drop(host);
    let host = SourceHost::new(config(root.path())).await.unwrap();
    assert_eq!(
        host.snapshot("fixture")
            .unwrap()
            .installed_version
            .as_deref(),
        Some("2.0.0"),
        "the replacement survives later launches"
    );
}

#[tokio::test]
async fn without_a_registry_replacement_the_source_stays_marked() {
    let root = tempfile::tempdir().unwrap();
    seed_node_install(&root.path().join("streaming-sources"));
    let mut registry = RegistryFixture::new("org.example.registry").await;
    let mut other = engine_manifest(&registry);
    other.source_id = "other".into();
    other.package_id = registry.package_id("other");
    registry.publish(&[other]).await;
    let host = SourceHost::new(config(root.path())).await.unwrap();
    host.add_registry(&registry.url()).await.unwrap();
    host.refresh().await.unwrap();

    let snapshot = host.snapshot("fixture").unwrap();
    assert_eq!(snapshot.state, SourceState::UpdateRequired);
    assert_eq!(
        snapshot.last_error.map(|error| error.code).as_deref(),
        Some("replacement_required")
    );
    assert_eq!(host.replacement_notice().len(), 1);
}
