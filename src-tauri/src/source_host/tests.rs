use super::*;
use base64::Engine;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::PathBuf};

struct Fixture {
    root: tempfile::TempDir,
    server: mockito::ServerGuard,
    key: Ed25519KeyPair,
    next_key: Ed25519KeyPair,
    generation: u64,
    runtime: PathBuf,
    index: Value,
    assets: BTreeMap<String, Vec<u8>>,
}
impl Fixture {
    async fn new() -> Self {
        let runtime = PathBuf::from(
            std::env::var_os("OPENKARA_SOURCE_TEST_NODE")
                .expect("prepare pinned Node and set OPENKARA_SOURCE_TEST_NODE"),
        );
        let document = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
        let next = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
        Self {
            root: tempfile::tempdir().unwrap(),
            server: mockito::Server::new_async().await,
            key: Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap(),
            next_key: Ed25519KeyPair::from_pkcs8(next.as_ref()).unwrap(),
            generation: 0,
            runtime,
            index: Value::Null,
            assets: BTreeMap::new(),
        }
    }
    fn config(&self) -> HostConfig {
        let mut config = HostConfig::production(
            self.root.path().to_path_buf(),
            self.root.path().to_path_buf(),
            "0.17.0".into(),
        );
        config.index_url = format!("{}/index.json", self.server.url());
        config.trusted_keys = BTreeMap::from([
            (
                "test-current".into(),
                self.key.public_key().as_ref().try_into().unwrap(),
            ),
            (
                "test-next".into(),
                self.next_key.public_key().as_ref().try_into().unwrap(),
            ),
        ]);
        config.runtime_policy = RuntimePolicy::ChannelBundled {
            executable: self.runtime.clone(),
            version: "24.22.1".into(),
            target: config.target.clone(),
        };
        config.allow_test_distribution_http = true;
        config.idle_timeout = std::time::Duration::from_millis(80);
        config.operation_timeout = std::time::Duration::from_millis(300);
        config.backoff_unit = std::time::Duration::from_millis(1);
        config
    }
    fn manifest(&self, version: &str) -> SourceManifest {
        let mut manifest: SourceManifest = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../fixtures/streaming-source/manifest.json"
        )))
        .unwrap();
        manifest.version = version.into();
        manifest
    }
    async fn publish(
        &mut self,
        version: &str,
        worker: &str,
        revoked: Vec<String>,
        sign_key: Option<&Ed25519KeyPair>,
    ) {
        self.server.reset();
        self.generation += 1;
        let manifest = self.manifest(version);
        let mut archive = tar::Builder::new(Vec::new());
        for (path, bytes) in [
            ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
            ("worker.mjs", worker.as_bytes().to_vec()),
            ("package-lock.json", b"{\"lockfileVersion\":3}".to_vec()),
            ("LICENSE", b"AGPL-3.0-only".to_vec()),
            ("NOTICE", b"Fixture".to_vec()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            archive
                .append_data(&mut header, path, bytes.as_slice())
                .unwrap();
        }
        let bytes = archive.into_inner().unwrap();
        let digest = digest(&bytes);
        self.assets.clear();
        self.assets
            .insert(format!("/package-{version}.tar"), bytes.clone());
        self.server
            .mock("GET", format!("/package-{version}.tar").as_str())
            .with_body(bytes.clone())
            .create_async()
            .await;
        let index = json!({"schema_version":1,"generation":self.generation,"sources":[{"source_id":"fixture","publisher":"org.openkara","package_id":"org.openkara.fixture","releases":[{"manifest":manifest,"archive":{"url":format!("{}/package-{version}.tar",self.server.url()),"size":bytes.len(),"sha256":digest,"format":"tar"},"runtime":{"version":"24.22.1","targets":{(self.config().target):{"url":"https://nodejs.org/never-fetched.tar.gz","size":1,"sha256":"00".repeat(32),"format":"tar.gz","executable":"node/bin/node"}}}}],"revoked_versions":revoked}]});
        self.index = index.clone();
        let payload = serde_json::to_vec(&index).unwrap();
        let signature = sign_key.unwrap_or(&self.key).sign(&payload);
        let envelope = json!({"key_id":"test-current","payload":base64::engine::general_purpose::STANDARD.encode(payload),"signature":base64::engine::general_purpose::STANDARD.encode(signature.as_ref())});
        self.server
            .mock("GET", "/index.json")
            .with_body(serde_json::to_vec(&envelope).unwrap())
            .create_async()
            .await;
    }
    async fn change_index(&mut self, next: bool, change: impl FnOnce(&mut Value)) {
        self.server.reset();
        self.generation += 1;
        self.index["generation"] = json!(self.generation);
        change(&mut self.index);
        for (path, bytes) in &self.assets {
            self.server
                .mock("GET", path.as_str())
                .with_body(bytes.clone())
                .create_async()
                .await;
        }
        let payload = serde_json::to_vec(&self.index).unwrap();
        let key = if next { &self.next_key } else { &self.key };
        let envelope = json!({"key_id":if next{"test-next"}else{"test-current"},"payload":base64::engine::general_purpose::STANDARD.encode(&payload),"signature":base64::engine::general_purpose::STANDARD.encode(key.sign(&payload).as_ref())});
        self.server
            .mock("GET", "/index.json")
            .with_body(serde_json::to_vec(&envelope).unwrap())
            .create_async()
            .await;
    }
    async fn append_package(&mut self, extra: impl FnOnce(&mut tar::Builder<Vec<u8>>)) {
        let (path, old) = self
            .assets
            .iter()
            .next()
            .map(|(path, bytes)| (path.clone(), bytes.clone()))
            .unwrap();
        let mut archive = tar::Archive::new(old.as_slice());
        let mut output = tar::Builder::new(Vec::new());
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let header = entry.header().clone();
            output.append(&header, &mut entry).unwrap();
        }
        extra(&mut output);
        let bytes = output.into_inner().unwrap();
        let size = bytes.len();
        let sha256 = digest(&bytes);
        self.assets.insert(path, bytes);
        self.change_index(false, |index| {
            index["sources"][0]["releases"][0]["archive"]["size"] = json!(size);
            index["sources"][0]["releases"][0]["archive"]["sha256"] = json!(sha256);
        })
        .await;
    }
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn signed_install_runs_real_source_and_untrusted_update_preserves_it() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    assert_eq!(
        host.refresh().await.unwrap()[0].state,
        SourceState::NotInstalled
    );
    let installed = host.install("fixture", None).await.unwrap();
    assert_eq!(installed.installed_version.as_deref(), Some("1.0.0"));
    assert!(!installed.enabled);
    host.set_enabled("fixture", true, false).await.unwrap();
    let description = host
        .request("fixture", "describe", json!({}))
        .await
        .unwrap();
    assert_eq!(description["name"], "Deterministic Streaming Source");
    let unknown = Ed25519KeyPair::from_seed_unchecked(&[23; 32]).unwrap();
    fixture
        .publish("2.0.0", "process.exit(77)", vec![], Some(&unknown))
        .await;
    assert_eq!(host.refresh().await.unwrap_err().code, "untrusted_index");
    assert_eq!(
        host.snapshot("fixture")
            .unwrap()
            .installed_version
            .as_deref(),
        Some("1.0.0")
    );
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap()["session"]["state"],
        "signed_out"
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn pinned_revoked_release_stays_stopped_after_offline_restart() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    host.set_pin("fixture", Some("1.0.0")).unwrap();
    host.request("fixture", "describe", json!({}))
        .await
        .unwrap();
    let lease = host.acquire_work("fixture", WorkKind::Preview).unwrap();
    let cancellation = lease.cancellation();
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec!["1.0.0".into()],
            None,
        )
        .await;
    host.refresh().await.unwrap();
    assert!(cancellation.is_cancelled());
    drop(lease);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!host.snapshot("fixture").unwrap().running);
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap_err()
            .code,
        "incompatible_plugin"
    );
    drop(host);
    fixture.server.reset();
    let host = SourceHost::new(fixture.config()).await.unwrap();
    assert!(host.refresh().await.is_err());
    let snapshot = host.snapshot("fixture").unwrap();
    assert_eq!(snapshot.state, SourceState::UpdateRequired);
    assert_eq!(snapshot.pinned_version.as_deref(), Some("1.0.0"));
    assert!(snapshot.enabled);
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap_err()
            .code,
        "incompatible_plugin"
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn manual_update_waits_for_work_and_failed_handshake_preserves_usable_release() {
    let mut fixture = Fixture::new().await;
    let worker = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/streaming-source/worker.mjs"
    ));
    fixture.publish("1.0.0", worker, vec![], None).await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    let lease = host.acquire_work("fixture", WorkKind::Import).unwrap();
    fixture.publish("1.1.0", worker, vec![], None).await;
    host.refresh().await.unwrap();
    let staged = host.install("fixture", None).await.unwrap();
    assert_eq!(staged.state, SourceState::WaitingForIdle);
    assert_eq!(staged.installed_version.as_deref(), Some("1.0.0"));
    assert_eq!(staged.staged_version.as_deref(), Some("1.1.0"));
    assert_eq!(
        host.set_enabled("fixture", false, false)
            .await
            .unwrap_err()
            .code,
        "active_work"
    );
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap()["name"],
        "Deterministic Streaming Source"
    );
    drop(lease);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        host.snapshot("fixture")
            .unwrap()
            .installed_version
            .as_deref(),
        Some("1.1.0")
    );
    fixture
        .publish("2.0.0", "process.exit(77)", vec![], None)
        .await;
    host.refresh().await.unwrap();
    assert!(host.install("fixture", None).await.is_err());
    assert_eq!(
        host.snapshot("fixture")
            .unwrap()
            .installed_version
            .as_deref(),
        Some("1.1.0")
    );
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap()["name"],
        "Deterministic Streaming Source"
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn rotated_key_is_accepted_and_owner_claim_cannot_replace_uninstalled_identity() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.uninstall("fixture", false).await.unwrap();
    fixture.change_index(true, |_| {}).await;
    assert_eq!(
        host.refresh().await.unwrap()[0].owner_binding,
        "org.openkara/org.openkara.fixture"
    );
    host.install("fixture", None).await.unwrap();
    assert_eq!(
        host.snapshot("fixture")
            .unwrap()
            .installed_version
            .as_deref(),
        Some("1.0.0")
    );
    host.uninstall("fixture", false).await.unwrap();
    fixture
        .change_index(true, |index| {
            index["sources"][0]["publisher"] = json!("org.attacker");
            index["sources"][0]["releases"][0]["manifest"]["publisher"] = json!("org.attacker");
        })
        .await;
    assert_eq!(host.refresh().await.unwrap_err().code, "owner_mismatch");
    assert_eq!(
        host.snapshot("fixture").unwrap().owner_binding,
        "org.openkara/org.openkara.fixture"
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn bundled_wrong_runtime_and_incompatible_protocol_never_fetch_or_replace_code() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    fixture
        .change_index(false, |index| {
            index["sources"][0]["releases"][0]["runtime"]["version"] = json!("24.22.0");
        })
        .await;
    host.refresh().await.unwrap();
    assert_eq!(
        host.install("fixture", None).await.unwrap_err().code,
        "incompatible_plugin"
    );
    fixture
        .change_index(false, |index| {
            index["sources"][0]["releases"][0]["runtime"]["version"] = json!("24.22.1");
            index["sources"][0]["releases"][0]["manifest"]["protocol"]["majors"] = json!([99]);
        })
        .await;
    host.refresh().await.unwrap();
    assert_eq!(
        host.install("fixture", None).await.unwrap_err().code,
        "incompatible_plugin"
    );
    assert_eq!(
        host.snapshot("fixture")
            .unwrap()
            .installed_version
            .as_deref(),
        Some("1.0.0")
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn real_stdio_multiplexes_progress_errors_cancellation_idle_and_restart() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    let lease = host.acquire_work("fixture", WorkKind::Import).unwrap();
    host.request(
        "fixture",
        "session.restore",
        json!({"credentials":{"fixture_session":"fixture-account-v1"}}),
    )
    .await
    .unwrap();
    let slow = host
        .start_request(
            "fixture",
            "media.resolve",
            json!({"track_id":"3","purpose":"import"}),
        )
        .await
        .unwrap();
    let mut progress = slow.subscribe_progress();
    let listing = host
        .start_request("fixture", "playlists", json!({"cursor":null}))
        .await
        .unwrap();
    assert_eq!(listing.result().await.unwrap()["items"][0]["id"], "42");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), progress.recv())
            .await
            .unwrap()
            .unwrap()["phase"],
        "resolve"
    );
    slow.cancel();
    assert_eq!(slow.result().await.unwrap_err().code, "cancelled");
    assert_eq!(
        host.request("fixture", "tracks.get", json!({"track_ids":["3"]}))
            .await
            .unwrap()["items"][0]["title"],
        "Fixture Night Tone"
    );
    assert_eq!(
        host.request(
            "fixture",
            "playlist.tracks",
            json!({"playlist_id":"wrong","cursor":null})
        )
        .await
        .unwrap_err()
        .code,
        "not_found"
    );
    drop(lease);
    tokio::time::sleep(Duration::from_millis(180)).await;
    assert!(!host.snapshot("fixture").unwrap().running);
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap()["session"]["state"],
        "signed_out"
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn approved_stop_waits_for_lease_acknowledgment_and_never_removes_other_sources_data() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    let lease = host
        .acquire_work("fixture", WorkKind::Authentication)
        .unwrap();
    let token = lease.cancellation();
    let stop = host.clone();
    let pending = tokio::spawn(async move { stop.set_enabled("fixture", false, true).await });
    tokio::time::timeout(Duration::from_secs(3), token.cancelled())
        .await
        .unwrap();
    assert!(host.snapshot("fixture").unwrap().enabled);
    assert!(!pending.is_finished());
    drop(lease);
    assert!(!pending.await.unwrap().unwrap().enabled);
    let library = fixture.root.path().join("library-song.wav");
    fs::write(&library, b"imported library song").unwrap();
    host.uninstall("fixture", false).await.unwrap();
    assert_eq!(fs::read(&library).unwrap(), b"imported library song");
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn downloaded_runtime_is_verified_and_shared_without_system_node() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let mut tar = tar::Builder::new(Vec::new());
    let bytes = fs::read(&fixture.runtime).unwrap();
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    tar.append_data(&mut header, "node/bin/node", bytes.as_slice())
        .unwrap();
    let archive = tar.into_inner().unwrap();
    let size = archive.len();
    let hash = digest(&archive);
    let url = format!("{}/runtime.tar", fixture.server.url());
    fixture.change_index(false,|index|{let target=index["sources"][0]["releases"][0]["runtime"]["targets"].as_object_mut().unwrap().values_mut().next().unwrap();*target=json!({"url":url,"size":size,"sha256":hash,"format":"tar","executable":"node/bin/node"});}).await;
    fixture
        .server
        .mock("GET", "/runtime.tar")
        .with_body(archive)
        .expect(2)
        .create_async()
        .await;
    let mut config = fixture.config();
    config.runtime_policy = RuntimePolicy::Downloaded;
    let host = SourceHost::new(config).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap()["name"],
        "Deterministic Streaming Source"
    );
    #[cfg(target_os = "macos")]
    {
        let root = fixture.config().data_root.join("runtimes");
        for entry in fs::read_dir(root).unwrap() {
            let executable = entry.unwrap().path().join("node/bin/node");
            let output = std::process::Command::new("/usr/bin/xattr")
                .arg("-p")
                .arg("com.apple.quarantine")
                .arg(executable)
                .output()
                .unwrap();
            assert!(
                !output.status.success(),
                "managed downloader introduced quarantine"
            );
        }
    }
    host.set_enabled("fixture", false, false).await.unwrap();
    let digest = fixture.index["sources"][0]["releases"][0]["runtime"]["targets"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()["sha256"]
        .as_str()
        .unwrap();
    let damaged = fixture
        .config()
        .data_root
        .join("runtimes")
        .join(digest)
        .join("node/bin/node");
    fs::write(&damaged, b"damaged runtime").unwrap();
    host.install("fixture", None).await.unwrap();
    assert_eq!(fs::read(damaged).unwrap(), b"damaged runtime");
    host.set_enabled("fixture", true, false).await.unwrap();
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap()["name"],
        "Deterministic Streaming Source"
    );
}

#[tokio::test]
async fn fresh_offline_discovery_and_private_media_never_imply_installation() {
    let root = tempfile::tempdir().unwrap();
    let mut config = HostConfig::production(
        root.path().to_path_buf(),
        root.path().to_path_buf(),
        "0.17.0".into(),
    );
    config.index_url = "https://127.0.0.1/index.json".into();
    let host = SourceHost::new(config).await.unwrap();
    assert_eq!(host.refresh().await.unwrap_err().code, "invalid_resource");
    assert!(host.snapshots().is_empty());
    assert_eq!(
        host.install("fixture", None).await.unwrap_err().code,
        "not_found"
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn repeated_crashes_become_unavailable_until_explicit_retry() {
    let mut fixture = Fixture::new().await;
    let worker = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/streaming-source/worker.mjs"
    ))
    .replace(
        "if (method === \"describe\")",
        "if (method === \"describe\") process.exit(71); if (method === \"describe\")",
    );
    fixture.publish("1.0.0", &worker, vec![], None).await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    for _ in 0..5 {
        assert_eq!(
            host.request("fixture", "describe", json!({}))
                .await
                .unwrap_err()
                .code,
            "source_unavailable"
        );
    }
    assert_eq!(
        host.snapshot("fixture").unwrap().state,
        SourceState::Unavailable
    );
    host.retry("fixture").await.unwrap();
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap_err()
            .code,
        "source_unavailable"
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn media_policy_rejects_private_destinations_and_unapproved_headers_before_fetch() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    for url in [
        "file:///etc/passwd",
        "http://streaming-fixture.openkara.test:9443/tone.wav",
        "https://127.0.0.1/tone.wav",
        "https://user:pass@streaming-fixture.openkara.test:9443/tone.wav",
    ] {
        let descriptor = ResourceDescriptor {
            url: url.into(),
            headers: BTreeMap::new(),
            expires_at: None,
        };
        assert!(host
            .open_media("fixture", &descriptor, None, Cancellation::new())
            .await
            .is_err());
    }
    let descriptor = ResourceDescriptor {
        url: "https://streaming-fixture.openkara.test:9443/tone.wav".into(),
        headers: BTreeMap::from([("Authorization".into(), "secret".into())]),
        expires_at: None,
    };
    assert_eq!(
        host.open_media("fixture", &descriptor, None, Cancellation::new())
            .await
            .err()
            .unwrap()
            .code,
        "invalid_resource"
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn cancelled_update_preserves_code_and_explicit_retry_retries_the_candidate() {
    let mut fixture = Fixture::new().await;
    let worker = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/streaming-source/worker.mjs"
    ));
    fixture.publish("1.0.0", worker, vec![], None).await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    fixture
        .publish(
            "1.1.0",
            &format!("await new Promise(resolve=>setTimeout(resolve,400));\n{worker}"),
            vec![],
            None,
        )
        .await;
    host.refresh().await.unwrap();
    let installer = host.clone();
    let install = tokio::spawn(async move { installer.install("fixture", None).await });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            Instant::now() < deadline,
            "candidate never reached handshake: {:?}",
            host.snapshot("fixture").unwrap()
        );
        if host
            .snapshot("fixture")
            .unwrap()
            .progress
            .is_some_and(|progress| progress.phase == "handshake")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    host.cancel_install("fixture").unwrap();
    assert_eq!(install.await.unwrap().unwrap_err().code, "cancelled");
    assert_eq!(
        host.snapshot("fixture")
            .unwrap()
            .installed_version
            .as_deref(),
        Some("1.0.0")
    );
    assert_eq!(
        host.retry("fixture")
            .await
            .unwrap()
            .installed_version
            .as_deref(),
        Some("1.1.0")
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn current_previous_majors_work_but_upgrade_outside_window_requires_update() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let mut config = fixture.config();
    config.current_protocol = 2;
    let host = SourceHost::new(config).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    host.set_pin("fixture", Some("1.0.0")).unwrap();
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap()["name"],
        "Deterministic Streaming Source"
    );
    drop(host);
    let mut config = fixture.config();
    config.current_protocol = 3;
    let host = SourceHost::new(config).await.unwrap();
    assert_eq!(
        host.snapshot("fixture").unwrap().state,
        SourceState::UpdateRequired
    );
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap_err()
            .code,
        "incompatible_plugin"
    );
    assert!(host.snapshot("fixture").unwrap().enabled);
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn invalid_archive_bytes_and_handshake_identity_never_replace_usable_code() {
    let mut fixture = Fixture::new().await;
    let worker = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/streaming-source/worker.mjs"
    ));
    fixture.publish("1.0.0", worker, vec![], None).await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    fixture.publish("1.1.0", worker, vec![], None).await;
    fixture
        .change_index(false, |index| {
            index["sources"][0]["releases"][0]["archive"]["sha256"] = json!("00".repeat(32));
        })
        .await;
    fixture
        .server
        .mock("GET", "/package-1.1.0.tar")
        .with_body("invalid package")
        .create_async()
        .await;
    host.refresh().await.unwrap();
    assert_eq!(
        host.install("fixture", None).await.unwrap_err().code,
        "invalid_package"
    );
    fixture
        .publish(
            "1.2.0",
            &worker.replace("source_id: manifest.source_id", "source_id: 'stolen'"),
            vec![],
            None,
        )
        .await;
    host.refresh().await.unwrap();
    assert_eq!(
        host.install("fixture", None).await.unwrap_err().code,
        "incompatible_plugin"
    );
    assert_eq!(
        host.snapshot("fixture")
            .unwrap()
            .installed_version
            .as_deref(),
        Some("1.0.0")
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn deadlines_and_concurrency_are_bounded_without_awaiting_handles() {
    let mut fixture = Fixture::new().await;
    let worker = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/streaming-source/worker.mjs"
    ))
    .replace(
        "track.id === \"3\" ? 250 : 5",
        "track.id === \"3\" ? 5000 : 5",
    );
    fixture.publish("1.0.0", &worker, vec![], None).await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    let lease = host.acquire_work("fixture", WorkKind::Import).unwrap();
    host.request(
        "fixture",
        "session.restore",
        json!({"credentials":{"fixture_session":"fixture-account-v1"}}),
    )
    .await
    .unwrap();
    let mut handles = Vec::new();
    for _ in 0..16 {
        handles.push(
            host.start_request(
                "fixture",
                "media.resolve",
                json!({"track_id":"3","purpose":"import"}),
            )
            .await
            .unwrap(),
        );
    }
    assert_eq!(
        host.start_request(
            "fixture",
            "media.resolve",
            json!({"track_id":"3","purpose":"import"})
        )
        .await
        .err()
        .unwrap()
        .code,
        "busy"
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    for handle in handles {
        assert_eq!(
            handle.result().await.unwrap_err().code,
            "source_unavailable"
        );
    }
    assert_eq!(
        host.request("fixture", "tracks.get", json!({"track_ids":["1"]}))
            .await
            .unwrap()["items"][0]["title"],
        "Fixture Morning Tone"
    );
    drop(lease);
}

#[tokio::test]
async fn corrupt_source_state_can_be_reported_without_overwriting_it_or_blocking_local_work() {
    let root = tempfile::tempdir().unwrap();
    let config = HostConfig::production(
        root.path().to_path_buf(),
        root.path().to_path_buf(),
        "0.17.0".into(),
    );
    fs::create_dir_all(&config.data_root).unwrap();
    let path = config.data_root.join("state.json");
    fs::write(&path, b"corrupt source state").unwrap();
    let error = SourceHost::new(config.clone()).await.err().unwrap();
    let host = SourceHost::unavailable(config, error.clone());
    assert!(host.snapshots().is_empty());
    assert_eq!(host.refresh().await.unwrap_err(), error);
    assert_eq!(host.install("fixture", None).await.unwrap_err(), error);
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap_err(),
        error
    );
    assert_eq!(fs::read(path).unwrap(), b"corrupt source state");
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn dependency_heavy_source_installs_but_archive_links_and_install_scripts_do_not_replace_it()
{
    let mut fixture = Fixture::new().await;
    let worker = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/streaming-source/worker.mjs"
    ));
    fixture.publish("1.0.0", worker, vec![], None).await;
    fixture
        .append_package(|archive| {
            for index in 0..700 {
                let mut header = tar::Header::new_gnu();
                header.set_size(1);
                header.set_mode(0o644);
                header.set_cksum();
                archive
                    .append_data(
                        &mut header,
                        format!("node_modules/dependency/lib-{index}.mjs"),
                        b"x".as_slice(),
                    )
                    .unwrap();
            }
        })
        .await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    fixture.publish("1.1.0", worker, vec![], None).await;
    fixture
        .append_package(|archive| {
            let mut header = tar::Header::new_gnu();
            header.set_size(0);
            header.set_mode(0o777);
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_link_name("../../worker.mjs").unwrap();
            header.set_cksum();
            archive
                .append_data(&mut header, "node_modules/unsafe", std::io::empty())
                .unwrap();
        })
        .await;
    host.refresh().await.unwrap();
    assert_eq!(
        host.install("fixture", None).await.unwrap_err().code,
        "invalid_package"
    );
    fixture.publish("1.2.0", worker, vec![], None).await;
    fixture
        .append_package(|archive| {
            let data = b"{\"scripts\":{\"postinstall\":\"touch host\"}}";
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            archive
                .append_data(&mut header, "package.json", data.as_slice())
                .unwrap();
        })
        .await;
    host.refresh().await.unwrap();
    assert_eq!(
        host.install("fixture", None).await.unwrap_err().code,
        "invalid_package"
    );
    assert_eq!(
        host.snapshot("fixture")
            .unwrap()
            .installed_version
            .as_deref(),
        Some("1.0.0")
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn injected_public_transport_still_enforces_origin_headers_and_dns_policy() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let mut config = fixture.config();
    let url = "https://streaming-fixture.openkara.test:9443/tone-1.wav";
    config.add_fixture_resource(
        url.into(),
        vec!["93.184.216.34".parse().unwrap()],
        b"0123456789".to_vec(),
    );
    config.add_fixture_resource(
        "https://streaming-fixture.openkara.test:9443/private.wav".into(),
        vec!["127.0.0.1".parse().unwrap()],
        b"private".to_vec(),
    );
    let host = SourceHost::new(config).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    let descriptor = ResourceDescriptor {
        url: url.into(),
        headers: BTreeMap::new(),
        expires_at: None,
    };
    let mut response = host
        .open_media(
            "fixture",
            &descriptor,
            Some((3, Some(5))),
            Cancellation::new(),
        )
        .await
        .unwrap();
    assert_eq!(response.status, 206);
    assert_eq!(response.content_range.as_deref(), Some("bytes 3-5/10"));
    assert_eq!(response.chunk().await.unwrap().unwrap(), b"345");
    assert!(response.chunk().await.unwrap().is_none());
    let mut private = descriptor.clone();
    private.url = "https://streaming-fixture.openkara.test:9443/private.wav".into();
    assert_eq!(
        host.open_media("fixture", &private, None, Cancellation::new())
            .await
            .err()
            .unwrap()
            .code,
        "invalid_resource"
    );
    let mut header = descriptor;
    header.headers.insert("Host".into(), "localhost".into());
    assert_eq!(
        host.open_media("fixture", &header, None, Cancellation::new())
            .await
            .err()
            .unwrap()
            .code,
        "invalid_resource"
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn auth_cancel_and_sign_out_remain_available_at_normal_request_limit() {
    let mut fixture = Fixture::new().await;
    let worker = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/streaming-source/worker.mjs"
    ))
    .replace(
        "track.id === \"3\" ? 250 : 5",
        "track.id === \"3\" ? 5000 : 5",
    );
    fixture.publish("1.0.0", &worker, vec![], None).await;
    let mut config = fixture.config();
    config.operation_timeout = Duration::from_secs(5);
    let host = SourceHost::new(config).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    let lease = host
        .acquire_work("fixture", WorkKind::Authentication)
        .unwrap();
    host.request(
        "fixture",
        "session.restore",
        json!({"credentials":{"fixture_session":"fixture-account-v1"}}),
    )
    .await
    .unwrap();
    let auth = host
        .request("fixture", "auth.start", json!({}))
        .await
        .unwrap();
    let mut handles = Vec::new();
    for _ in 0..16 {
        handles.push(
            host.start_request(
                "fixture",
                "media.resolve",
                json!({"track_id":"3","purpose":"import"}),
            )
            .await
            .unwrap(),
        );
    }
    assert_eq!(
        host.request(
            "fixture",
            "auth.cancel",
            json!({"challenge":auth["challenge"]})
        )
        .await
        .unwrap()["state"],
        "cancelled"
    );
    assert_eq!(
        host.request("fixture", "auth.sign_out", json!({}))
            .await
            .unwrap()["state"],
        "signed_out"
    );
    for handle in handles {
        assert_eq!(handle.result().await.unwrap_err().code, "cancelled");
    }
    drop(lease);
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn known_expired_media_is_refused_before_transport_for_bounded_preview_refresh() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let mut config = fixture.config();
    let url = "https://streaming-fixture.openkara.test:9443/tone-1.wav";
    config.add_fixture_resource(
        url.into(),
        vec!["93.184.216.34".parse().unwrap()],
        b"media".to_vec(),
    );
    config.set_fixture_status(url.into(), 410);
    let host = SourceHost::new(config).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    let descriptor = ResourceDescriptor {
        url: url.into(),
        headers: BTreeMap::new(),
        expires_at: Some("2000-01-01T00:00:00Z".into()),
    };
    assert_eq!(
        host.open_media("fixture", &descriptor, None, Cancellation::new())
            .await
            .err()
            .unwrap()
            .code,
        "media_expired"
    );
    assert_eq!(
        host.load_resource("fixture", descriptor, Cancellation::new())
            .await
            .unwrap_err()
            .code,
        "media_expired"
    );
    let descriptor = ResourceDescriptor {
        url: url.into(),
        headers: BTreeMap::new(),
        expires_at: None,
    };
    assert_eq!(
        host.open_media("fixture", &descriptor, None, Cancellation::new())
            .await
            .unwrap()
            .status,
        410
    );
    assert_eq!(
        host.download_media(
            "fixture",
            descriptor,
            fixture.root.path().join("expired.wav"),
            Cancellation::new()
        )
        .await
        .unwrap_err()
        .code,
        "media_expired"
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn requests_at_idle_boundary_are_not_terminated_by_maintenance() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let mut config = fixture.config();
    config.idle_timeout = Duration::ZERO;
    let host = SourceHost::new(config).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    for _ in 0..30 {
        assert_eq!(
            host.request("fixture", "describe", json!({}))
                .await
                .unwrap()["name"],
            "Deterministic Streaming Source"
        );
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn aborted_backpressured_dispatch_stops_partial_frames_and_releases_activation() {
    let mut fixture = Fixture::new().await;
    let worker=include_str!(concat!(env!("CARGO_MANIFEST_DIR"),"/../fixtures/streaming-source/worker.mjs")).replace("if (method === \"describe\")","if (method === \"tracks.get\") { process.stdin.pause(); return new Promise(()=>{}); } if (method === \"describe\")");
    fixture.publish("1.0.0", &worker, vec![], None).await;
    let mut config = fixture.config();
    config.operation_timeout = Duration::from_millis(100);
    let host = SourceHost::new(config).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    let stalled = host
        .start_request("fixture", "tracks.get", json!({"track_ids":["1"]}))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    let copy = host.clone();
    let dispatch = tokio::spawn(async move {
        copy.start_request(
            "fixture",
            "tracks.get",
            json!({"track_ids":["2"],"padding":"x".repeat(900_000)}),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    dispatch.abort();
    let _ = dispatch.await;
    let started = Instant::now();
    stalled.cancel();
    assert!(stalled.result().await.is_err());
    assert!(started.elapsed() < Duration::from_millis(2300));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!host.snapshot("fixture").unwrap().running);
    fixture
        .publish(
            "1.1.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    host.refresh().await.unwrap();
    assert_eq!(
        host.install("fixture", None)
            .await
            .unwrap()
            .installed_version
            .as_deref(),
        Some("1.1.0")
    );
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap()["name"],
        "Deterministic Streaming Source"
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn verified_revocation_applies_even_when_settings_cannot_be_saved() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    let lease = host.acquire_work("fixture", WorkKind::Preview).unwrap();
    let cancel = lease.cancellation();
    host.request("fixture", "describe", json!({}))
        .await
        .unwrap();
    fs::create_dir(fixture.config().data_root.join("state.json.tmp")).unwrap();
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec!["1.0.0".into()],
            None,
        )
        .await;
    assert!(host.refresh().await.is_err());
    assert!(cancel.is_cancelled());
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap_err()
            .code,
        "incompatible_plugin"
    );
    drop(lease);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!host.snapshot("fixture").unwrap().running);
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn startup_mutation_and_damaged_staged_files_preserve_previous_release() {
    let mut fixture = Fixture::new().await;
    let worker = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/streaming-source/worker.mjs"
    ));
    fixture.publish("1.0.0", worker, vec![], None).await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    fixture.publish("1.1.0",&format!("import {{writeFileSync}} from 'node:fs';writeFileSync('startup-marker','changed');\n{worker}"),vec![],None).await;
    host.refresh().await.unwrap();
    assert_eq!(
        host.install("fixture", None).await.unwrap_err().code,
        "invalid_package"
    );
    assert_eq!(
        host.snapshot("fixture")
            .unwrap()
            .installed_version
            .as_deref(),
        Some("1.0.0")
    );
    let lease = host.acquire_work("fixture", WorkKind::Import).unwrap();
    fixture.publish("1.2.0", worker, vec![], None).await;
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    let sha = fixture.index["sources"][0]["releases"][0]["archive"]["sha256"]
        .as_str()
        .unwrap();
    fs::remove_file(
        fixture
            .config()
            .data_root
            .join("packages")
            .join(sha)
            .join("worker.mjs"),
    )
    .unwrap();
    drop(lease);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        host.snapshot("fixture")
            .unwrap()
            .installed_version
            .as_deref(),
        Some("1.0.0")
    );
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap()["name"],
        "Deterministic Streaming Source"
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn retired_cached_root_does_not_block_new_trusted_discovery_or_working_install() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    drop(host);
    let mut config = fixture.config();
    config.trusted_keys.remove("test-current");
    let host = SourceHost::new(config).await.unwrap();
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap()["name"],
        "Deterministic Streaming Source"
    );
    fixture.change_index(true, |_| {}).await;
    host.refresh().await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn explicit_reinstall_repairs_damaged_package_without_overwriting_running_bytes() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    host.request("fixture", "describe", json!({}))
        .await
        .unwrap();
    let lease = host.acquire_work("fixture", WorkKind::Preview).unwrap();
    let sha = fixture.index["sources"][0]["releases"][0]["archive"]["sha256"]
        .as_str()
        .unwrap();
    let original = fixture
        .config()
        .data_root
        .join("packages")
        .join(sha)
        .join("worker.mjs");
    fs::write(&original, b"damaged").unwrap();
    let snapshot = host.install("fixture", None).await.unwrap();
    assert_eq!(snapshot.state, SourceState::WaitingForIdle);
    assert_eq!(fs::read(&original).unwrap(), b"damaged");
    drop(lease);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        host.request("fixture", "describe", json!({}))
            .await
            .unwrap()["name"],
        "Deterministic Streaming Source"
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn revocation_during_download_prevents_candidate_execution() {
    let mut fixture = Fixture::new().await;
    let marker = fixture.root.path().join("candidate-executed");
    let worker = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/streaming-source/worker.mjs"
    ));
    fixture.publish("1.0.0", worker, vec![], None).await;
    let host = SourceHost::new(fixture.config()).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    let startup = format!(
        "import {{writeFileSync}} from 'node:fs';writeFileSync({},'executed');\n{worker}",
        serde_json::to_string(&marker).unwrap()
    );
    fixture.publish("1.1.0", &startup, vec![], None).await;
    host.refresh().await.unwrap();
    let bytes = fixture.assets["/package-1.1.0.tar"].clone();
    fixture.server.reset();
    fixture
        .server
        .mock("GET", "/package-1.1.0.tar")
        .with_chunked_body(move |stream| {
            std::thread::sleep(Duration::from_millis(300));
            stream.write_all(&bytes)
        })
        .create_async()
        .await;
    let installer = host.clone();
    let install = tokio::spawn(async move { installer.install("fixture", None).await });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            Instant::now() < deadline,
            "candidate never reached download: {:?}",
            host.snapshot("fixture").unwrap()
        );
        if host
            .snapshot("fixture")
            .unwrap()
            .progress
            .is_some_and(|progress| progress.phase == "package")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    fixture.index["sources"][0]["revoked_versions"] = json!(["1.1.0"]);
    fixture.generation += 1;
    fixture.index["generation"] = json!(fixture.generation);
    let payload = serde_json::to_vec(&fixture.index).unwrap();
    let envelope = json!({"key_id":"test-current","payload":base64::engine::general_purpose::STANDARD.encode(&payload),"signature":base64::engine::general_purpose::STANDARD.encode(fixture.key.sign(&payload).as_ref())});
    fixture
        .server
        .mock("GET", "/index.json")
        .with_body(serde_json::to_vec(&envelope).unwrap())
        .create_async()
        .await;
    host.refresh().await.unwrap();
    assert!(install.await.unwrap().is_err());
    assert!(!marker.exists());
    assert_eq!(
        host.snapshot("fixture")
            .unwrap()
            .installed_version
            .as_deref(),
        Some("1.0.0")
    );
}

#[tokio::test]
#[ignore = "requires explicitly prepared pinned Node via OPENKARA_SOURCE_TEST_NODE"]
async fn cancelled_artwork_work_refuses_transfer_and_releases_stop_acknowledgment() {
    let mut fixture = Fixture::new().await;
    fixture
        .publish(
            "1.0.0",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/streaming-source/worker.mjs"
            )),
            vec![],
            None,
        )
        .await;
    let mut config = fixture.config();
    let url = "https://streaming-fixture.openkara.test:9443/artwork.png";
    config.add_fixture_resource(
        url.into(),
        vec!["93.184.216.34".parse().unwrap()],
        b"artwork".to_vec(),
    );
    let host = SourceHost::new(config).await.unwrap();
    host.refresh().await.unwrap();
    host.install("fixture", None).await.unwrap();
    host.set_enabled("fixture", true, false).await.unwrap();
    let lease = host.acquire_work("fixture", WorkKind::Import).unwrap();
    let cancellation = lease.cancellation();
    let stop = host.clone();
    let stopping = tokio::spawn(async move { stop.set_enabled("fixture", false, true).await });
    tokio::time::timeout(Duration::from_secs(3), cancellation.cancelled())
        .await
        .unwrap();
    let descriptor = ResourceDescriptor {
        url: url.into(),
        headers: BTreeMap::new(),
        expires_at: None,
    };
    assert_eq!(
        host.load_resource("fixture", descriptor, cancellation)
            .await
            .unwrap_err()
            .code,
        "cancelled"
    );
    drop(lease);
    assert!(!stopping.await.unwrap().unwrap().enabled);
}
