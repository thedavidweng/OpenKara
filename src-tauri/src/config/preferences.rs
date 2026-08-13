use serde::{Deserialize, Serialize};

use super::AppConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StemMode {
    TwoStem,
    #[default]
    FourStem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySortMode {
    #[default]
    RecentlyImported,
    TitleAsc,
    ArtistAsc,
}

impl LibrarySortMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RecentlyImported => "recently_imported",
            Self::TitleAsc => "title_asc",
            Self::ArtistAsc => "artist_asc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePolicy {
    Manual,
    #[default]
    Notify,
    AutoDownload,
}

impl UpdatePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Notify => "notify",
            Self::AutoDownload => "auto_download",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "manual" => Some(Self::Manual),
            "notify" => Some(Self::Notify),
            "auto_download" => Some(Self::AutoDownload),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    System,
    Light,
    #[default]
    Dark,
}

impl ThemePreference {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "system" => Some(Self::System),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelVariant {
    #[default]
    Htdemucs,
    HtdemucsFt,
}

impl ModelVariant {
    pub fn as_str(self) -> &'static str {
        match self {
            ModelVariant::Htdemucs => "htdemucs",
            ModelVariant::HtdemucsFt => "htdemucs_ft",
        }
    }

    pub fn parse(s: &str) -> Option<ModelVariant> {
        match s {
            "htdemucs" => Some(ModelVariant::Htdemucs),
            "htdemucs_ft" => Some(ModelVariant::HtdemucsFt),
            _ => None,
        }
    }
}

impl AppConfig {
    pub fn effective_stem_mode(&self) -> StemMode {
        self.stem_mode.unwrap_or_default()
    }

    pub fn effective_model_variant(&self) -> ModelVariant {
        self.model_variant.unwrap_or_default()
    }

    pub fn effective_lyrics_font_step(&self) -> i8 {
        self.lyrics_font_step.unwrap_or(0)
    }

    pub fn effective_eq_enabled(&self) -> bool {
        self.eq_enabled.unwrap_or(false)
    }

    pub fn effective_eq_gains_db(&self) -> [f32; 5] {
        let mut gains = self.eq_gains_db.unwrap_or([0.0; 5]);
        for g in gains.iter_mut() {
            if !g.is_finite() {
                *g = 0.0;
            }
            *g = g.clamp(-12.0, 12.0);
        }
        gains
    }

    pub fn effective_library_sort_mode(&self) -> LibrarySortMode {
        self.library_sort_mode.unwrap_or_default()
    }

    pub fn effective_crossfade_enabled(&self) -> bool {
        self.crossfade_enabled.unwrap_or(false)
    }

    pub fn effective_crossfade_duration_ms(&self) -> u32 {
        self.crossfade_duration_ms
            .unwrap_or(3_000)
            .clamp(500, 10_000)
    }

    pub fn effective_theme_preference(&self) -> ThemePreference {
        self.theme_preference.unwrap_or_default()
    }

    pub fn effective_update_policy(&self) -> UpdatePolicy {
        self.update_policy.unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{load_config, save_config};

    #[test]
    fn effective_stem_mode_defaults_to_four_stem() {
        let config = AppConfig::default();
        assert_eq!(config.effective_stem_mode(), StemMode::FourStem);
    }

    #[test]
    fn stem_mode_none_is_omitted_from_json() {
        let config = AppConfig {
            library_path: None,
            libraries: vec![],
            active_library_id: None,
            stem_mode: None,
            language: None,
            hide_batch_separate: None,
            cover_art_backdrop: None,
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
            remote_cache_bytes_limit: None,
            pending_mirror_restore: false,
            pending_mirror_restore_active_library_id: None,
            directml_disabled_by_runtime_timeout: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("stem_mode"));
    }

    #[test]
    fn effective_lyrics_font_step_defaults_to_zero() {
        let config = AppConfig::default();
        assert_eq!(config.effective_lyrics_font_step(), 0);
    }

    #[test]
    fn lyrics_font_step_none_is_omitted_from_json() {
        let config = AppConfig {
            library_path: None,
            libraries: vec![],
            active_library_id: None,
            stem_mode: None,
            language: None,
            hide_batch_separate: None,
            cover_art_backdrop: None,
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
            remote_cache_bytes_limit: None,
            pending_mirror_restore: false,
            pending_mirror_restore_active_library_id: None,
            directml_disabled_by_runtime_timeout: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("lyrics_font_step"));
    }

    #[test]
    fn library_sort_mode_defaults_to_recently_imported() {
        assert_eq!(
            AppConfig::default().effective_library_sort_mode(),
            LibrarySortMode::RecentlyImported
        );
    }

    #[test]
    fn library_sort_mode_none_is_omitted_from_json() {
        let config = AppConfig {
            library_sort_mode: None,
            theme_preference: None,
            update_policy: None,
            ..AppConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("library_sort_mode"));
    }

    #[test]
    fn library_sort_mode_round_trips_through_json() {
        for mode in [
            LibrarySortMode::RecentlyImported,
            LibrarySortMode::TitleAsc,
            LibrarySortMode::ArtistAsc,
        ] {
            let config = AppConfig {
                library_sort_mode: Some(mode),
                theme_preference: None,
                update_policy: None,
                ..AppConfig::default()
            };
            let json = serde_json::to_string(&config).unwrap();
            let loaded: AppConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(loaded.library_sort_mode, Some(mode));
        }
    }

    #[test]
    fn effective_eq_defaults_to_disabled_flat() {
        let config = AppConfig::default();
        assert!(!config.effective_eq_enabled());
        assert_eq!(config.effective_eq_gains_db(), [0.0; 5]);
    }

    #[test]
    fn effective_eq_enabled_hydrates_from_persisted_value() {
        let config = AppConfig {
            eq_enabled: Some(true),
            ..Default::default()
        };
        assert!(config.effective_eq_enabled());
    }

    #[test]
    fn effective_eq_gains_hydrates_from_persisted_values() {
        let config = AppConfig {
            eq_gains_db: Some([3.0, -6.0, 0.0, 12.0, -12.0]),
            ..Default::default()
        };
        assert_eq!(
            config.effective_eq_gains_db(),
            [3.0, -6.0, 0.0, 12.0, -12.0]
        );
    }

    #[test]
    fn effective_eq_gains_clamps_out_of_range_persisted_values() {
        let config = AppConfig {
            eq_gains_db: Some([20.0, -20.0, 0.0, 100.0, -100.0]),
            ..Default::default()
        };
        assert_eq!(
            config.effective_eq_gains_db(),
            [12.0, -12.0, 0.0, 12.0, -12.0]
        );
    }

    #[test]
    fn effective_eq_gains_replaces_non_finite_persisted_values_with_zero() {
        let config = AppConfig {
            eq_gains_db: Some([f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, 6.0]),
            ..Default::default()
        };
        let gains = config.effective_eq_gains_db();
        assert_eq!(gains, [0.0, 0.0, 0.0, 0.0, 6.0]);
    }

    #[test]
    fn library_sort_mode_rejects_unknown_snake_case_string() {
        let raw = r#"{ "library_sort_mode": "descending_title" }"#;
        let result: Result<AppConfig, _> = serde_json::from_str(raw);
        assert!(result.is_err());
    }

    #[test]
    fn library_sort_mode_persists_through_save_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let config = AppConfig {
            library_sort_mode: Some(LibrarySortMode::ArtistAsc),
            theme_preference: None,
            update_policy: None,
            ..AppConfig::default()
        };
        save_config(tmp.path(), &config).unwrap();
        let loaded = load_config(tmp.path()).unwrap().unwrap();
        assert_eq!(
            loaded.effective_library_sort_mode(),
            LibrarySortMode::ArtistAsc
        );
    }

    #[test]
    fn effective_theme_preference_defaults_to_dark() {
        let config = AppConfig::default();
        assert_eq!(config.effective_theme_preference(), ThemePreference::Dark);
    }

    #[test]
    fn theme_preference_none_is_omitted_from_json() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("theme_preference"));
    }

    #[test]
    fn theme_preference_round_trips_through_json() {
        for preference in [
            ThemePreference::System,
            ThemePreference::Light,
            ThemePreference::Dark,
        ] {
            let config = AppConfig {
                theme_preference: Some(preference),
                ..AppConfig::default()
            };
            let json = serde_json::to_string(&config).unwrap();
            let loaded: AppConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(loaded.theme_preference, Some(preference));
        }
    }

    #[test]
    fn invalid_theme_preference_string_is_rejected() {
        let json = r#"{"theme_preference": "high_contrast"}"#;
        let result = serde_json::from_str::<AppConfig>(json);
        assert!(result.is_err());
    }

    #[test]
    fn missing_theme_preference_field_defaults_to_dark() {
        let loaded: AppConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(loaded.effective_theme_preference(), ThemePreference::Dark);
    }

    #[test]
    fn effective_crossfade_defaults_to_disabled_3000ms() {
        let config = AppConfig::default();
        assert!(!config.effective_crossfade_enabled());
        assert_eq!(config.effective_crossfade_duration_ms(), 3_000);
    }

    #[test]
    fn effective_crossfade_enabled_hydrates_from_persisted_value() {
        let config = AppConfig {
            crossfade_enabled: Some(true),
            ..Default::default()
        };
        assert!(config.effective_crossfade_enabled());
    }

    #[test]
    fn effective_crossfade_duration_hydrates_from_persisted_value() {
        let config = AppConfig {
            crossfade_duration_ms: Some(5_000),
            ..Default::default()
        };
        assert_eq!(config.effective_crossfade_duration_ms(), 5_000);
    }

    #[test]
    fn effective_crossfade_duration_clamps_below_minimum() {
        let config = AppConfig {
            crossfade_duration_ms: Some(100),
            ..Default::default()
        };
        assert_eq!(config.effective_crossfade_duration_ms(), 500);
    }

    #[test]
    fn effective_crossfade_duration_clamps_above_maximum() {
        let config = AppConfig {
            crossfade_duration_ms: Some(20_000),
            ..Default::default()
        };
        assert_eq!(config.effective_crossfade_duration_ms(), 10_000);
    }
}
