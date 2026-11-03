use serde::{Deserialize, Serialize};
use std::path::Path;

use super::{load_config, AppConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProviderPreference {
    Cpu,
    Xnnpack,
    CoreMl,
    #[serde(rename = "webgpu", alias = "directml", alias = "direct_ml")]
    WebGpu,
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
    coreml: bool,
    xnnpack: bool,
}

impl ExecutionProviderCapabilities {
    fn current() -> Self {
        Self {
            coreml: crate::platform_capabilities::coreml_available(),
            xnnpack: crate::platform_capabilities::xnnpack_available(),
        }
    }

    fn for_platform(platform: ExecutionProviderPlatform) -> Self {
        Self {
            coreml: matches!(
                platform,
                ExecutionProviderPlatform::MacosAppleSilicon
                    | ExecutionProviderPlatform::MacosIntel
            ),
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
            ExecutionProviderPlatform::Windows => &[Self::Cpu, Self::WebGpu],
            ExecutionProviderPlatform::MacosAppleSilicon => {
                &[Self::Cpu, Self::CoreMl, Self::WebGpu]
            }
            ExecutionProviderPlatform::MacosIntel => &[Self::Cpu, Self::CoreMl],
            ExecutionProviderPlatform::Linux => &[Self::Cpu],
            ExecutionProviderPlatform::Other => &[Self::Cpu],
        }
    }

    /// Try WebGPU on Apple Silicon and Windows, CoreML on Intel Macs, and CPU elsewhere.
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
            ExecutionProviderPlatform::Windows | ExecutionProviderPlatform::MacosAppleSilicon => {
                Self::WebGpu
            }
            ExecutionProviderPlatform::MacosIntel if capabilities.coreml => Self::CoreMl,
            ExecutionProviderPlatform::MacosIntel
            | ExecutionProviderPlatform::Linux
            | ExecutionProviderPlatform::Other => Self::Cpu,
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
                matches!(
                    platform,
                    ExecutionProviderPlatform::MacosAppleSilicon
                        | ExecutionProviderPlatform::MacosIntel
                ) && capabilities.coreml
            }
            Self::WebGpu => matches!(
                platform,
                ExecutionProviderPlatform::MacosAppleSilicon | ExecutionProviderPlatform::Windows
            ),
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
            Self::WebGpu => "webgpu",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cpu" => Some(Self::Cpu),
            "xnnpack" => Some(Self::Xnnpack),
            "coreml" => Some(Self::CoreMl),
            "webgpu" => Some(Self::WebGpu),
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
            _ => ExecutionProviderPreference::default_for(platform),
        }
    }

    pub fn effective_execution_provider(&self) -> ExecutionProviderPreference {
        self.effective_execution_provider_for(ExecutionProviderPlatform::current())
    }
}

/// Resolve the preferred provider using the saved configuration.
pub fn effective_execution_provider_from_dir(app_data_dir: &Path) -> ExecutionProviderPreference {
    match load_config(app_data_dir) {
        Ok(Some(config)) => config.effective_execution_provider(),
        _ => ExecutionProviderPreference::default_for_current_platform(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webgpu_uses_the_same_name_in_settings_and_config() {
        let provider = ExecutionProviderPreference::WebGpu;
        assert_eq!(serde_json::to_string(&provider).unwrap(), "\"webgpu\"");
        assert_eq!(
            serde_json::from_str::<ExecutionProviderPreference>("\"webgpu\"").unwrap(),
            provider
        );
        assert_eq!(provider.as_str(), "webgpu");
    }
    #[test]
    fn legacy_directml_preference_migrates_without_losing_config() {
        let config: AppConfig =
            serde_json::from_str(r#"{"execution_provider":"directml","language":"en"}"#).unwrap();
        assert_eq!(
            config.execution_provider,
            Some(ExecutionProviderPreference::WebGpu)
        );
        assert_eq!(config.language.as_deref(), Some("en"));
    }

    #[test]
    fn execution_provider_policy_holds_for_every_platform() {
        use ExecutionProviderPlatform as Platform;
        use ExecutionProviderPreference as Ep;

        for platform in [
            Platform::Windows,
            Platform::MacosAppleSilicon,
            Platform::MacosIntel,
            Platform::Linux,
        ] {
            assert!(Ep::Cpu.is_available_for(platform));
            assert!(!Ep::Xnnpack.is_available_for(platform));
        }
        assert!(Ep::Cpu.is_available_for(Platform::Other));
        assert!(Ep::CoreMl.is_available_for(Platform::MacosAppleSilicon));
        assert!(Ep::CoreMl.is_available_for(Platform::MacosIntel));

        assert_eq!(Ep::default_for(Platform::MacosAppleSilicon), Ep::WebGpu);
        assert_eq!(Ep::default_for(Platform::Windows), Ep::WebGpu);
        assert_eq!(Ep::default_for(Platform::MacosIntel), Ep::CoreMl);
        for platform in [Platform::Linux, Platform::Other] {
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
    fn windows_auto_selection_attempts_webgpu_before_fallback() {
        use ExecutionProviderPlatform::Windows;
        use ExecutionProviderPreference as Ep;

        let hardware_available = ExecutionProviderCapabilities {
            coreml: false,
            xnnpack: false,
        };
        let hardware_unavailable = ExecutionProviderCapabilities {
            coreml: false,
            xnnpack: false,
        };
        assert_eq!(
            Ep::default_for_capabilities(Windows, hardware_available),
            Ep::WebGpu
        );
        assert_eq!(
            Ep::default_for_capabilities(Windows, hardware_unavailable),
            Ep::WebGpu
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
            remote_cache_bytes_limit: None,
            pending_mirror_restore: false,
            pending_mirror_restore_active_library_id: None,
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
        assert!(!providers.contains(&"xnnpack"));

        #[cfg(target_os = "windows")]
        assert!(providers.contains(&"webgpu"));
    }

    #[test]
    fn effective_execution_provider_defaults_to_platform_default() {
        let config = AppConfig::default();

        // Host-conditional expectations mirror the runtime artifact matrix.
        #[cfg(target_os = "windows")]
        assert_eq!(
            config.effective_execution_provider(),
            ExecutionProviderPreference::default_for_current_platform()
        );

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        assert_eq!(
            config.effective_execution_provider(),
            ExecutionProviderPreference::WebGpu
        );

        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        assert_eq!(
            config.effective_execution_provider(),
            ExecutionProviderPreference::CoreMl
        );

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
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
                ExecutionProviderPreference::WebGpu,
            ]
        );
        assert_eq!(
            ExecutionProviderPreference::available_for(MacosIntel),
            &[
                ExecutionProviderPreference::Cpu,
                ExecutionProviderPreference::CoreMl
            ]
        );
        assert_eq!(
            ExecutionProviderPreference::available_for(Linux),
            &[ExecutionProviderPreference::Cpu]
        );
        assert_eq!(
            ExecutionProviderPreference::available_for(Other),
            &[ExecutionProviderPreference::Cpu]
        );
        assert_eq!(
            ExecutionProviderPreference::available_for(Windows),
            &[
                ExecutionProviderPreference::Cpu,
                ExecutionProviderPreference::WebGpu,
            ]
        );
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
            coreml: true,
            xnnpack: true,
        };
        assert_eq!(
            Ep::default_for_capabilities(MacosAppleSilicon, available),
            Ep::WebGpu
        );
        assert!(Ep::CoreMl.is_compatible_for(MacosAppleSilicon, available));
        assert!(Ep::CoreMl.is_compatible_for(MacosIntel, available));
        assert!(Ep::Xnnpack.is_compatible_for(Linux, available));
        assert!(!Ep::Xnnpack.is_compatible_for(Windows, available));

        let unavailable = ExecutionProviderCapabilities {
            coreml: false,
            xnnpack: false,
        };
        assert_eq!(
            Ep::default_for_capabilities(MacosAppleSilicon, unavailable),
            Ep::WebGpu
        );
        assert!(!Ep::CoreMl.is_compatible_for(MacosAppleSilicon, unavailable));
        assert!(!Ep::Xnnpack.is_compatible_for(Linux, unavailable));
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
            execution_provider: Some(ExecutionProviderPreference::WebGpu),
            ..AppConfig::default()
        };
        assert_eq!(
            config.effective_execution_provider_for(MacosAppleSilicon),
            ExecutionProviderPreference::WebGpu
        );
        assert_eq!(
            config.effective_execution_provider_for(MacosIntel),
            ExecutionProviderPreference::CoreMl
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
            ExecutionProviderPreference::WebGpu
        );
    }

    #[test]
    fn effective_execution_provider_for_falls_back_when_unset() {
        use ExecutionProviderPlatform::*;
        let config = AppConfig {
            execution_provider: None,
            ..AppConfig::default()
        };
        assert_eq!(
            config.effective_execution_provider_for(MacosAppleSilicon),
            ExecutionProviderPreference::WebGpu
        );
        assert_eq!(
            config.effective_execution_provider_for(Windows),
            ExecutionProviderPreference::default_for(Windows)
        );
        assert_eq!(
            config.effective_execution_provider_for(MacosIntel),
            ExecutionProviderPreference::CoreMl
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
}
