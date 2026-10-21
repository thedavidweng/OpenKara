//! Guards the asset-protocol configuration the library grid depends on.
//!
//! Cover-art thumbnails are served straight off disk through `convertFileSrc`.
//! Three pieces of static configuration have to agree for that to work, and all
//! three fail *silently*: the image request is simply denied and the component
//! falls back to reading bytes over IPC, so the app keeps working while the
//! optimisation quietly stops existing.
//!
//! Tauri merges the platform override files with RFC 7396 JSON Merge Patch. An
//! object key present only in the base survives the merge, but a platform file
//! that redefines `app.security.csp` would replace the string wholesale — which
//! is how the launch-flash regression happened for `app.windows`.

use serde_json::Value;
use std::fs;
use std::path::Path;

const PLATFORM_CONFIG_FILES: [&str; 3] = [
    "tauri.macos.conf.json",
    "tauri.windows.conf.json",
    "tauri.linux.conf.json",
];

fn config(file: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

#[test]
fn asset_protocol_is_enabled_for_cover_art_thumbnails() {
    let enabled = config("tauri.conf.json")["app"]["security"]["assetProtocol"]["enable"]
        .as_bool()
        .expect("app.security.assetProtocol.enable must be declared");

    assert!(
        enabled,
        "the library grid serves thumbnails through the asset protocol"
    );
}

#[test]
fn asset_protocol_scope_is_granted_at_runtime_only() {
    let scope = config("tauri.conf.json")["app"]["security"]["assetProtocol"]["scope"]
        .as_array()
        .expect("app.security.assetProtocol.scope must be declared")
        .clone();

    // The library root is user-relocatable, so a static scope entry could never
    // name the right directory. `get_library` grants the active artwork
    // directory instead. A non-empty static scope here means someone widened
    // the protocol's reach beyond what the app actually hands out.
    assert!(
        scope.is_empty(),
        "static asset scope must stay empty; the artwork directory is granted at runtime"
    );
}

#[test]
fn csp_allows_the_asset_protocol_on_every_platform() {
    let csp = config("tauri.conf.json")["app"]["security"]["csp"]
        .as_str()
        .expect("app.security.csp must be declared")
        .to_owned();

    // macOS serves `asset://localhost`; Windows and Linux serve
    // `http://asset.localhost`. Both tokens are required for one CSP string to
    // cover every platform.
    assert!(
        csp.contains("asset:"),
        "img-src must allow the macOS asset scheme: {csp}"
    );
    assert!(
        csp.contains("http://asset.localhost"),
        "img-src must allow the Windows/Linux asset origin: {csp}"
    );
}

#[test]
fn no_platform_override_replaces_the_security_block() {
    for file in PLATFORM_CONFIG_FILES {
        let security = &config(file)["app"]["security"];
        assert!(
            security.is_null(),
            "{file} declares app.security; the merge patch would replace the CSP \
             string and silently drop the asset protocol on that platform"
        );
    }
}
