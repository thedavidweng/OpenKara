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

    // Print only a fixed summary with no worker-controlled bytes. `state` is
    // checked against the allowed literal before printing as a fixed tag, so
    // a malformed worker cannot smuggle an account object or other PII into
    // the state field and have it land in a public log.
    let state_tag = match description["session"].get("state").and_then(Value::as_str) {
        Some("signed_out") => "signed_out",
        Some(_) => "unexpected_state",
        None => "missing_state",
    };
    let capability_count = description["capabilities"].as_array().map_or(0, Vec::len);
    println!(
        "published source consumer acceptance: source=netease version={version} runtime_ready={} state_tag={state_tag} session_validated=false capabilities={capability_count}",
        enabled.runtime_ready,
    );

    assert_valid_worker_description(&description, &manifest);
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

/// Assert the same contract the live acceptance enforces. Shared so the
/// synthetic regression below exercises the exact assertions the real test
/// runs, not a mirrored copy.
fn assert_valid_worker_description(description: &Value, manifest: &SourceManifest) {
    if let Err(message) = check_worker_description(description, manifest) {
        panic!("{message}");
    }
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

    fn show(result: Result<(), String>) -> String {
        match result {
            Ok(()) => "accepted".into(),
            Err(message) => message,
        }
    }

    #[test]
    fn malformed_descriptions_are_rejected_without_echoing_worker_values() {
        let manifest = manifest();
        // Well-formed control: accepted.
        assert_eq!(
            show(check_worker_description(&valid(), &manifest)),
            "accepted"
        );

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
            let failure = show(check_worker_description(&description, &manifest));
            assert_ne!(failure, "accepted", "malformed case accepted: {label}");
            for sentinel in [
                "REAL_ACCOUNT_pii-acct",
                "REAL_DISPLAY_NAME_pii-state",
                "REAL_ACCOUNT_pii-state",
                "REAL_NAME_pii-name",
                "REAL_CAP_pii-cap",
            ] {
                assert!(
                    !failure.contains(sentinel),
                    "rejection for {label} echoes worker value"
                );
            }
        }
    }
}
