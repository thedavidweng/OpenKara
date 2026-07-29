//! In-app updater support probe (#255).

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
