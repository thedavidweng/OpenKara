//! In-app updater support probe (#255).

/// Whether the running install can self-update through `tauri-plugin-updater`.
///
/// Only bundles that emit signed updater artifacts are updatable: the AppImage
/// on Linux, the `.app`/DMG on macOS, and the NSIS installer on Windows. A
/// Linux `.deb`, Flatpak, or an unbundled dev binary has no updater artifact of
/// its own. The plugin's `check()` carries no install-format guard — it would
/// fall back to offering the AppImage payload to a `.deb` install and then fail
/// on install — so the frontend gates its launch check on this probe to keep
/// non-AppImage Linux installs silent.
///
/// The bundle format is baked into each artifact at bundling time
/// (`tauri::utils::platform::bundle_type`), so a `.deb` binary reports `Deb`, an
/// AppImage reports `AppImage`, and an unbundled dev binary reports nothing.
#[tauri::command]
pub fn self_update_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        matches!(
            tauri::utils::platform::bundle_type(),
            Some(tauri::utils::config::BundleType::AppImage)
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}
