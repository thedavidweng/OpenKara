use super::test_support::fixture_manifest;
use super::*;
use base64::Engine;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

const SOURCE: &str = r#"
export default {
  describe() {
    return {
      protocol: { major: __MAJOR__, minor: 0 },
      source_id: "__SOURCE__",
      package_id: "__PACKAGE__",
      package_version: "__VERSION__",
      name: "Registry Fixture",
      description: "Source Registry test source.",
      capabilities: ["playlists"],
      authentication: { flow: "none" },
      requires_authentication: [],
      session: { state: "signed_out", account: null },
    };
  },
  playlists() {
    return { items: [], next_cursor: null, version: "__VERSION__" };
  },
};
"#;

struct Version {
    version: &'static str,
    registry_majors: Vec<u32>,
    manifest_majors: Vec<u32>,
    describe_major: u32,
}
impl Version {
    fn new(version: &'static str) -> Self {
        Self {
            version,
            registry_majors: vec![2],
            manifest_majors: vec![2],
            describe_major: 2,
        }
    }
}

fn manifest(source_id: &str, package_id: &str, version: &Version) -> SourceManifest {
    let mut manifest = fixture_manifest(source_id, version.version);
    manifest.package_id = package_id.into();
    manifest.runtime = RuntimeRequirement {
        kind: "openkara-js".into(),
        major: 1,
    };
    manifest.protocol = ProtocolDeclaration {
        majors: version.manifest_majors.clone(),
        minor: 0,
    };
    manifest.entry = "source.mjs".into();
    manifest.capabilities = vec!["playlists".into()];
    manifest.authentication = json!({"flow":"none"});
    manifest.requires_authentication = vec![];
    manifest
}

fn tarball(manifest: &SourceManifest, source: &str) -> Vec<u8> {
    let mut archive = tar::Builder::new(Vec::new());
    for (path, bytes) in [
        ("package/package.json", br#"{"name":"fixture"}"#.to_vec()),
        (
            "package/manifest.json",
            serde_json::to_vec(manifest).unwrap(),
        ),
        ("package/source.mjs", source.as_bytes().to_vec()),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, path, bytes.as_slice())
            .unwrap();
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&archive.into_inner().unwrap()).unwrap();
    encoder.finish().unwrap()
}

fn sri(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha512};
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
    )
}

/// A mock Source Registry that lists one package and serves its tarballs.
struct MockRegistry {
    server: mockito::ServerGuard,
    id: &'static str,
    source_id: &'static str,
    tarballs: BTreeMap<String, Vec<u8>>,
}
impl MockRegistry {
    async fn new(id: &'static str, source_id: &'static str) -> Self {
        Self {
            server: mockito::Server::new_async().await,
            id,
            source_id,
            tarballs: BTreeMap::new(),
        }
    }
    fn url(&self) -> String {
        format!("{}/registry.json", self.server.url())
    }
    fn package_id(&self) -> String {
        format!("{}.{}", self.id, self.source_id)
    }
    fn tarball_url(&self, version: &str) -> String {
        format!("{}/{}-{version}.tgz", self.server.url(), self.source_id)
    }
    async fn publish(&mut self, versions: &[Version]) {
        self.publish_with(versions, |_| {}).await;
    }
    async fn publish_with(&mut self, versions: &[Version], edit: impl FnOnce(&mut Value)) {
        self.server.reset();
        self.tarballs.clear();
        let package_id = self.package_id();
        let mut entries = Vec::new();
        for version in versions {
            let manifest = manifest(self.source_id, &package_id, version);
            let source = SOURCE
                .replace("__MAJOR__", &version.describe_major.to_string())
                .replace("__SOURCE__", self.source_id)
                .replace("__PACKAGE__", &package_id)
                .replace("__VERSION__", version.version);
            let bytes = tarball(&manifest, &source);
            entries.push(json!({
                "version": version.version,
                "protocol_majors": version.registry_majors,
                "minimum_host_version": manifest.minimum_host_version,
                "size": bytes.len(),
                "integrity": sri(&bytes),
                "urls": [self.tarball_url(version.version)],
                "resource_origins": manifest.resource_origins,
                "media_origins": manifest.media_origins,
                "capabilities": manifest.capabilities,
            }));
            self.tarballs.insert(
                format!("/{}-{}.tgz", self.source_id, version.version),
                bytes,
            );
        }
        let mut document = json!({
            "format_version": 1,
            "registry_id": self.id,
            "display_name": "Fixture Registry",
            "homepage": "https://registry.example/",
            "packages": [{
                "package_id": package_id,
                "source_id": self.source_id,
                "name": "Registry Fixture",
                "description": "Source Registry test source.",
                "license": "AGPL-3.0-only",
                "versions": entries,
            }],
        });
        edit(&mut document);
        for (path, bytes) in &self.tarballs {
            self.server
                .mock("GET", path.as_str())
                .with_body(bytes.clone())
                .create_async()
                .await;
        }
        self.server
            .mock("GET", "/registry.json")
            .with_body(serde_json::to_vec(&document).unwrap())
            .create_async()
            .await;
    }
}

fn config(root: &Path) -> HostConfig {
    let mut config = HostConfig::fixture(
        root.to_path_buf(),
        "0.17.0".into(),
        String::new(),
        BTreeMap::new(),
        RuntimePolicy::Downloaded,
    );
    config.describe_timeout = Duration::from_millis(1500);
    config.operation_timeout = Duration::from_millis(1500);
    config
}

struct Fixture {
    root: tempfile::TempDir,
    registry: MockRegistry,
    host: SourceHost,
}
impl Fixture {
    async fn new(versions: &[Version]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut registry = MockRegistry::new("org.example.registry", "fixture").await;
        registry.publish(versions).await;
        let host = SourceHost::new(config(root.path())).await.unwrap();
        host.add_registry(&registry.url()).await.unwrap();
        Self {
            root,
            registry,
            host,
        }
    }
    async fn install(&self, version: Option<&str>) -> HostResult<SourceSnapshot> {
        self.host
            .install_package(self.registry.id, &self.registry.package_id(), version)
            .await
    }
    async fn served_version(&self) -> HostResult<Value> {
        Ok(self
            .host
            .request("fixture", "playlists", json!({"cursor":null}))
            .await?["version"]
            .clone())
    }
    fn package_directories(&self) -> usize {
        std::fs::read_dir(self.root.path().join("streaming-sources/packages"))
            .map(|entries| entries.count())
            .unwrap_or(0)
    }
}

async fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn registry_package_installs_verified_bytes_and_runs_without_node() {
    let fixture = Fixture::new(&[Version::new("1.0.0")]).await;
    let host = &fixture.host;
    let registries = host.registries();
    assert_eq!(registries.len(), 1);
    assert_eq!(registries[0].registry_id, "org.example.registry");
    assert_eq!(registries[0].display_name, "Fixture Registry");
    assert_eq!(registries[0].homepage, "https://registry.example/");
    assert_eq!(registries[0].url, fixture.registry.url());
    assert!(registries[0].refreshed_at.is_some());
    assert_eq!(registries[0].last_error, None);
    assert!(
        host.snapshots().is_empty(),
        "adding a registry installs nothing"
    );

    let installed = fixture.install(None).await.unwrap();
    assert_eq!(installed.source_id, "fixture");
    assert_eq!(installed.installed_version.as_deref(), Some("1.0.0"));
    assert_eq!(installed.available_version.as_deref(), Some("1.0.0"));
    assert_eq!(installed.name, "Deterministic Streaming Source");
    assert_eq!(
        installed.owner_binding,
        "registry:org.example.registry/org.example.registry.fixture"
    );
    assert!(!installed.enabled);
    host.set_enabled("fixture", true, false).await.unwrap();
    assert_eq!(fixture.served_version().await.unwrap(), "1.0.0");

    drop(fixture.host);
    let host = SourceHost::new(config(fixture.root.path())).await.unwrap();
    assert_eq!(host.registries().len(), 1, "added registries persist");
    let snapshot = host.snapshot("fixture").unwrap();
    assert_eq!(snapshot.installed_version.as_deref(), Some("1.0.0"));
    assert_eq!(snapshot.state, SourceState::Ready);
}

#[tokio::test]
async fn registries_are_https_only_and_invalid_documents_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let host = SourceHost::new(config(root.path())).await.unwrap();
    assert_eq!(
        host.add_registry("http://registry.example/registry.json")
            .await
            .unwrap_err()
            .code,
        "invalid_resource"
    );
    assert_eq!(
        host.add_registry("ftp://registry.example/registry.json")
            .await
            .unwrap_err()
            .code,
        "invalid_resource"
    );

    let mut registry = MockRegistry::new("org.example.registry", "fixture").await;
    let edits: Vec<(&str, Box<dyn FnOnce(&mut Value)>)> = vec![
        ("format", Box::new(|doc| doc["format_version"] = json!(2))),
        (
            "registry id",
            Box::new(|doc| doc["registry_id"] = json!("Bad/Id")),
        ),
        (
            "homepage",
            Box::new(|doc| doc["homepage"] = json!("http://registry.example/")),
        ),
        (
            "source id",
            Box::new(|doc| doc["packages"][0]["source_id"] = json!("Fixture")),
        ),
        (
            "license",
            Box::new(|doc| doc["packages"][0]["license"] = json!("")),
        ),
        (
            "duplicate package",
            Box::new(|doc| {
                let package = doc["packages"][0].clone();
                doc["packages"].as_array_mut().unwrap().push(package);
            }),
        ),
        (
            "version",
            Box::new(|doc| doc["packages"][0]["versions"][0]["version"] = json!("one")),
        ),
        (
            "protocol majors",
            Box::new(|doc| doc["packages"][0]["versions"][0]["protocol_majors"] = json!([])),
        ),
        (
            "integrity algorithm",
            Box::new(|doc| {
                doc["packages"][0]["versions"][0]["integrity"] =
                    json!("sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=")
            }),
        ),
        (
            "plain http download",
            Box::new(|doc| {
                doc["packages"][0]["versions"][0]["urls"] =
                    json!(["http://registry.example/fixture.tgz"])
            }),
        ),
        (
            "no download urls",
            Box::new(|doc| doc["packages"][0]["versions"][0]["urls"] = json!([])),
        ),
        (
            "oversized package",
            Box::new(|doc| doc["packages"][0]["versions"][0]["size"] = json!(64u64 * 1024 * 1024)),
        ),
        (
            "origin with a path",
            Box::new(|doc| {
                doc["packages"][0]["versions"][0]["resource_origins"] =
                    json!(["https://streaming-fixture.openkara.test/api"])
            }),
        ),
    ];
    for (case, edit) in edits {
        registry.publish_with(&[Version::new("1.0.0")], edit).await;
        assert_eq!(
            host.add_registry(&registry.url()).await.unwrap_err().code,
            "invalid_registry",
            "{case}"
        );
    }
    assert!(host.registries().is_empty());
}

#[tokio::test]
async fn unreachable_registry_keeps_the_last_good_listing_and_installed_source() {
    let mut fixture = Fixture::new(&[Version::new("1.0.0")]).await;
    fixture.install(None).await.unwrap();
    fixture
        .host
        .set_enabled("fixture", true, false)
        .await
        .unwrap();
    fixture.registry.server.reset();

    fixture.host.refresh().await.unwrap();
    let registry = &fixture.host.registries()[0];
    assert_eq!(registry.display_name, "Fixture Registry");
    assert_eq!(
        registry
            .last_error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("network")
    );
    let snapshot = fixture.host.snapshot("fixture").unwrap();
    assert_eq!(snapshot.available_version.as_deref(), Some("1.0.0"));
    assert_eq!(fixture.served_version().await.unwrap(), "1.0.0");

    drop(fixture.host);
    fixture.host = SourceHost::new(config(fixture.root.path())).await.unwrap();
    let registry = &fixture.host.registries()[0];
    assert_eq!(registry.display_name, "Fixture Registry");
    assert_eq!(
        fixture
            .host
            .snapshot("fixture")
            .unwrap()
            .available_version
            .as_deref(),
        Some("1.0.0"),
        "the cached listing survives a restart"
    );
    assert_eq!(fixture.served_version().await.unwrap(), "1.0.0");

    fixture
        .registry
        .publish(&[Version::new("1.0.0"), Version::new("1.1.0")])
        .await;
    fixture.host.refresh().await.unwrap();
    assert_eq!(fixture.host.registries()[0].last_error, None);
    assert_eq!(
        fixture
            .host
            .snapshot("fixture")
            .unwrap()
            .available_version
            .as_deref(),
        Some("1.1.0")
    );
}

#[tokio::test]
async fn registries_refresh_periodically() {
    let root = tempfile::tempdir().unwrap();
    let mut registry = MockRegistry::new("org.example.registry", "fixture").await;
    registry.publish(&[Version::new("1.0.0")]).await;
    let mut config = config(root.path());
    config.registry_refresh_interval = Duration::from_millis(100);
    let host = SourceHost::new(config).await.unwrap();
    host.add_registry(&registry.url()).await.unwrap();
    host.install_package(registry.id, &registry.package_id(), None)
        .await
        .unwrap();

    registry
        .publish(&[Version::new("1.0.0"), Version::new("2.0.0")])
        .await;
    eventually("the periodic refresh", || {
        host.snapshot("fixture")
            .unwrap()
            .available_version
            .as_deref()
            == Some("2.0.0")
    })
    .await;
}

#[tokio::test]
async fn downloads_try_urls_in_order_and_a_failed_install_names_every_url() {
    let mut fixture = Fixture::new(&[Version::new("1.0.0")]).await;
    let good = fixture.registry.tarball_url("1.0.0");
    let declared = fixture.registry.tarballs.values().next().unwrap().len();
    let base = fixture.registry.server.url();
    let missing = format!("{base}/missing.tgz");
    let corrupt = format!("{base}/corrupt.tgz");
    let oversized = format!("{base}/oversized.tgz");
    let unused = format!("{base}/unused.tgz");
    let bad = [missing.clone(), corrupt.clone(), oversized.clone()];

    let publish = |urls: Vec<String>| {
        move |doc: &mut Value| doc["packages"][0]["versions"][0]["urls"] = json!(urls)
    };
    fixture
        .registry
        .publish_with(&[Version::new("1.0.0")], publish(bad.to_vec()))
        .await;
    let server = &mut fixture.registry.server;
    let corrupt_mock = server
        .mock("GET", "/corrupt.tgz")
        .with_body(vec![7u8; declared])
        .expect(1)
        .create_async()
        .await;
    let oversized_mock = server
        .mock("GET", "/oversized.tgz")
        .with_body(vec![7u8; declared + 64 * 1024])
        .expect(1)
        .create_async()
        .await;
    fixture.host.refresh().await.unwrap();
    let error = fixture.install(None).await.unwrap_err();
    assert_eq!(error.code, "invalid_package");
    let positions: Vec<_> = bad
        .iter()
        .map(|url| {
            error
                .message
                .find(url.as_str())
                .unwrap_or_else(|| panic!("{url} missing from {}", error.message))
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{error}"
    );
    assert!(error.message.contains("HTTP 501"), "{error}");
    assert!(error.message.contains("integrity"), "{error}");
    assert!(
        error.message.contains("larger than the declared size"),
        "{error}"
    );
    corrupt_mock.assert_async().await;
    oversized_mock.assert_async().await;
    let snapshot = fixture.host.snapshot("fixture").unwrap();
    assert_eq!(snapshot.installed_version, None);
    assert_eq!(snapshot.last_error, Some(error));
    assert_eq!(
        fixture.package_directories(),
        0,
        "unverified bytes never load"
    );

    let mut urls = bad.to_vec();
    urls.extend([good.clone(), unused]);
    fixture
        .registry
        .publish_with(&[Version::new("1.0.0")], publish(urls))
        .await;
    let server = &mut fixture.registry.server;
    let missing_mock = server
        .mock("GET", "/missing.tgz")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let unused_mock = server
        .mock("GET", "/unused.tgz")
        .expect(0)
        .create_async()
        .await;
    fixture.host.refresh().await.unwrap();
    let installed = fixture.install(None).await.unwrap();
    assert_eq!(installed.installed_version.as_deref(), Some("1.0.0"));
    missing_mock.assert_async().await;
    unused_mock.assert_async().await;

    fixture
        .registry
        .publish_with(&[Version::new("1.0.0")], |doc| {
            doc["packages"][0]["versions"][0]["urls"] = json!([format!("{base}/missing.tgz")])
        })
        .await;
    fixture.host.refresh().await.unwrap();
    fixture.host.uninstall("fixture", false).await.unwrap();
    let error = fixture.install(None).await.unwrap_err();
    assert_eq!(error.code, "network", "{error}");
    assert!(error.message.contains(&missing), "{error}");
}

#[tokio::test]
async fn only_one_installed_package_can_hold_a_source_id() {
    let fixture = Fixture::new(&[Version::new("1.0.0")]).await;
    let mut other = MockRegistry::new("org.example.other", "fixture").await;
    other.publish(&[Version::new("5.0.0")]).await;
    fixture.host.add_registry(&other.url()).await.unwrap();
    assert_eq!(fixture.host.registries().len(), 2);

    fixture.install(None).await.unwrap();
    let error = fixture
        .host
        .install_package(other.id, &other.package_id(), None)
        .await
        .unwrap_err();
    assert_eq!(error.code, "source_id_taken");
    let snapshot = fixture.host.snapshot("fixture").unwrap();
    assert_eq!(snapshot.installed_version.as_deref(), Some("1.0.0"));
    assert_eq!(
        snapshot.owner_binding,
        "registry:org.example.registry/org.example.registry.fixture"
    );

    let mut same_id = MockRegistry::new("org.example.registry", "other").await;
    same_id.publish(&[Version::new("1.0.0")]).await;
    assert_eq!(
        fixture
            .host
            .add_registry(&same_id.url())
            .await
            .unwrap_err()
            .code,
        "invalid_registry",
        "a second URL cannot claim an added registry id"
    );

    fixture.host.uninstall("fixture", false).await.unwrap();
    let replaced = fixture
        .host
        .install_package(other.id, &other.package_id(), None)
        .await
        .unwrap();
    assert_eq!(replaced.installed_version.as_deref(), Some("5.0.0"));
    assert_eq!(
        replaced.owner_binding,
        "registry:org.example.other/org.example.other.fixture"
    );
    assert_eq!(
        fixture.install(None).await.unwrap_err().code,
        "source_id_taken"
    );
}

#[tokio::test]
async fn an_update_must_pass_describe_before_it_replaces_the_active_version() {
    let mut fixture = Fixture::new(&[Version::new("1.0.0")]).await;
    fixture.install(None).await.unwrap();
    fixture
        .host
        .set_enabled("fixture", true, false)
        .await
        .unwrap();
    assert_eq!(fixture.served_version().await.unwrap(), "1.0.0");

    fixture
        .registry
        .publish(&[
            Version::new("1.0.0"),
            Version {
                describe_major: 3,
                ..Version::new("2.0.0")
            },
        ])
        .await;
    fixture.host.refresh().await.unwrap();
    let error = fixture.host.install("fixture", None).await.unwrap_err();
    assert_eq!(error.code, "incompatible_plugin");
    let snapshot = fixture.host.snapshot("fixture").unwrap();
    assert_eq!(snapshot.installed_version.as_deref(), Some("1.0.0"));
    assert_eq!(snapshot.staged_version, None);
    assert_eq!(fixture.served_version().await.unwrap(), "1.0.0");

    fixture
        .registry
        .publish(&[Version::new("1.0.0"), Version::new("3.0.0")])
        .await;
    fixture.host.refresh().await.unwrap();
    let lease = fixture
        .host
        .acquire_work("fixture", WorkKind::Import)
        .unwrap();
    let staged = fixture.host.install("fixture", None).await.unwrap();
    assert_eq!(staged.state, SourceState::WaitingForIdle);
    assert_eq!(staged.installed_version.as_deref(), Some("1.0.0"));
    assert_eq!(staged.staged_version.as_deref(), Some("3.0.0"));
    assert_eq!(fixture.served_version().await.unwrap(), "1.0.0");
    assert!(
        fixture.package_directories() >= 2,
        "the previous files stay"
    );

    drop(lease);
    eventually("activation at idle", || {
        fixture
            .host
            .snapshot("fixture")
            .unwrap()
            .installed_version
            .as_deref()
            == Some("3.0.0")
    })
    .await;
    assert_eq!(fixture.served_version().await.unwrap(), "3.0.0");
}

#[tokio::test]
async fn protocol_major_mismatches_are_incompatible_and_registry_disagreement_is_refused() {
    let fixture = Fixture::new(&[Version {
        registry_majors: vec![3],
        manifest_majors: vec![3],
        ..Version::new("1.0.0")
    }])
    .await;
    assert_eq!(
        fixture.install(None).await.unwrap_err().code,
        "incompatible_plugin"
    );
    assert_eq!(fixture.package_directories(), 0, "nothing was downloaded");

    let mut fixture = fixture;
    fixture
        .registry
        .publish(&[Version {
            manifest_majors: vec![3],
            ..Version::new("1.0.0")
        }])
        .await;
    fixture.host.refresh().await.unwrap();
    assert_eq!(
        fixture.install(None).await.unwrap_err().code,
        "invalid_package"
    );

    fixture
        .registry
        .publish_with(&[Version::new("1.0.0")], |doc| {
            doc["packages"][0]["versions"][0]["capabilities"] = json!(["playlists", "search"])
        })
        .await;
    fixture.host.refresh().await.unwrap();
    assert_eq!(
        fixture.install(None).await.unwrap_err().code,
        "invalid_package",
        "the package must declare what the registry lists"
    );
    assert_eq!(
        fixture.host.snapshot("fixture").unwrap().installed_version,
        None
    );
    assert_eq!(fixture.package_directories(), 0);
}

#[tokio::test]
async fn unknown_registry_packages_are_not_found() {
    let fixture = Fixture::new(&[Version::new("1.0.0")]).await;
    assert_eq!(
        fixture
            .host
            .install_package("org.example.registry", "missing", None)
            .await
            .unwrap_err()
            .code,
        "not_found"
    );
    assert_eq!(
        fixture
            .host
            .install_package("org.example.missing", "org.example.registry.fixture", None)
            .await
            .unwrap_err()
            .code,
        "not_found"
    );
}
