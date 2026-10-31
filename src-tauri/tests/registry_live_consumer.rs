//! Live production-registry consumer gate for the Source Registry architecture.
//!
//! Contract: exactly one required input, `OPENKARA_LIVE_REGISTRY_URL`, holding
//! the HTTPS URL of a real published Source Registry document. No fallback to
//! the mock registry, no fixture, no skip: when the variable is absent the
//! test fails with a static message before any network or account action.
//! Package and version come from the registry document itself (the latest
//! compatible release the host can run), never from a hardcoded Node-era
//! index or a bundled default. Nothing here creates a registry, a version, a
//! key, or a trust root; the test only consumes what the source producer has
//! already published.
//!
//! The gate deliberately needs network access and a real published artifact,
//! so it is `#[ignore]`d for local runs and executed explicitly on Windows CI:
//!
//! ```text
//! cargo test -q --test integration registry_live_consumer -- --ignored --nocapture
//! ```
//!
//! Windows CI arms the gate only when the repo variable is set; the test
//! itself never skips. A missing or empty URL fails before any network or
//! account action: no env-empty skip, no retry/timeout weakening, no mock
//! substitution.

use openkara_lib::source_host::{
    HostConfig, ProtocolDeclaration, RuntimeRequirement, SourceHost, SourceManifest, SourceState,
};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;

/// The single required configuration input. Everything else (package id,
/// version, origins, integrity) is read from the registry document the URL
/// serves, which is exactly what the user gesture `add_registry` consumes.
const REGISTRY_URL_ENV: &str = "OPENKARA_LIVE_REGISTRY_URL";

fn registry_url() -> String {
    std::env::var(REGISTRY_URL_ENV)
        .map(|value| value.trim().to_owned())
        .ok()
        .filter(|value| !value.is_empty())
        .expect("missing OPENKARA_LIVE_REGISTRY_URL: set it to the published Source Registry document URL")
}

fn production_config(root: &std::path::Path) -> HostConfig {
    // Same constructor the packaged app uses: loopback HTTP is refused, so a
    // mock registry can never satisfy this gate even if one were reachable.
    HostConfig::production(
        root.join("live-data").to_path_buf(),
        root.join("live-resources").to_path_buf(),
        env!("CARGO_PKG_VERSION").to_owned(),
    )
}

/// Validate the untrusted worker `describe` payload using boolean contract
/// checks with constant failure text. Never includes worker bytes in the
/// message, so `assert!(...)` and panic output cannot carry PII into logs.
/// This is the same shape as the 632 offline proof; the live test below and
/// the offline regression below both call this exact helper.
/// NOTE: this helper deliberately mirrors the crate-private lib contract in
/// `source_host::describe` (which an integration target cannot import). If
/// the two drift, the mocked-worker contract test below catches it: the mock
/// now reports the same full capability set the lib asserts.
fn check_worker_description(description: &Value, manifest: &SourceManifest) -> Result<(), String> {
    const FAIL: &str = "worker description failed contract";
    if description["protocol"]["major"].as_u64() != Some(2) {
        return Err(FAIL.into());
    }
    if description["source_id"].as_str() != Some(manifest.source_id.as_str()) {
        return Err(FAIL.into());
    }
    if description["package_id"].as_str() != Some(manifest.package_id.as_str()) {
        return Err(FAIL.into());
    }
    if description["package_version"].as_str() != Some(manifest.version.as_str()) {
        return Err(FAIL.into());
    }
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

// The live test and the offline regression share this output and failure path.
fn accept_description(
    description: &Value,
    manifest: &SourceManifest,
    version: &str,
    output: &mut impl Write,
) {
    if check_worker_description(description, manifest).is_err() {
        panic!("worker description failed contract");
    }
    writeln!(
        output,
        "registry live consumer acceptance: source={} package={} version={} state=Ready session=signed_out capabilities={}",
        manifest.source_id,
        manifest.package_id,
        version,
        manifest.capabilities.len(),
    )
    .expect("failed to write acceptance summary");
}

fn live_manifest() -> SourceManifest {
    SourceManifest {
        manifest_version: 1,
        source_id: "live".into(),
        package_id: "org.example.live.fixture".into(),
        publisher: "live-fixture".into(),
        version: "1.0.0".into(),
        minimum_host_version: "0.0.0".into(),
        protocol: ProtocolDeclaration {
            majors: vec![2],
            minor: 0,
        },
        runtime: RuntimeRequirement {
            kind: "openkara-js".into(),
            major: 1,
        },
        entry: "source.mjs".into(),
        name: "Live Fixture".into(),
        description: "offline contract fixture".into(),
        license: "AGPL-3.0-only".into(),
        capabilities: vec!["search".into(), "playlists".into()],
        authentication: Value::Null,
        requires_authentication: vec![],
        resource_origins: vec![],
        media_origins: vec![],
    }
}

fn live_description() -> Value {
    json!({
        "protocol": {"major": 2, "minor": 0},
        "source_id": "live",
        "package_id": "org.example.live.fixture",
        "package_version": "1.0.0",
        "name": "Live Fixture",
        "capabilities": ["playlists", "search"],
        "session": {"state": "signed_out", "account": null},
    })
}

#[test]
fn live_consumer_contract_rejects_malformed_describe_without_echo() {
    // Offline privacy regression on the exact helper the live gate calls:
    // every malformed case carries a PII sentinel that must never appear in
    // the rejection message, and nothing is printed for invalid payloads.
    // A Node-era protocol-1 describe and a signed-in session are refused here
    // the same way they are refused on the live path.
    let manifest = live_manifest();
    let mut output = Vec::new();
    accept_description(&live_description(), &manifest, "1.0.0", &mut output);
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "registry live consumer acceptance: source=live package=org.example.live.fixture version=1.0.0 state=Ready session=signed_out capabilities=2\n"
    );

    let cases: Vec<(&str, Value)> = vec![
        (
            "protocol-1-node-era",
            json!({
                "protocol": {"major": 1, "minor": 0},
                "source_id": "live",
                "package_id": "org.example.live.fixture",
                "package_version": "1.0.0",
                "name": "Live Fixture",
                "capabilities": ["playlists", "search"],
                "session": {"state": "signed_out", "account": null},
            }),
        ),
        (
            "state-object-with-account",
            json!({
                "protocol": {"major": 2, "minor": 0},
                "source_id": "live",
                "package_id": "org.example.live.fixture",
                "package_version": "1.0.0",
                "name": "Live Fixture",
                "capabilities": ["playlists", "search"],
                "session": {"state": {"pii": "REAL_ACCOUNT_pii-state"}, "account": null},
            }),
        ),
        (
            "state-unknown-string",
            json!({
                "protocol": {"major": 2, "minor": 0},
                "source_id": "live",
                "package_id": "org.example.live.fixture",
                "package_version": "1.0.0",
                "name": "Live Fixture",
                "capabilities": ["playlists", "search"],
                "session": {"state": "REAL_DISPLAY_NAME_pii-state", "account": null},
            }),
        ),
        (
            "missing-state",
            json!({
                "protocol": {"major": 2, "minor": 0},
                "source_id": "live",
                "package_id": "org.example.live.fixture",
                "package_version": "1.0.0",
                "name": "Live Fixture",
                "capabilities": ["playlists", "search"],
                "session": {"account": null},
            }),
        ),
        (
            "missing-account",
            json!({
                "protocol": {"major": 2, "minor": 0},
                "source_id": "live",
                "package_id": "org.example.live.fixture",
                "package_version": "1.0.0",
                "name": "Live Fixture",
                "capabilities": ["playlists", "search"],
                "session": {"state": "signed_out"},
            }),
        ),
        (
            "non-null-account",
            json!({
                "protocol": {"major": 2, "minor": 0},
                "source_id": "live",
                "package_id": "org.example.live.fixture",
                "package_version": "1.0.0",
                "name": "Live Fixture",
                "capabilities": ["playlists", "search"],
                "session": {"state": "signed_out", "account": {"id": "REAL_ACCOUNT_pii-acct"}},
            }),
        ),
        (
            "wrong-name",
            json!({
                "protocol": {"major": 2, "minor": 0},
                "source_id": "live",
                "package_id": "org.example.live.fixture",
                "package_version": "1.0.0",
                "name": "REAL_NAME_pii-name",
                "capabilities": ["playlists", "search"],
                "session": {"state": "signed_out", "account": null},
            }),
        ),
        (
            "extra-capability",
            json!({
                "protocol": {"major": 2, "minor": 0},
                "source_id": "live",
                "package_id": "org.example.live.fixture",
                "package_version": "1.0.0",
                "name": "Live Fixture",
                "capabilities": ["playlists", "search", "REAL_CAP_pii-cap"],
                "session": {"state": "signed_out", "account": null},
            }),
        ),
        (
            "missing-search",
            json!({
                "protocol": {"major": 2, "minor": 0},
                "source_id": "live",
                "package_id": "org.example.live.fixture",
                "package_version": "1.0.0",
                "name": "Live Fixture",
                "capabilities": ["playlists"],
                "session": {"state": "signed_out", "account": null},
            }),
        ),
        (
            "package-substitution",
            json!({
                "protocol": {"major": 2, "minor": 0},
                "source_id": "live",
                "package_id": "REAL_PKG_pii-substituted",
                "package_version": "1.0.0",
                "name": "Live Fixture",
                "capabilities": ["playlists", "search"],
                "session": {"state": "signed_out", "account": null},
            }),
        ),
    ];
    for (label, description) in cases {
        let mut output = Vec::new();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            accept_description(&description, &manifest, "1.0.0", &mut output);
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
}

#[tokio::test]
#[ignore = "needs a published production Source Registry: set OPENKARA_LIVE_REGISTRY_URL to run"]
async fn registry_live_consumer_installs_starts_and_describes_with_verified_identity() {
    // Fail-closed config read: missing or empty input fails before any host,
    // network, or account action. No skip, no fixture, no invented default.
    let url = registry_url();
    assert!(
        url.starts_with("https://"),
        "the live registry URL must be a production HTTPS address"
    );

    let root = tempfile::tempdir().expect("live consumer needs an isolated state dir");
    let host = SourceHost::new(production_config(root.path()))
        .await
        .expect("production host must open");
    assert!(
        host.snapshots().is_empty(),
        "live consumer starts from fresh isolated state"
    );

    // The explicit user gesture: add the real registry. Adding installs
    // nothing by itself; the document supplies package identity.
    let status = host
        .add_registry(&url)
        .await
        .expect("the published registry document must fetch and validate");
    assert!(status.refreshed_at.is_some());
    assert_eq!(status.last_error, None);

    let market = host.market();
    assert!(
        !market.is_empty(),
        "the published registry must list at least one package"
    );
    let package = market
        .iter()
        .find(|entry| {
            entry.registry_id == status.registry_id
                && entry.versions.iter().any(|version| {
                    version
                        .capabilities
                        .iter()
                        .any(|capability| capability == "search")
                })
        })
        .expect("the published registry must offer a search-capable package");
    let source_id = package.source_id.clone();
    let registry_id = package.registry_id.clone();
    let package_id = package.package_id.clone();

    // Install selects the latest release THIS host can run (protocol 2,
    // openkara-js 1, minimum host version), asserting what was produced
    // rather than re-deriving the selector, exactly like the 632 proof.
    let installed = host
        .install_package(&registry_id, &package_id, None, false, false)
        .await
        .expect("the published package must install a compatible release");
    let version = installed
        .installed_version
        .clone()
        .expect("the published package must install a compatible release");
    let manifest = installed
        .manifest
        .clone()
        .expect("an installed release carries its signed registry manifest");
    assert_eq!(manifest.source_id, source_id);
    assert_eq!(manifest.package_id, package_id);
    assert_eq!(manifest.version, version);
    assert!(manifest.protocol.majors.contains(&2));
    assert_eq!(manifest.runtime.kind, "openkara-js");
    assert_eq!(manifest.runtime.major, 1);
    assert!(manifest
        .capabilities
        .iter()
        .any(|capability| capability == "search"));
    assert!(!installed.enabled);

    let enabled = host
        .set_enabled(&source_id, true, false)
        .await
        .expect("the installed source must enable");
    assert!(enabled.enabled);
    assert_eq!(enabled.state, SourceState::Ready);

    let description = host
        .request(&source_id, "describe", json!({"cursor": null}))
        .await
        .expect("the installed source must answer describe from QuickJS");
    assert!(
        host.snapshot(&source_id)
            .expect("installed source must snapshot")
            .running,
        "a served describe proves the source runs in the embedded engine"
    );
    accept_description(&description, &manifest, &version, &mut std::io::stdout());

    let data_root: PathBuf = root.path().join("live-data/streaming-sources/packages");
    assert!(
        data_root.join(".verified-files.json").is_file()
            || std::fs::read_dir(&data_root)
                .expect("installed packages land under the data root")
                .filter_map(Result::ok)
                .any(|entry| entry.path().join(".verified-files.json").is_file()),
        "the install seals its package directory"
    );
    drop(host);
    let host = SourceHost::new(production_config(root.path()))
        .await
        .expect("production host must reopen");
    assert!(
        host.registries()
            .iter()
            .any(|entry| entry.registry_id == registry_id),
        "added registries persist"
    );
    assert_eq!(
        host.snapshot(&source_id)
            .expect("installed source must persist")
            .installed_version
            .as_deref(),
        Some(version.as_str())
    );
}
