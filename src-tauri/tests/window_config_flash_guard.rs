//! Guards the launch-flash fix in the Tauri window configuration.
//!
//! Tauri merges the platform override files into `tauri.conf.json` with RFC
//! 7396 JSON Merge Patch, under which an array in the patch REPLACES the base
//! array wholesale. Because every platform file redefines the whole
//! `app.windows` array, keys that only exist in the base config (`visible`,
//! `center`, `backgroundColor`) are silently dropped on that platform — which
//! is exactly how the window ended up visible during the webview load and
//! flashed white before the dark UI painted.
//!
//! These assertions fail if a window object stops carrying the keys itself.

use serde_json::Value;
use std::fs;
use std::path::Path;

const CONFIG_FILES: [&str; 4] = [
    "tauri.conf.json",
    "tauri.macos.conf.json",
    "tauri.windows.conf.json",
    "tauri.linux.conf.json",
];

fn windows_array(file: &str) -> Vec<Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    let raw = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("failed to read {}: {error}", path.display());
    });
    let config: Value = serde_json::from_str(&raw)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));

    config
        .get("app")
        .and_then(|app| app.get("windows"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| panic!("{} declares no app.windows array", path.display()))
}

#[test]
fn every_platform_window_starts_hidden_so_the_webview_load_is_never_shown() {
    for file in CONFIG_FILES {
        for window in windows_array(file) {
            assert_eq!(
                window.get("visible"),
                Some(&Value::Bool(false)),
                "{file}: window must set visible:false itself; the platform merge \
                 replaces the whole windows array, so inheriting it from the base \
                 config does not work"
            );
        }
    }
}

#[test]
fn every_platform_window_paints_a_dark_background_before_the_document_loads() {
    for file in CONFIG_FILES {
        for window in windows_array(file) {
            assert_eq!(
                window.get("backgroundColor"),
                Some(&Value::String("#121212".to_owned())),
                "{file}: window must pin a dark backgroundColor so the webview's \
                 default white surface can never be exposed"
            );
        }
    }
}

#[test]
fn every_platform_window_keeps_the_centered_placement() {
    for file in CONFIG_FILES {
        for window in windows_array(file) {
            assert_eq!(
                window.get("center"),
                Some(&Value::Bool(true)),
                "{file}: window must set center:true itself for the same \
                 array-replacement reason as visible:false"
            );
        }
    }
}
