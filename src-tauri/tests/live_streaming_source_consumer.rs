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

use openkara_lib::source_host::{
    HostConfig, ProtocolDeclaration, RuntimeRequirement, SourceHost, SourceManifest, SourceState,
};
use serde_json::{json, Value};
use std::io::Write;

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

    accept_description(
        &description,
        &manifest,
        &version,
        enabled.runtime_ready,
        &mut std::io::stdout(),
    );
}

/// Validate the untrusted worker `describe` payload using only boolean
/// contract checks with constant failure text. Never includes worker bytes
/// in the message, so `assert!(... , "{message}")` and panic output cannot
/// carry PII into logs.
fn check_worker_description(description: &Value, manifest: &SourceManifest) -> Result<(), String> {
    const FAIL: &str = "worker description failed contract";
    if description["name"].as_str() != Some(manifest.name.as_str()) {
        return Err(FAIL.into());
    }
    let mut reported: Vec<&str> = description["capabilities"]
        .as_array()
        .ok_or_else(|| FAIL.to_string())?
        .iter()
        .map(|value| value.as_str().ok_or_else(|| FAIL.to_string()))
        .collect::<Result<Vec<&str>, String>>()?;
    let mut declared: Vec<&str> = manifest.capabilities.iter().map(String::as_str).collect();
    reported.sort_unstable();
    declared.sort_unstable();
    if reported != declared {
        return Err(FAIL.into());
    }
    if !declared.contains(&"search") {
        return Err(FAIL.into());
    }
    if description["session"].get("state").and_then(Value::as_str) != Some("signed_out") {
        return Err(FAIL.into());
    }
    match description["session"].get("account") {
        Some(Value::Null) => Ok(()),
        _ => Err(FAIL.into()),
    }
}

// Both the live test and offline regression use this exact output and failure path.
fn accept_description(
    description: &Value,
    manifest: &SourceManifest,
    version: &str,
    runtime_ready: bool,
    output: &mut impl Write,
) {
    if check_worker_description(description, manifest).is_err() {
        panic!("worker description failed contract");
    }
    writeln!(output, "published source consumer acceptance: source=netease version={version} runtime_ready={runtime_ready} state=Ready session=signed_out capabilities={}", manifest.capabilities.len()).expect("failed to write acceptance summary");
}

#[cfg(test)]
mod malformed {
    use super::*;

    fn protocol() -> ProtocolDeclaration {
        ProtocolDeclaration {
            majors: vec![1],
            minor: 0,
        }
    }

    fn runtime() -> RuntimeRequirement {
        RuntimeRequirement {
            kind: "node".into(),
            major: 22,
        }
    }

    fn manifest() -> SourceManifest {
        SourceManifest {
            manifest_version: 1,
            source_id: "netease".into(),
            package_id: "netease".into(),
            publisher: "openkara".into(),
            version: "1.0.1".into(),
            minimum_host_version: "0.0.0".into(),
            protocol: protocol(),
            runtime: runtime(),
            entry: "index.js".into(),
            name: "NetEase Cloud Music".into(),
            description: "test manifest".into(),
            license: "test".into(),
            capabilities: vec!["search".into(), "preview".into()],
            authentication: Value::Null,
            requires_authentication: vec![],
            resource_origins: vec![],
            media_origins: vec![],
        }
    }

    fn valid() -> Value {
        json!({
            "name": "NetEase Cloud Music",
            "capabilities": ["search", "preview"],
            "session": {"state": "signed_out", "account": null},
        })
    }

    #[test]
    fn malformed_descriptions_are_rejected_without_echoing_worker_values() {
        let manifest = manifest();
        let mut output = Vec::new();
        accept_description(&valid(), &manifest, "1.0.1", true, &mut output);
        assert_eq!(String::from_utf8(output).unwrap(), "published source consumer acceptance: source=netease version=1.0.1 runtime_ready=true state=Ready session=signed_out capabilities=2\n");

        // Each malformed case carries a PII sentinel that must never appear in
        // the rejection message. The rejection uses constant text only.
        let cases: Vec<(&str, Value)> = vec![
            (
                "state-object-with-account",
                json!({
                    "name": "NetEase Cloud Music",
                    "capabilities": ["search", "preview"],
                    "session": {"state": {"pii": "REAL_ACCOUNT_pii-state"}, "account": null},
                }),
            ),
            (
                "state-unknown-string",
                json!({
                    "name": "NetEase Cloud Music",
                    "capabilities": ["search", "preview"],
                    "session": {"state": "REAL_DISPLAY_NAME_pii-state", "account": null},
                }),
            ),
            (
                "missing-state",
                json!({
                    "name": "NetEase Cloud Music",
                    "capabilities": ["search", "preview"],
                    "session": {"account": null},
                }),
            ),
            (
                "missing-account",
                json!({
                    "name": "NetEase Cloud Music",
                    "capabilities": ["search", "preview"],
                    "session": {"state": "signed_out"},
                }),
            ),
            (
                "non-null-account",
                json!({
                    "name": "NetEase Cloud Music",
                    "capabilities": ["search", "preview"],
                    "session": {"state": "signed_out", "account": {"id": "REAL_ACCOUNT_pii-acct"}},
                }),
            ),
            (
                "wrong-name",
                json!({
                    "name": "REAL_NAME_pii-name",
                    "capabilities": ["search", "preview"],
                    "session": {"state": "signed_out", "account": null},
                }),
            ),
            (
                "extra-capability",
                json!({
                    "name": "NetEase Cloud Music",
                    "capabilities": ["search", "REAL_CAP_pii-cap"],
                    "session": {"state": "signed_out", "account": null},
                }),
            ),
            (
                "missing-search",
                json!({
                    "name": "NetEase Cloud Music",
                    "capabilities": ["preview"],
                    "session": {"state": "signed_out", "account": null},
                }),
            ),
        ];
        for (label, description) in cases {
            let mut output = Vec::new();
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                accept_description(&description, &manifest, "1.0.1", true, &mut output);
            }))
            .expect_err("malformed description must panic");
            let message = panic
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
                .expect("panic must have a string message");
            assert_eq!(
                message, "worker description failed contract",
                "case {label}"
            );
            assert!(
                output.is_empty(),
                "invalid description must produce no summary: {label}"
            );
        }
        // Equal capability sets isolate the mandatory search guard.
        let mut no_search_manifest = manifest.clone();
        no_search_manifest.capabilities = vec!["preview".into()];
        let mut no_search = valid();
        no_search["capabilities"] = json!(["preview"]);
        let mut output = Vec::new();
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            accept_description(&no_search, &no_search_manifest, "1.0.1", true, &mut output);
        }))
        .is_err());
        assert!(output.is_empty());
    }
}
