//! Real published-source acceptance for the streaming-source consumer.
//!
//! This test uses the production trust root and the published default
//! distribution URL. It refreshes the signed index, downloads and extracts the
//! pinned Node runtime archive for the host platform, installs the latest
//! NetEase release, starts its worker, and asks it to describe itself.
//!
//! It is ignored by default because it needs network access and downloads tens
//! of megabytes. Run it explicitly on a real host:
//!
//! ```text
//! cargo test -q --test live_streaming_source_consumer -- --ignored --nocapture
//! ```
//!
//! On Windows this is the acceptance path for #579: the pinned runtime entry is
//! declared with `/` while the archive extractor works on `Path`, and the
//! worker cannot start unless `node.exe` was extracted and sealed.

use openkara_lib::source_host::{HostConfig, SourceHost, SourceState};
use serde_json::json;

#[tokio::test]
#[ignore = "needs network access to the published signed source index and pinned Node runtime"]
async fn published_source_installs_and_starts_with_production_trust() {
    let root = tempfile::tempdir().unwrap();
    let config = HostConfig::production(
        root.path().to_path_buf(),
        root.path().to_path_buf(),
        env!("CARGO_PKG_VERSION").into(),
    );
    let host = SourceHost::new(config).await.unwrap();

    let refreshed = host.refresh().await.unwrap();
    let netease = refreshed
        .iter()
        .find(|snapshot| snapshot.source_id == "netease")
        .expect("published signed index must describe the NetEase source");
    assert_eq!(netease.state, SourceState::NotInstalled);
    assert!(
        netease.available_version.is_some(),
        "published signed index must offer a NetEase release"
    );

    // `install` selects from the releases this host can actually run, which is
    // a narrower set than `available_version` (that one only drops revoked
    // releases). Assert what the install produced instead of re-deriving the
    // selector here, so a legal upstream release that is not compatible with
    // this host cannot turn this gate red.
    let installed = host.install("netease", None).await.unwrap();
    let version = installed
        .installed_version
        .clone()
        .expect("published source must install a compatible release");
    let manifest = installed
        .manifest
        .clone()
        .expect("an installed release carries its signed manifest");
    assert_eq!(manifest.source_id, "netease");
    assert_eq!(manifest.version, version);
    assert!(installed.runtime_ready);

    let enabled = host.set_enabled("netease", true, false).await.unwrap();
    assert!(enabled.enabled);
    assert_eq!(enabled.state, SourceState::Ready);

    let description = host
        .request("netease", "describe", json!({}))
        .await
        .unwrap();

    // Print a fixed summary plus non-sensitive fields only. The session account
    // block can carry a real display name when this runs on a signed-in
    // developer machine, and `--nocapture` writes stdout to a public CI log.
    println!(
        "published source consumer acceptance: source=netease version={version} runtime_ready={} state={:?} session={} capabilities={}",
        enabled.runtime_ready,
        enabled.state,
        description["session"]["state"],
        description["capabilities"].as_array().map_or(0, Vec::len),
    );

    assert_eq!(
        description["name"], manifest.name,
        "the running worker must report the signed manifest name"
    );
    let mut reported: Vec<&str> = description["capabilities"]
        .as_array()
        .expect("describe must report capabilities")
        .iter()
        .map(|value| value.as_str().expect("capabilities are strings"))
        .collect();
    let mut declared: Vec<&str> = manifest.capabilities.iter().map(String::as_str).collect();
    reported.sort_unstable();
    declared.sort_unstable();
    assert_eq!(
        reported, declared,
        "the worker must advertise the capabilities the signed manifest declares"
    );
    assert!(
        declared.contains(&"search"),
        "the accepted source must still be able to search"
    );
    assert_eq!(description["session"]["state"], "signed_out");
    assert!(description["session"]["account"].is_null());
}
