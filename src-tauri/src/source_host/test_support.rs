use super::*;
use base64::Engine;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::json;

pub(crate) struct FixturePackage {
    pub manifest: SourceManifest,
    pub worker: String,
}
pub(crate) struct FixtureDistribution {
    server: mockito::ServerGuard,
    key: Ed25519KeyPair,
    generation: u64,
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
impl FixtureDistribution {
    pub(crate) async fn new() -> Self {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
        Self {
            server: mockito::Server::new_async().await,
            key: Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap(),
            generation: 0,
        }
    }
    pub(crate) fn config(&self, data_root: PathBuf) -> HostConfig {
        let runtime = PathBuf::from(
            std::env::var_os("OPENKARA_SOURCE_TEST_NODE")
                .expect("prepare pinned Node and set OPENKARA_SOURCE_TEST_NODE"),
        );
        let target = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
        HostConfig::fixture(
            data_root,
            "0.17.0".into(),
            format!("{}/index.json", self.server.url()),
            BTreeMap::from([(
                "fixture-current".into(),
                self.key.public_key().as_ref().try_into().unwrap(),
            )]),
            RuntimePolicy::ChannelBundled {
                executable: runtime,
                version: "24.22.1".into(),
                target,
            },
        )
    }
    pub(crate) async fn publish(&mut self, packages: Vec<FixturePackage>) {
        self.server.reset();
        self.generation += 1;
        let mut sources = Vec::new();
        for package in packages {
            let mut archive = tar::Builder::new(Vec::new());
            for (path, bytes) in [
                (
                    "manifest.json",
                    serde_json::to_vec(&package.manifest).unwrap(),
                ),
                ("worker.mjs", package.worker.into_bytes()),
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
            let bytes = archive.into_inner().unwrap();
            let path = format!(
                "/{}-{}.tar",
                package.manifest.source_id, package.manifest.version
            );
            let artifact = json!({"url":format!("{}{path}",self.server.url()),"size":bytes.len(),"sha256":digest(&bytes),"format":"tar"});
            self.server
                .mock("GET", path.as_str())
                .with_body(bytes)
                .create_async()
                .await;
            let target = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
            sources.push(json!({"source_id":package.manifest.source_id,"publisher":package.manifest.publisher,"package_id":package.manifest.package_id,"releases":[{"manifest":package.manifest,"archive":artifact,"runtime":{"version":"24.22.1","targets":{(target):{"url":"https://nodejs.org/fixture-never-download.tar.gz","size":1,"sha256":"00".repeat(32),"format":"tar.gz","executable":"node/bin/node"}}}}],"revoked_versions":[]}));
        }
        let payload = serde_json::to_vec(
            &json!({"schema_version":1,"generation":self.generation,"sources":sources}),
        )
        .unwrap();
        let envelope = json!({"key_id":"fixture-current","payload":base64::engine::general_purpose::STANDARD.encode(&payload),"signature":base64::engine::general_purpose::STANDARD.encode(self.key.sign(&payload).as_ref())});
        self.server
            .mock("GET", "/index.json")
            .with_body(serde_json::to_vec(&envelope).unwrap())
            .create_async()
            .await;
    }
}
