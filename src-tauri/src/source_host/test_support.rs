use super::{digest, types::*};
use base64::Engine;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) const CURRENT_KEY: &str = "fixture-current";
pub(crate) const NEXT_KEY: &str = "fixture-next";

pub(crate) struct FixturePackage {
    pub manifest: SourceManifest,
    pub worker: String,
    pub revoked_versions: Vec<String>,
}
/// Mock HTTP server that serves a signed source index and its package archives.
pub(crate) struct FixtureDistribution {
    pub(crate) server: mockito::ServerGuard,
    key: Ed25519KeyPair,
    next_key: Ed25519KeyPair,
    generation: u64,
    pub(crate) index: Value,
    assets: BTreeMap<String, Vec<u8>>,
}
pub(crate) fn fixture_manifest(source_id: &str, version: &str) -> SourceManifest {
    let mut manifest: SourceManifest = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/streaming-source/manifest.json"
    )))
    .unwrap();
    manifest.source_id = source_id.into();
    manifest.package_id = format!("org.openkara.{source_id}");
    manifest.version = version.into();
    manifest
}
pub(crate) fn test_node() -> PathBuf {
    PathBuf::from(
        std::env::var_os("OPENKARA_SOURCE_TEST_NODE")
            .expect("prepare pinned Node and set OPENKARA_SOURCE_TEST_NODE"),
    )
}
pub(crate) fn archive_path(source_id: &str, version: &str) -> String {
    format!("/{source_id}-{version}.tar")
}
fn target() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}
fn archive(package: &FixturePackage) -> Vec<u8> {
    let mut archive = tar::Builder::new(Vec::new());
    for (path, bytes) in [
        (
            "manifest.json",
            serde_json::to_vec(&package.manifest).unwrap(),
        ),
        (
            package.manifest.entry.as_str(),
            package.worker.as_bytes().to_vec(),
        ),
        ("package-lock.json", b"{\"lockfileVersion\":3}".to_vec()),
        ("LICENSE", b"AGPL-3.0-only".to_vec()),
        ("NOTICE", b"Deterministic source test fixture".to_vec()),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, path, bytes.as_slice())
            .unwrap();
    }
    archive.into_inner().unwrap()
}
impl FixtureDistribution {
    pub(crate) async fn new() -> Self {
        let random = ring::rand::SystemRandom::new();
        let key = Ed25519KeyPair::generate_pkcs8(&random).unwrap();
        let next_key = Ed25519KeyPair::generate_pkcs8(&random).unwrap();
        Self {
            server: mockito::Server::new_async().await,
            key: Ed25519KeyPair::from_pkcs8(key.as_ref()).unwrap(),
            next_key: Ed25519KeyPair::from_pkcs8(next_key.as_ref()).unwrap(),
            generation: 0,
            index: Value::Null,
            assets: BTreeMap::new(),
        }
    }
    pub(crate) fn config(&self, data_root: PathBuf) -> HostConfig {
        HostConfig::fixture(
            data_root,
            "0.17.0".into(),
            format!("{}/index.json", self.server.url()),
            BTreeMap::from([
                (
                    CURRENT_KEY.into(),
                    self.key.public_key().as_ref().try_into().unwrap(),
                ),
                (
                    NEXT_KEY.into(),
                    self.next_key.public_key().as_ref().try_into().unwrap(),
                ),
            ]),
            RuntimePolicy::ChannelBundled {
                executable: test_node(),
                version: "24.22.1".into(),
                target: target(),
            },
        )
    }
    /// Configuration for sources that run in the embedded engine; it needs no Node.
    pub(crate) fn engine_config(&self, data_root: PathBuf) -> HostConfig {
        HostConfig::fixture(
            data_root,
            "0.17.0".into(),
            format!("{}/index.json", self.server.url()),
            BTreeMap::from([(
                CURRENT_KEY.into(),
                self.key.public_key().as_ref().try_into().unwrap(),
            )]),
            RuntimePolicy::Downloaded,
        )
    }
    pub(crate) fn asset(&self, path: &str) -> Vec<u8> {
        self.assets[path].clone()
    }
    pub(crate) async fn publish(&mut self, packages: Vec<FixturePackage>) {
        self.publish_signed(packages, None).await;
    }
    /// Publishes under the current key id; `signer` replaces the current key to forge a signature.
    pub(crate) async fn publish_signed(
        &mut self,
        packages: Vec<FixturePackage>,
        signer: Option<&Ed25519KeyPair>,
    ) {
        self.server.reset();
        self.generation += 1;
        self.assets.clear();
        let mut sources = Vec::new();
        for package in packages {
            let bytes = archive(&package);
            let path = archive_path(&package.manifest.source_id, &package.manifest.version);
            let artifact = json!({"url":format!("{}{path}",self.server.url()),"size":bytes.len(),"sha256":digest(&bytes),"format":"tar"});
            let mut release = json!({"manifest":package.manifest,"archive":artifact});
            if package.manifest.runtime.kind == "node" {
                release["runtime"] = json!({"version":"24.22.1","targets":{(target()):{"url":"https://nodejs.org/fixture-never-download.tar.gz","size":1,"sha256":"00".repeat(32),"format":"tar.gz","executable":"node/bin/node"}}});
            }
            sources.push(json!({"source_id":package.manifest.source_id,"publisher":package.manifest.publisher,"package_id":package.manifest.package_id,"releases":[release],"revoked_versions":package.revoked_versions}));
            self.assets.insert(path, bytes);
        }
        self.serve_assets().await;
        self.index = json!({"schema_version":1,"generation":self.generation,"sources":sources});
        let payload = serde_json::to_vec(&self.index).unwrap();
        let signature = signer.unwrap_or(&self.key).sign(&payload);
        self.serve_index(CURRENT_KEY, &payload, signature.as_ref())
            .await;
    }
    pub(crate) async fn change_index(&mut self, next: bool, change: impl FnOnce(&mut Value)) {
        self.server.reset();
        self.serve_assets().await;
        self.reissue_index(next, change).await;
    }
    /// Serves a newer signed index while every mock already on the server keeps answering.
    pub(crate) async fn reissue_index(&mut self, next: bool, change: impl FnOnce(&mut Value)) {
        self.generation += 1;
        self.index["generation"] = json!(self.generation);
        change(&mut self.index);
        let payload = serde_json::to_vec(&self.index).unwrap();
        let (key_id, key) = if next {
            (NEXT_KEY, &self.next_key)
        } else {
            (CURRENT_KEY, &self.key)
        };
        let signature = key.sign(&payload);
        self.serve_index(key_id, &payload, signature.as_ref()).await;
    }
    /// Rebuilds the first published archive with extra entries and re-signs its size and digest.
    pub(crate) async fn append_package(&mut self, extra: impl FnOnce(&mut tar::Builder<Vec<u8>>)) {
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
    async fn serve_assets(&mut self) {
        for (path, bytes) in &self.assets {
            self.server
                .mock("GET", path.as_str())
                .with_body(bytes.clone())
                .create_async()
                .await;
        }
    }
    async fn serve_index(&mut self, key_id: &str, payload: &[u8], signature: &[u8]) {
        let envelope = json!({"key_id":key_id,"payload":base64::engine::general_purpose::STANDARD.encode(payload),"signature":base64::engine::general_purpose::STANDARD.encode(signature)});
        self.server
            .mock("GET", "/index.json")
            .with_body(serde_json::to_vec(&envelope).unwrap())
            .create_async()
            .await;
    }
}

/// The repo's single-file engine fixture (protocol 2, openkara-js). Its
/// placeholder line is replaced with the package manifest when a registry
/// tarball is built.
pub(crate) const ENGINE_FIXTURE_SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../fixtures/streaming-source/source.mjs"
));

/// A manifest for the single-file engine fixture; callers adjust name,
/// authentication, capabilities and origins per package.
pub(crate) fn engine_fixture_manifest(source_id: &str, version: &str) -> SourceManifest {
    let mut manifest = fixture_manifest(source_id, version);
    manifest.runtime = RuntimeRequirement {
        kind: super::engine::RUNTIME_KIND.into(),
        major: super::engine::RUNTIME_MAJOR,
    };
    manifest.protocol = ProtocolDeclaration {
        majors: vec![super::engine::PROTOCOL_MAJOR],
        minor: 0,
    };
    manifest.entry = "source.mjs".into();
    manifest
}

fn registry_tarball(manifest: &SourceManifest) -> Vec<u8> {
    let manifest_json = serde_json::to_vec(manifest).unwrap();
    let source =
        ENGINE_FIXTURE_SOURCE.replace("__MANIFEST__", std::str::from_utf8(&manifest_json).unwrap());
    assert!(
        !source.contains("__MANIFEST__"),
        "the engine fixture carries exactly one manifest placeholder"
    );
    let mut archive = tar::Builder::new(Vec::new());
    for (path, bytes) in [
        ("package/manifest.json".to_owned(), manifest_json),
        (format!("package/{}", manifest.entry), source.into_bytes()),
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
    std::io::Write::write_all(&mut encoder, &archive.into_inner().unwrap()).unwrap();
    encoder.finish().unwrap()
}

fn sha512_sri(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha512};
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
    )
}

/// A mock Source Registry that serves the repo's single-file engine fixture
/// as npm-style tarballs, so tests install it through the real registry path.
pub(crate) struct RegistryFixture {
    pub(crate) server: mockito::ServerGuard,
    registry_id: String,
}
impl RegistryFixture {
    pub(crate) async fn new(registry_id: &str) -> Self {
        Self {
            server: mockito::Server::new_async().await,
            registry_id: registry_id.into(),
        }
    }
    pub(crate) fn registry_id(&self) -> &str {
        &self.registry_id
    }
    pub(crate) fn package_id(&self, source_id: &str) -> String {
        format!("{}.{source_id}", self.registry_id)
    }
    pub(crate) fn url(&self) -> String {
        format!("{}/registry.json", self.server.url())
    }
    /// Publishes each manifest's single version; every package's entry module
    /// is the repo fixture with that manifest baked in.
    pub(crate) async fn publish(&mut self, manifests: &[SourceManifest]) {
        let mut packages = Vec::new();
        for manifest in manifests {
            let manifest = manifest.clone();
            let bytes = registry_tarball(&manifest);
            let path = format!("/{}-{}.tgz", manifest.source_id, manifest.version);
            let url = format!("{}{path}", self.server.url());
            packages.push(json!({
                "package_id": manifest.package_id,
                "source_id": manifest.source_id,
                "name": manifest.name,
                "description": manifest.description,
                "license": manifest.license,
                "versions": [{
                    "version": manifest.version,
                    "protocol_majors": manifest.protocol.majors,
                    "minimum_host_version": manifest.minimum_host_version,
                    "size": bytes.len(),
                    "integrity": sha512_sri(&bytes),
                    "urls": [url],
                    "resource_origins": manifest.resource_origins,
                    "media_origins": manifest.media_origins,
                    "capabilities": manifest.capabilities,
                }],
            }));
            self.server
                .mock("GET", path.as_str())
                .with_body(bytes)
                .create_async()
                .await;
        }
        let document = json!({
            "format_version": 1,
            "registry_id": self.registry_id,
            "display_name": "Fixture Source Registry",
            "homepage": "https://registry.openkara.test/",
            "packages": packages,
        });
        self.server
            .mock("GET", "/registry.json")
            .with_body(serde_json::to_vec(&document).unwrap())
            .create_async()
            .await;
    }
}
