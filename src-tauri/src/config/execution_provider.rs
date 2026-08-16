use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::{load_config, save_config, AppConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProviderPreference {
    Cpu,
    Xnnpack,
    CoreMl,
    #[serde(alias = "directml")]
    DirectMl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum ExecutionProviderPlatform {
    MacosAppleSilicon,
    MacosIntel,
    Windows,
    Linux,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExecutionProviderCapabilities {
    directml: bool,
    coreml: bool,
    xnnpack: bool,
}

impl ExecutionProviderCapabilities {
    fn current() -> Self {
        Self {
            directml: crate::platform_capabilities::directml_available(),
            coreml: crate::platform_capabilities::coreml_available(),
            xnnpack: crate::platform_capabilities::xnnpack_available(),
        }
    }

    fn for_platform(platform: ExecutionProviderPlatform) -> Self {
        let current_platform = ExecutionProviderPlatform::current();
        Self {
            directml: platform == ExecutionProviderPlatform::Windows
                && current_platform == ExecutionProviderPlatform::Windows
                && crate::platform_capabilities::directml_available(),
            coreml: platform == ExecutionProviderPlatform::MacosAppleSilicon,
            xnnpack: matches!(
                platform,
                ExecutionProviderPlatform::MacosIntel | ExecutionProviderPlatform::Linux
            ),
        }
    }
}

impl ExecutionProviderPlatform {
    fn current() -> Self {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            Self::MacosAppleSilicon
        }
        #[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
        {
            Self::MacosIntel
        }
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(target_os = "linux")]
        {
            Self::Linux
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            Self::Other
        }
    }
}

impl Default for ExecutionProviderPreference {
    fn default() -> Self {
        Self::default_for_current_platform()
    }
}

impl ExecutionProviderPreference {
    fn available_for(platform: ExecutionProviderPlatform) -> &'static [Self] {
        match platform {
            ExecutionProviderPlatform::Windows => &[Self::Cpu, Self::Xnnpack, Self::DirectMl],
            ExecutionProviderPlatform::MacosAppleSilicon => {
                &[Self::Cpu, Self::CoreMl, Self::Xnnpack]
            }
            ExecutionProviderPlatform::MacosIntel | ExecutionProviderPlatform::Linux => {
                &[Self::Cpu, Self::Xnnpack]
            }
            ExecutionProviderPlatform::Other => &[Self::Cpu],
        }
    }

    /// Platform default EP: CoreML on Apple Silicon when `capabilities.coreml`
    /// is available (else CPU); DirectML on Windows with D3D12 hardware;
    /// otherwise CPU (XNNPACK loses to ORT CPU on Intel/Linux).
    fn default_for(platform: ExecutionProviderPlatform) -> Self {
        Self::default_for_capabilities(
            platform,
            ExecutionProviderCapabilities::for_platform(platform),
        )
    }

    fn default_for_capabilities(
        platform: ExecutionProviderPlatform,
        capabilities: ExecutionProviderCapabilities,
    ) -> Self {
        match platform {
            ExecutionProviderPlatform::Windows if capabilities.directml => Self::DirectMl,
            ExecutionProviderPlatform::Windows => Self::Cpu,
            ExecutionProviderPlatform::MacosAppleSilicon if capabilities.coreml => Self::CoreMl,
            ExecutionProviderPlatform::MacosIntel
            | ExecutionProviderPlatform::Linux
            | ExecutionProviderPlatform::Other => Self::Cpu,
            ExecutionProviderPlatform::MacosAppleSilicon => Self::Cpu,
        }
    }

    fn is_available_for(self, platform: ExecutionProviderPlatform) -> bool {
        Self::available_for(platform).contains(&self)
    }

    fn is_compatible_for(
        self,
        platform: ExecutionProviderPlatform,
        capabilities: ExecutionProviderCapabilities,
    ) -> bool {
        match self {
            Self::Cpu => true,
            Self::Xnnpack => {
                matches!(
                    platform,
                    ExecutionProviderPlatform::MacosIntel | ExecutionProviderPlatform::Linux
                ) && capabilities.xnnpack
            }
            Self::CoreMl => {
                platform == ExecutionProviderPlatform::MacosAppleSilicon && capabilities.coreml
            }
            Self::DirectMl => {
                platform == ExecutionProviderPlatform::Windows && capabilities.directml
            }
        }
    }

    pub fn default_for_current_platform() -> Self {
        Self::default_for(ExecutionProviderPlatform::current())
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Xnnpack => "xnnpack",
            Self::CoreMl => "coreml",
            Self::DirectMl => "directml",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cpu" => Some(Self::Cpu),
            "xnnpack" => Some(Self::Xnnpack),
            "coreml" => Some(Self::CoreMl),
            "directml" => Some(Self::DirectMl),
            _ => None,
        }
    }

    pub fn available_for_current_platform() -> Vec<&'static str> {
        Self::available_for(ExecutionProviderPlatform::current())
            .iter()
            .map(|ep| ep.as_str())
            .collect()
    }

    pub fn compatible_for_current_platform() -> Vec<&'static str> {
        let platform = ExecutionProviderPlatform::current();
        let capabilities = ExecutionProviderCapabilities::current();
        Self::available_for(platform)
            .iter()
            .copied()
            .filter(|provider| provider.is_compatible_for(platform, capabilities))
            .map(Self::as_str)
            .collect()
    }

    pub fn is_compatible_for_current_platform(self) -> bool {
        self.is_compatible_for(
            ExecutionProviderPlatform::current(),
            ExecutionProviderCapabilities::current(),
        )
    }

    pub fn is_available_for_current_platform(self) -> bool {
        self.is_available_for(ExecutionProviderPlatform::current())
    }
}

impl AppConfig {
    fn effective_execution_provider_for(
        &self,
        platform: ExecutionProviderPlatform,
    ) -> ExecutionProviderPreference {
        match self.execution_provider {
            Some(ep) if ep.is_available_for(platform) => ep,
            _ => self.default_execution_provider_for(platform),
        }
    }

    /// Platform default with the DirectML-timeout disable honored. When the
    /// host recorded a DirectML load timeout, the Windows default downgrades
    /// from DirectML to CPU so the next bootstrap selects a CPU-only runtime.
    fn default_execution_provider_for(
        &self,
        platform: ExecutionProviderPlatform,
    ) -> ExecutionProviderPreference {
        if self.directml_disabled_by_runtime_timeout.is_some()
            && platform == ExecutionProviderPlatform::Windows
            && ExecutionProviderPreference::default_for(platform)
                == ExecutionProviderPreference::DirectMl
        {
            ExecutionProviderPreference::Cpu
        } else {
            ExecutionProviderPreference::default_for(platform)
        }
    }

    pub fn effective_execution_provider(&self) -> ExecutionProviderPreference {
        self.effective_execution_provider_for(ExecutionProviderPlatform::current())
    }
}

/// Resolve the execution provider a runtime selection should target, reading
/// the persisted config from `app_data_dir`. Falls back to the platform
/// default when the config is missing or unreadable. Runtime catalog
/// resolution (which picks a CPU-only vs DirectML Windows runtime) calls this
/// so a host that recorded a DirectML load timeout resolves the CPU runtime
/// even before the OS capability probe is consulted.
pub fn effective_execution_provider_from_dir(app_data_dir: &Path) -> ExecutionProviderPreference {
    match load_config(app_data_dir) {
        Ok(Some(config)) => config.effective_execution_provider(),
        _ => ExecutionProviderPreference::default_for_current_platform(),
    }
}

const DIRECTML_RUNTIME_TIMEOUT_REASON: &str = "directml-runtime-load-timeout";

pub fn record_directml_unavailable_on_timeout(
    app_data_dir: &Path,
    runtime_execution_providers: &[String],
    error_message: &str,
) -> Result<bool> {
    if !error_message
        .contains(crate::commands::runtime_worker::RUNTIME_POST_DOWNLOAD_TIMEOUT_MARKER)
    {
        return Ok(false);
    }
    let runtime_advertises_directml = runtime_execution_providers
        .iter()
        .any(|provider| provider.eq_ignore_ascii_case("directml"));
    if !runtime_advertises_directml {
        return Ok(false);
    }

    crate::platform_capabilities::set_directml_disabled_by_timeout(true);

    let mut config = load_config(app_data_dir)
        .with_context(|| format!("failed to load config from {}", app_data_dir.display()))?
        .unwrap_or_default();
    if config.directml_disabled_by_runtime_timeout.as_deref()
        == Some(DIRECTML_RUNTIME_TIMEOUT_REASON)
    {
        return Ok(true);
    }
    config.directml_disabled_by_runtime_timeout = Some(DIRECTML_RUNTIME_TIMEOUT_REASON.to_owned());
    save_config(app_data_dir, &config)
        .with_context(|| format!("failed to save config to {}", app_data_dir.display()))?;
    Ok(true)
}

/// Restore the process-level DirectML timeout override from the persisted
/// config so startup snapshots (`directml_available`, `cpu_fallback_notice_for`)
/// observe the recorded state without waiting for a fresh timeout to fire.
pub fn restore_directml_timeout_state(app_config: Option<&AppConfig>) {
    let disabled = app_config
        .and_then(|config| config.directml_disabled_by_runtime_timeout.as_ref())
        .is_some();
    crate::platform_capabilities::set_directml_disabled_by_timeout(disabled);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform_capabilities::directml_timeout_test_guard;

    #[test]
    fn execution_provider_policy_holds_for_every_platform() {
        use ExecutionProviderPlatform as Platform;
        use ExecutionProviderPreference as Ep;

        let _guard = directml_timeout_test_guard();

        assert!(Ep::DirectMl.is_available_for(Platform::Windows));
        for platform in [
            Platform::MacosAppleSilicon,
            Platform::MacosIntel,
            Platform::Linux,
            Platform::Other,
        ] {
            assert!(
                !Ep::DirectMl.is_available_for(platform),
                "DirectML must not be offered on {platform:?}"
            );
        }

        for platform in [
            Platform::Windows,
            Platform::MacosAppleSilicon,
            Platform::MacosIntel,
            Platform::Linux,
        ] {
            assert!(Ep::Cpu.is_available_for(platform));
            assert!(Ep::Xnnpack.is_available_for(platform));
        }
        assert!(Ep::Cpu.is_available_for(Platform::Other));
        assert!(Ep::CoreMl.is_available_for(Platform::MacosAppleSilicon));
        assert!(!Ep::CoreMl.is_available_for(Platform::MacosIntel));

        assert_eq!(Ep::default_for(Platform::MacosAppleSilicon), Ep::CoreMl);
        assert_eq!(
            Ep::default_for(Platform::Windows),
            if crate::platform_capabilities::directml_available() {
                Ep::DirectMl
            } else {
                Ep::Cpu
            }
        );
        for platform in [Platform::MacosIntel, Platform::Linux, Platform::Other] {
            assert_eq!(
                Ep::default_for(platform),
                Ep::Cpu,
                "{platform:?} must default to the ORT CPU EP"
            );
        }

        for platform in [
            Platform::Windows,
            Platform::MacosAppleSilicon,
            Platform::MacosIntel,
            Platform::Linux,
            Platform::Other,
        ] {
            assert!(Ep::default_for(platform).is_available_for(platform));
        }
    }

    #[test]
    fn windows_auto_selection_requires_a_hardware_capability() {
        use ExecutionProviderPlatform::Windows;
        use ExecutionProviderPreference as Ep;

        let hardware_available = ExecutionProviderCapabilities {
            directml: true,
            coreml: false,
            xnnpack: false,
        };
        let hardware_unavailable = ExecutionProviderCapabilities {
            directml: false,
            coreml: false,
            xnnpack: false,
        };
        assert_eq!(
            Ep::default_for_capabilities(Windows, hardware_available),
            Ep::DirectMl
        );
        assert_eq!(
            Ep::default_for_capabilities(Windows, hardware_unavailable),
            Ep::Cpu
        );
        assert_eq!(
            Ep::default_for_capabilities(ExecutionProviderPlatform::Linux, hardware_available),
            Ep::Cpu
        );
    }

    #[test]
    fn execution_provider_none_is_omitted_from_json() {
        let config = AppConfig {
            library_path: None,
            libraries: vec![],
            active_library_id: None,
            stem_mode: None,
            language: None,
            hide_batch_separate: None,
            cover_art_backdrop: None,
            lyrics_blur_inactive: None,
            hide_upgrade_all: None,
            model_variant: None,
            lyrics_font_step: None,
            execution_provider: None,
            eq_enabled: None,
            eq_gains_db: None,
            crossfade_enabled: None,
            crossfade_duration_ms: None,
            library_sort_mode: None,
            theme_preference: None,
            update_policy: None,
            youtube_source_enabled: None,
            netease_source_enabled: None,
            remote_cache_bytes_limit: None,
            pending_mirror_restore: false,
            pending_mirror_restore_active_library_id: None,
            directml_disabled_by_runtime_timeout: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("execution_provider"));
    }

    #[test]
    fn execution_provider_round_trips_through_json() {
        let config = AppConfig {
            execution_provider: Some(ExecutionProviderPreference::Xnnpack),
            library_sort_mode: None,
            theme_preference: None,
            update_policy: None,
            youtube_source_enabled: None,
            netease_source_enabled: None,
            ..AppConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let loaded: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(
            loaded.execution_provider,
            Some(ExecutionProviderPreference::Xnnpack)
        );
    }

    #[test]
    fn available_execution_providers_are_explicit_only() {
        let providers = ExecutionProviderPreference::available_for_current_platform();
        assert!(!providers.contains(&"auto"));
        assert!(providers.contains(&"cpu"));
        assert!(providers.contains(&"xnnpack"));

        #[cfg(target_os = "windows")]
        assert!(providers.contains(&"directml"));
    }

    #[test]
    fn effective_execution_provider_defaults_to_platform_default() {
        let _guard = directml_timeout_test_guard();
        let config = AppConfig::default();

        // Host-conditional expectations mirror the runtime artifact matrix:
        // Windows selects DirectML only with a D3D12 hardware adapter, Apple
        // Silicon selects CoreML, and other hosts use CPU.
        #[cfg(target_os = "windows")]
        assert_eq!(
            config.effective_execution_provider(),
            ExecutionProviderPreference::default_for_current_platform()
        );

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        assert_eq!(
            config.effective_execution_provider(),
            ExecutionProviderPreference::CoreMl
        );

        #[cfg(not(any(
            target_os = "windows",
            all(target_os = "macos", target_arch = "aarch64")
        )))]
        assert_eq!(
            config.effective_execution_provider(),
            ExecutionProviderPreference::Cpu
        );
    }

    #[test]
    fn execution_provider_available_table_is_exact_and_ordered() {
        use ExecutionProviderPlatform::*;

        assert_eq!(
            ExecutionProviderPreference::available_for(MacosAppleSilicon),
            &[
                ExecutionProviderPreference::Cpu,
                ExecutionProviderPreference::CoreMl,
                ExecutionProviderPreference::Xnnpack,
            ]
        );
        assert_eq!(
            ExecutionProviderPreference::available_for(MacosIntel),
            &[
                ExecutionProviderPreference::Cpu,
                ExecutionProviderPreference::Xnnpack
            ]
        );
        assert_eq!(
            ExecutionProviderPreference::available_for(Linux),
            &[
                ExecutionProviderPreference::Cpu,
                ExecutionProviderPreference::Xnnpack
            ]
        );
        assert_eq!(
            ExecutionProviderPreference::available_for(Other),
            &[ExecutionProviderPreference::Cpu]
        );
        assert_eq!(
            ExecutionProviderPreference::available_for(Windows),
            &[
                ExecutionProviderPreference::Cpu,
                ExecutionProviderPreference::Xnnpack,
                ExecutionProviderPreference::DirectMl,
            ]
        );
    }

    #[test]
    fn directml_is_present_only_for_windows() {
        use ExecutionProviderPlatform::*;
        for &platform in &[MacosAppleSilicon, MacosIntel, Linux, Other] {
            assert!(!ExecutionProviderPreference::available_for(platform)
                .contains(&ExecutionProviderPreference::DirectMl));
        }
        assert!(ExecutionProviderPreference::available_for(Windows)
            .contains(&ExecutionProviderPreference::DirectMl));
    }

    #[test]
    fn cpu_is_present_for_every_target() {
        use ExecutionProviderPlatform::*;
        for &platform in &[MacosAppleSilicon, MacosIntel, Windows, Linux, Other] {
            let list = ExecutionProviderPreference::available_for(platform);
            assert!(list.contains(&ExecutionProviderPreference::Cpu));
        }
    }

    #[test]
    fn provider_capabilities_match_runtime_artifacts() {
        use ExecutionProviderPlatform::*;
        use ExecutionProviderPreference as Ep;

        let available = ExecutionProviderCapabilities {
            directml: true,
            coreml: true,
            xnnpack: true,
        };
        assert_eq!(
            Ep::default_for_capabilities(MacosAppleSilicon, available),
            Ep::CoreMl
        );
        assert!(Ep::CoreMl.is_compatible_for(MacosAppleSilicon, available));
        assert!(!Ep::CoreMl.is_compatible_for(MacosIntel, available));
        assert!(Ep::Xnnpack.is_compatible_for(Linux, available));
        assert!(!Ep::Xnnpack.is_compatible_for(Windows, available));
        assert!(Ep::DirectMl.is_compatible_for(Windows, available));
        assert!(!Ep::DirectMl.is_compatible_for(Linux, available));

        let unavailable = ExecutionProviderCapabilities {
            directml: false,
            coreml: false,
            xnnpack: false,
        };
        assert_eq!(
            Ep::default_for_capabilities(MacosAppleSilicon, unavailable),
            Ep::Cpu
        );
        assert!(!Ep::CoreMl.is_compatible_for(MacosAppleSilicon, unavailable));
        assert!(!Ep::Xnnpack.is_compatible_for(Linux, unavailable));
        assert!(!Ep::DirectMl.is_compatible_for(Windows, unavailable));
    }

    #[test]
    fn every_target_default_is_a_member_of_its_list() {
        use ExecutionProviderPlatform::*;
        for &platform in &[MacosAppleSilicon, MacosIntel, Windows, Linux, Other] {
            let default = ExecutionProviderPreference::default_for(platform);
            assert!(default.is_available_for(platform));
        }
    }

    #[test]
    fn effective_execution_provider_for_preserves_valid_saved_value() {
        use ExecutionProviderPlatform::*;
        let config = AppConfig {
            execution_provider: Some(ExecutionProviderPreference::Cpu),
            ..AppConfig::default()
        };
        assert_eq!(
            config.effective_execution_provider_for(MacosAppleSilicon),
            ExecutionProviderPreference::Cpu
        );
        assert_eq!(
            config.effective_execution_provider_for(Windows),
            ExecutionProviderPreference::Cpu
        );
    }

    #[test]
    fn effective_execution_provider_for_falls_back_for_stale_cross_platform_value() {
        use ExecutionProviderPlatform::*;
        let config = AppConfig {
            execution_provider: Some(ExecutionProviderPreference::DirectMl),
            ..AppConfig::default()
        };
        assert_eq!(
            config.effective_execution_provider_for(MacosAppleSilicon),
            ExecutionProviderPreference::CoreMl
        );
        assert_eq!(
            config.effective_execution_provider_for(MacosIntel),
            ExecutionProviderPreference::Cpu
        );
        assert_eq!(
            config.effective_execution_provider_for(Linux),
            ExecutionProviderPreference::Cpu
        );
        assert_eq!(
            config.effective_execution_provider_for(Other),
            ExecutionProviderPreference::Cpu
        );
        assert_eq!(
            config.effective_execution_provider_for(Windows),
            ExecutionProviderPreference::DirectMl
        );
    }

    #[test]
    fn effective_execution_provider_for_falls_back_when_unset() {
        use ExecutionProviderPlatform::*;
        let _guard = directml_timeout_test_guard();
        let config = AppConfig {
            execution_provider: None,
            ..AppConfig::default()
        };
        assert_eq!(
            config.effective_execution_provider_for(MacosAppleSilicon),
            ExecutionProviderPreference::CoreMl
        );
        assert_eq!(
            config.effective_execution_provider_for(Windows),
            ExecutionProviderPreference::default_for(Windows)
        );
        assert_eq!(
            config.effective_execution_provider_for(MacosIntel),
            ExecutionProviderPreference::Cpu
        );
        assert_eq!(
            config.effective_execution_provider_for(Linux),
            ExecutionProviderPreference::Cpu
        );
    }

    #[test]
    fn current_target_wrappers_agree_with_compile_target() {
        use ExecutionProviderPlatform::*;
        let current = ExecutionProviderPlatform::current();
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        assert_eq!(current, MacosAppleSilicon);
        #[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
        assert_eq!(current, MacosIntel);
        #[cfg(target_os = "windows")]
        assert_eq!(current, Windows);
        #[cfg(target_os = "linux")]
        assert_eq!(current, Linux);
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        assert_eq!(current, Other);
    }

    #[test]
    fn directml_timeout_disable_downgrades_windows_default_to_cpu() {
        let _guard = directml_timeout_test_guard();
        crate::platform_capabilities::set_directml_disabled_by_timeout(false);
        let config = AppConfig {
            directml_disabled_by_runtime_timeout: Some("directml-runtime-load-timeout".to_owned()),
            ..AppConfig::default()
        };
        let resolved = config.effective_execution_provider_for(ExecutionProviderPlatform::Windows);
        assert_eq!(resolved, ExecutionProviderPreference::Cpu);
        crate::platform_capabilities::set_directml_disabled_by_timeout(false);
    }

    #[test]
    fn explicit_user_execution_provider_overrides_timeout_disable() {
        let _guard = directml_timeout_test_guard();
        crate::platform_capabilities::set_directml_disabled_by_timeout(false);
        let config = AppConfig {
            execution_provider: Some(ExecutionProviderPreference::DirectMl),
            directml_disabled_by_runtime_timeout: Some("directml-runtime-load-timeout".to_owned()),
            ..AppConfig::default()
        };
        let resolved = config.effective_execution_provider_for(ExecutionProviderPlatform::Windows);
        assert_eq!(resolved, ExecutionProviderPreference::DirectMl);
        crate::platform_capabilities::set_directml_disabled_by_timeout(false);
    }

    #[test]
    fn record_directml_unavailable_ignores_non_timeout_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let providers = vec!["directml".to_owned()];
        let recorded = record_directml_unavailable_on_timeout(
            tmp.path(),
            &providers,
            "some unrelated load error",
        )
        .unwrap();
        assert!(!recorded);
        let loaded = load_config(tmp.path()).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn record_directml_unavailable_ignores_cpu_only_runtime_timeouts() {
        let tmp = tempfile::tempdir().unwrap();
        let providers = vec!["cpu".to_owned()];
        let marker = crate::commands::runtime_worker::RUNTIME_POST_DOWNLOAD_TIMEOUT_MARKER;
        let recorded = record_directml_unavailable_on_timeout(
            tmp.path(),
            &providers,
            &format!("{marker}: timed out"),
        )
        .unwrap();
        assert!(!recorded);
    }

    #[test]
    fn record_directml_unavailable_persists_flag_and_flips_process_override() {
        let _guard = directml_timeout_test_guard();
        let tmp = tempfile::tempdir().unwrap();
        crate::platform_capabilities::set_directml_disabled_by_timeout(false);
        let providers = vec!["directml".to_owned(), "cpu".to_owned()];
        let marker = crate::commands::runtime_worker::RUNTIME_POST_DOWNLOAD_TIMEOUT_MARKER;
        let recorded = record_directml_unavailable_on_timeout(
            tmp.path(),
            &providers,
            &format!("{marker}: timed out"),
        )
        .unwrap();
        assert!(recorded);
        assert!(crate::platform_capabilities::directml_disabled_by_timeout());
        let loaded = load_config(tmp.path()).unwrap().unwrap();
        assert_eq!(
            loaded.directml_disabled_by_runtime_timeout.as_deref(),
            Some("directml-runtime-load-timeout"),
        );
        crate::platform_capabilities::set_directml_disabled_by_timeout(false);
    }
}
