//! Supply-chain and packaging guard for the app-local VC++ CRT DLLs.
//!
//! `onnxruntime.dll` is built with /MD; its PE import table names exactly the
//! CRT DLLs below. The Windows installer must place each one next to
//! `openkara.exe` (the first directory in the loader's standard search order)
//! or runtime loads fail with `ERROR_MOD_NOT_FOUND` on machines without the
//! VC++ Redistributable (#284). This test pins the repo-committed DLLs to the
//! manifest digests and the manifest to the Windows bundle configuration, so
//! a swapped binary, a forgotten file, or a broken resource mapping fails in
//! CI on every host.

use std::collections::BTreeMap;
use std::path::Path;

use openkara_lib::separator::artifacts::sha256_file;
use serde::Deserialize;

/// The import set of onnxruntime.dll (cpu and directml builds alike), read
/// from the PE import table of the catalog artifacts. `DirectML.dll` and the
/// `api-ms-win-*` / system DLLs are intentionally absent: DirectML ships
/// inside the runtime artifact and the rest are Windows inbox components.
const REQUIRED_CRT_DLLS: [&str; 4] = [
    "msvcp140.dll",
    "msvcp140_1.dll",
    "vcruntime140.dll",
    "vcruntime140_1.dll",
];

#[derive(Deserialize)]
struct VcredistManifest {
    schema_version: String,
    files: BTreeMap<String, VcredistFile>,
}

#[derive(Deserialize)]
struct VcredistFile {
    sha256: String,
    size: u64,
}

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn load_vcredist_manifest() -> VcredistManifest {
    let path = manifest_dir().join("resources/windows/vcredist/manifest.json");
    let bytes = std::fs::read(&path).expect("vcredist manifest must exist");
    serde_json::from_slice(&bytes).expect("vcredist manifest must be valid JSON")
}

#[test]
fn vcredist_manifest_covers_exactly_the_runtime_import_set() {
    let manifest = load_vcredist_manifest();
    assert_eq!(manifest.schema_version, "openkara.windows-vcredist/v1");
    let listed: Vec<&str> = manifest.files.keys().map(String::as_str).collect();
    assert_eq!(
        listed, REQUIRED_CRT_DLLS,
        "manifest must list exactly the CRT DLLs onnxruntime.dll imports"
    );
}

#[test]
fn committed_crt_dlls_match_their_pinned_digests() {
    let manifest = load_vcredist_manifest();
    let dir = manifest_dir().join("resources/windows/vcredist");
    for (name, expected) in &manifest.files {
        let path = dir.join(name);
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("missing bundled CRT DLL {name}: {error}"));
        assert_eq!(
            bytes.len() as u64,
            expected.size,
            "{name} size drifted from the pinned manifest"
        );
        assert!(
            bytes.starts_with(b"MZ"),
            "{name} is not a PE binary; the committed file was replaced"
        );
        let actual = sha256_file(&path).expect("hash bundled CRT DLL");
        assert_eq!(
            &actual, &expected.sha256,
            "{name} content drifted from the pinned manifest"
        );
    }
}

#[test]
fn windows_bundle_places_every_crt_dll_beside_the_executable() {
    let manifest = load_vcredist_manifest();
    let conf: serde_json::Value = serde_json::from_slice(
        &std::fs::read(manifest_dir().join("tauri.windows.conf.json"))
            .expect("tauri.windows.conf.json must exist"),
    )
    .expect("tauri.windows.conf.json must be valid JSON");
    let resources = conf
        .pointer("/bundle/resources")
        .and_then(serde_json::Value::as_object)
        .expect("Windows bundle config must declare bundle.resources");

    for name in manifest.files.keys() {
        let src = format!("resources/windows/vcredist/{name}");
        let dest = resources.get(&src).and_then(serde_json::Value::as_str);
        assert_eq!(
            dest,
            Some(name.as_str()),
            "{name} must be mapped to the install root so the loader's \
             standard search order (application directory first) resolves it"
        );
    }
}

#[test]
fn crt_dlls_are_not_bundled_on_other_platforms() {
    let base = std::fs::read_to_string(manifest_dir().join("tauri.conf.json"))
        .expect("tauri.conf.json must exist");
    assert!(
        !base.contains("vcredist"),
        "the VC++ CRT DLLs are Windows-only; keep them out of the shared bundle config"
    );
}
