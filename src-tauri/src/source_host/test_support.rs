use super::types::*;
use base64::Engine;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The manifest of the repo's single-file engine fixture (protocol 2,
/// `openkara-js`); callers adjust name, authentication, capabilities and
/// origins per package.
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

/// The repo's single-file engine fixture. Its placeholder line is replaced
/// with the package manifest when a registry tarball is built.
pub(crate) const ENGINE_FIXTURE_SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../fixtures/streaming-source/source.mjs"
));

fn registry_tarball(manifest: &SourceManifest, source: &str) -> Vec<u8> {
    let mut archive = tar::Builder::new(Vec::new());
    for (path, bytes) in [
        (
            "package/manifest.json".to_owned(),
            serde_json::to_vec(manifest).unwrap(),
        ),
        (
            format!("package/{}", manifest.entry),
            source.as_bytes().to_vec(),
        ),
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

/// The owner binding of the Node-era install that `seed_node_install` leaves.
pub(crate) const NODE_ERA_OWNER: &str = "org.openkara/org.openkara.fixture";

/// The files that a seeded Node-era install leaves on disk.
pub(crate) struct NodeEraInstall {
    pub(crate) packages: Vec<PathBuf>,
    pub(crate) runtimes: PathBuf,
    pub(crate) signed_index: PathBuf,
}

/// A manifest as Node-era hosts installed it from the signed index.
fn node_manifest(source_id: &str, version: &str) -> SourceManifest {
    let mut manifest = fixture_manifest(source_id, version);
    manifest.protocol = ProtocolDeclaration {
        majors: vec![1],
        minor: 0,
    };
    manifest.runtime = RuntimeRequirement {
        kind: "node".into(),
        major: 24,
    };
    manifest.entry = "worker.mjs".into();
    manifest
}

/// A release record in the shape that Node-era hosts stored.
fn node_release(version: &str, package_sha256: &str, runtime_sha256: &str) -> Value {
    json!({
        "manifest": node_manifest("fixture", version),
        "archive": {"url": "https://packages.openkara.test/fixture.tar", "size": 1, "sha256": package_sha256, "format": "tar"},
        "runtime": {"version": "24.22.1", "targets": {"linux-x86_64": {
            "url": "https://nodejs.org/fixture.tar.gz", "size": 1, "sha256": runtime_sha256,
            "format": "tar.gz", "executable": "node/bin/node"
        }}}
    })
}

/// Writes a package tree that holds only the manifest, which is all the host
/// reads to tell a Node package from an embedded-engine package.
pub(crate) fn write_package_tree(root: &Path, manifest: &SourceManifest) {
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(manifest).unwrap(),
    )
    .unwrap();
}

/// Seeds what a Node-era host leaves in `data_root`: an enabled, pinned
/// signed-index install of `fixture` with a staged update, both package trees,
/// the managed Node runtime, and the cached signed index.
pub(crate) fn seed_node_install(data_root: &Path) -> NodeEraInstall {
    let (active, staged, runtime) = ("a".repeat(64), "b".repeat(64), "c".repeat(64));
    std::fs::create_dir_all(data_root).unwrap();
    let state = json!({
        "generation": 3,
        "sources": {"fixture": {
            "owner_binding": NODE_ERA_OWNER,
            "enabled": true,
            "pinned_version": "1.0.0",
            "active": node_release("1.0.0", &active, &runtime),
            "staged": node_release("1.1.0", &staged, &runtime),
            "revoked": []
        }}
    });
    std::fs::write(
        data_root.join("state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let mut packages = Vec::new();
    for (sha256, version) in [(active, "1.0.0"), (staged, "1.1.0")] {
        let tree = data_root.join("packages").join(sha256);
        write_package_tree(&tree, &node_manifest("fixture", version));
        packages.push(tree);
    }
    let runtimes = data_root.join("runtimes");
    let node = runtimes.join(runtime).join("node/bin");
    std::fs::create_dir_all(&node).unwrap();
    std::fs::write(node.join("node"), b"node").unwrap();
    let signed_index = data_root.join("verified-index.json");
    std::fs::write(
        &signed_index,
        br#"{"key_id":"retired","payload":"","signature":""}"#,
    )
    .unwrap();
    NodeEraInstall {
        packages,
        runtimes,
        signed_index,
    }
}
fn sha512_sri(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha512};
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
    )
}

/// A mock Source Registry that serves engine packages as npm-style tarballs,
/// so tests install them through the real registry path.
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
        let packages: Vec<_> = manifests
            .iter()
            .map(|manifest| {
                let manifest_json = serde_json::to_string(manifest).unwrap();
                let source = ENGINE_FIXTURE_SOURCE.replace("__MANIFEST__", &manifest_json);
                assert!(
                    !source.contains("__MANIFEST__"),
                    "the engine fixture carries exactly one manifest placeholder"
                );
                (manifest.clone(), source)
            })
            .collect();
        self.publish_sources(&packages).await;
    }
    /// Publishes each manifest's single version with its own entry module.
    pub(crate) async fn publish_sources(&mut self, packages: &[(SourceManifest, String)]) {
        let mut listed = Vec::new();
        for (manifest, source) in packages {
            let bytes = registry_tarball(manifest, source);
            let path = format!("/{}-{}.tgz", manifest.source_id, manifest.version);
            let url = format!("{}{path}", self.server.url());
            listed.push(json!({
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
            "packages": listed,
        });
        self.server
            .mock("GET", "/registry.json")
            .with_body(serde_json::to_vec(&document).unwrap())
            .create_async()
            .await;
    }
}
