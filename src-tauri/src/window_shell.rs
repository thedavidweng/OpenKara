use serde::Serialize;
#[cfg(target_os = "macos")]
use tauri::Manager;
use tauri::Runtime;

const DESKTOP_TOOLBAR_HEIGHT: u16 = 48;
#[cfg(target_os = "macos")]
const MAC_TOOLBAR_HEIGHT: u16 = 48;
const DESKTOP_TRAFFIC_LIGHT_INSET_LEADING: u16 = 0;
#[cfg(target_os = "macos")]
const MAC_TRAFFIC_LIGHT_INSET_LEADING: u16 = 78;
const DESKTOP_SIDEBAR_HEADER_HEIGHT: u16 = 0;
#[cfg(target_os = "macos")]
const MAC_SIDEBAR_HEADER_HEIGHT: u16 = 28;
const DEFAULT_SIDEBAR_WIDTH: u16 = 260;
#[cfg(target_os = "macos")]
const MAC_SIDEBAR_WIDTH: u16 = 260;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowChromeVariant {
    Desktop,
    // The `Mac` variant is only constructed on macOS, but must exist on all
    // platforms for serde serialization compatibility — the frontend expects
    // `mac` in the JSON payload regardless of the backend platform.
    #[allow(dead_code)]
    Mac,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowShellTier {
    Desktop,
    #[allow(dead_code)]
    Mac,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowShellState {
    pub chrome_variant: WindowChromeVariant,
    pub tier: WindowShellTier,
    pub toolbar_height_px: u16,
    pub traffic_light_inset_leading: u16,
    pub sidebar_header_height_px: u16,
    pub sidebar_width_px: u16,
}

impl WindowShellState {
    pub fn desktop() -> Self {
        Self {
            chrome_variant: WindowChromeVariant::Desktop,
            tier: WindowShellTier::Desktop,
            toolbar_height_px: DESKTOP_TOOLBAR_HEIGHT,
            traffic_light_inset_leading: DESKTOP_TRAFFIC_LIGHT_INSET_LEADING,
            sidebar_header_height_px: DESKTOP_SIDEBAR_HEADER_HEIGHT,
            sidebar_width_px: DEFAULT_SIDEBAR_WIDTH,
        }
    }

    #[cfg(target_os = "macos")]
    pub fn mac() -> Self {
        Self {
            chrome_variant: WindowChromeVariant::Mac,
            tier: WindowShellTier::Mac,
            toolbar_height_px: MAC_TOOLBAR_HEIGHT,
            traffic_light_inset_leading: MAC_TRAFFIC_LIGHT_INSET_LEADING,
            sidebar_header_height_px: MAC_SIDEBAR_HEADER_HEIGHT,
            sidebar_width_px: MAC_SIDEBAR_WIDTH,
        }
    }

    #[cfg(target_os = "macos")]
    fn with_native_metrics(
        mut self,
        toolbar_height: u16,
        traffic_light_inset_leading: u16,
        sidebar_header_height: u16,
    ) -> Self {
        self.toolbar_height_px = toolbar_height;
        self.traffic_light_inset_leading = traffic_light_inset_leading;
        self.sidebar_header_height_px = sidebar_header_height;
        self
    }
}

pub fn initialize_main_window<R: Runtime>(
    app: &tauri::App<R>,
    app_config: Option<&crate::config::AppConfig>,
) -> WindowShellState {
    #[cfg(target_os = "macos")]
    {
        let _ = app_config;
        let detected = native::detect_window_shell_state().unwrap_or_else(WindowShellState::mac);

        let Some(window) = app.get_webview_window("main") else {
            return WindowShellState::mac();
        };

        let fallback_to_mac = || {
            let _ = native::apply_main_window_shell(&window, &WindowShellState::mac());
            WindowShellState::mac()
        };

        match native::apply_main_window_shell(&window, &detected) {
            Ok(Some(applied)) => applied,
            Ok(None) => fallback_to_mac(),
            Err(_) => fallback_to_mac(),
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, app_config);
        WindowShellState::desktop()
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::{WindowShellState, WindowShellTier};
    use std::ffi::c_void;
    use tauri::{Runtime, WebviewWindow};

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    struct NativeWindowShellProfile {
        macos_major_version: isize,
        tier_tag: isize,
        toolbar_height: isize,
        traffic_light_inset_leading: isize,
        sidebar_header_height: isize,
    }

    unsafe extern "C" {
        fn ok_window_shell_detect_profile(profile_out: *mut NativeWindowShellProfile);
        fn ok_window_shell_configure_main_window(
            ns_view_ptr: *mut c_void,
            tier_tag: isize,
            toolbar_height: f64,
            traffic_light_inset_leading: f64,
            sidebar_header_height: f64,
            profile_out: *mut NativeWindowShellProfile,
        ) -> bool;
    }

    fn window_shell_state_from_profile(
        profile: NativeWindowShellProfile,
    ) -> Option<WindowShellState> {
        let toolbar_height = u16::try_from(profile.toolbar_height).ok()?;
        let traffic_light_inset_leading =
            u16::try_from(profile.traffic_light_inset_leading).ok()?;
        let sidebar_header_height = u16::try_from(profile.sidebar_header_height).ok()?;
        let base_state = match profile.tier_tag {
            0 => WindowShellState::desktop(),
            1 => WindowShellState::mac(),
            _ => return None,
        };

        Some(base_state.with_native_metrics(
            toolbar_height,
            traffic_light_inset_leading,
            sidebar_header_height,
        ))
    }

    pub(super) fn detect_window_shell_state() -> Option<WindowShellState> {
        let mut profile = NativeWindowShellProfile {
            macos_major_version: 0,
            tier_tag: 0,
            toolbar_height: 0,
            traffic_light_inset_leading: 0,
            sidebar_header_height: 0,
        };

        // SAFETY: The Objective-C bridge writes a small POD struct into the out pointer.
        unsafe {
            ok_window_shell_detect_profile(&mut profile);
        }

        window_shell_state_from_profile(profile)
    }

    pub(super) fn apply_main_window_shell<R: Runtime>(
        window: &WebviewWindow<R>,
        state: &WindowShellState,
    ) -> anyhow::Result<Option<WindowShellState>> {
        let ns_view = window
            .ns_view()
            .map_err(|error| anyhow::anyhow!("failed to get ns_view: {error}"))?;

        let tier_tag = match state.tier {
            WindowShellTier::Desktop => 0,
            WindowShellTier::Mac => 1,
        };

        let mut resolved_profile = NativeWindowShellProfile {
            macos_major_version: 0,
            tier_tag,
            toolbar_height: state.toolbar_height_px as isize,
            traffic_light_inset_leading: state.traffic_light_inset_leading as isize,
            sidebar_header_height: state.sidebar_header_height_px as isize,
        };

        // SAFETY: Tauri exposes the live NSView pointer for the current webview window.
        let configured = unsafe {
            ok_window_shell_configure_main_window(
                ns_view,
                tier_tag,
                f64::from(state.toolbar_height_px),
                f64::from(state.traffic_light_inset_leading),
                f64::from(state.sidebar_header_height_px),
                &mut resolved_profile,
            )
        };

        if !configured {
            return Ok(None);
        }

        Ok(window_shell_state_from_profile(resolved_profile))
    }
}

/// Legacy IPC: split-shell sidebar visibility is a no-op now that the product uses
/// a single webview layout; kept so older frontends do not error on invoke.
pub fn set_native_sidebar_visibility_impl<R: Runtime>(
    _webview: &tauri::Webview<R>,
    _visible: bool,
) -> anyhow::Result<()> {
    Ok(())
}

#[tauri::command]
pub fn get_window_shell_state(state: tauri::State<'_, WindowShellState>) -> WindowShellState {
    state.inner().clone()
}

#[tauri::command]
pub fn set_native_sidebar_visibility(
    webview: tauri::Webview,
    visible: bool,
) -> Result<(), crate::commands::error::CommandError> {
    set_native_sidebar_visibility_impl(&webview, visible)
        .map_err(|error| crate::commands::error::internal_error(error.to_string()))
}

#[tauri::command]
pub fn window_ready(
    window: tauri::WebviewWindow,
) -> Result<(), crate::commands::error::CommandError> {
    window
        .show()
        .map_err(|error| crate::commands::error::internal_error(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_shell_state_preserves_existing_non_mac_behavior() {
        let state = WindowShellState::desktop();

        assert_eq!(state.chrome_variant, WindowChromeVariant::Desktop);
        assert_eq!(state.tier, WindowShellTier::Desktop);
        assert_eq!(state.toolbar_height_px, 48);
        assert_eq!(state.traffic_light_inset_leading, 0);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn mac_shell_state_exposes_the_unified_metrics() {
        let state = WindowShellState::mac();

        assert_eq!(state.chrome_variant, WindowChromeVariant::Mac);
        assert_eq!(state.tier, WindowShellTier::Mac);
        assert_eq!(state.toolbar_height_px, 48);
        assert_eq!(state.traffic_light_inset_leading, 78);
        assert_eq!(state.sidebar_header_height_px, 28);
        assert_eq!(state.sidebar_width_px, 260);
    }
}
