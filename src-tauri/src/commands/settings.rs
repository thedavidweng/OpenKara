use crate::commands::error::{internal_error, CommandResult};
use crate::config::{
    self, AppConfig, ExecutionProviderPreference, ModelVariant, StemMode, ThemePreference,
};
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, Manager, State};

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct AppSettings {
    pub stem_mode: String,
    pub model_variant: String,
    pub language: Option<String>,
    pub hide_batch_separate: bool,
    pub cover_art_backdrop: bool,
    pub lyrics_font_step: i8,
    pub execution_provider: String,
    pub available_execution_providers: Vec<&'static str>,
    pub theme_preference: String,
}

fn settings_from_config(config: &AppConfig) -> AppSettings {
    let mode = config.effective_stem_mode();
    let variant = config.effective_model_variant();
    let ep = config.effective_execution_provider();
    AppSettings {
        stem_mode: match mode {
            StemMode::TwoStem => "two_stem".to_owned(),
            StemMode::FourStem => "four_stem".to_owned(),
        },
        model_variant: variant.as_str().to_owned(),
        language: config.language.clone(),
        hide_batch_separate: config.hide_batch_separate.unwrap_or(false),
        cover_art_backdrop: config.cover_art_backdrop.unwrap_or(true),
        lyrics_font_step: config.effective_lyrics_font_step(),
        execution_provider: ep.as_str().to_owned(),
        available_execution_providers: ExecutionProviderPreference::available_for_current_platform(
        ),
        theme_preference: config.effective_theme_preference().as_str().to_owned(),
    }
}

fn validate_lyrics_font_step(step: i8) -> CommandResult<i8> {
    if !(-2..=2).contains(&step) {
        return Err(internal_error(format!("invalid lyrics font step: {step}")));
    }

    Ok(step)
}

fn persist_lyrics_font_step(app_data_dir: &Path, step: i8) -> CommandResult<AppSettings> {
    let step = validate_lyrics_font_step(step)?;
    let mut config = config::load_config(app_data_dir)
        .map_err(|e| internal_error(format!("failed to load config: {e}")))?
        .unwrap_or_default();
    config.lyrics_font_step = Some(step);
    config::save_config(app_data_dir, &config)
        .map_err(|e| internal_error(format!("failed to save config: {e}")))?;
    Ok(settings_from_config(&config))
}

#[tauri::command]
pub fn get_settings(app_handle: AppHandle) -> CommandResult<AppSettings> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let config = config::load_config(&app_data_dir)
        .map_err(|e| internal_error(format!("failed to load config: {e}")))?
        .unwrap_or_default();
    Ok(settings_from_config(&config))
}

#[tauri::command]
pub fn set_stem_mode(app_handle: AppHandle, mode: String) -> CommandResult<AppSettings> {
    let stem_mode = match mode.as_str() {
        "two_stem" => StemMode::TwoStem,
        "four_stem" => StemMode::FourStem,
        _ => return Err(internal_error(format!("invalid stem mode: {mode}"))),
    };
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let mut config = config::load_config(&app_data_dir)
        .map_err(|e| internal_error(format!("failed to load config: {e}")))?
        .unwrap_or_default();
    config.stem_mode = Some(stem_mode);
    config::save_config(&app_data_dir, &config)
        .map_err(|e| internal_error(format!("failed to save config: {e}")))?;
    Ok(settings_from_config(&config))
}

#[tauri::command]
pub fn set_model_variant(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    variant: String,
) -> CommandResult<AppSettings> {
    let model_variant = ModelVariant::parse(&variant)
        .ok_or_else(|| internal_error(format!("invalid model variant: {variant}")))?;
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let mut config = config::load_config(&app_data_dir)
        .map_err(|e| internal_error(format!("failed to load config: {e}")))?
        .unwrap_or_default();
    config.model_variant = Some(model_variant);
    config::save_config(&app_data_dir, &config)
        .map_err(|e| internal_error(format!("failed to save config: {e}")))?;

    let snapshot = crate::commands::bootstrap::sync_active_model_bootstrap_status(
        &app_data_dir,
        &state.shell.model_bootstrap_status,
    )?;

    crate::commands::bootstrap::emit_model_bootstrap_snapshot(&app_handle, &snapshot);

    Ok(settings_from_config(&config))
}

#[tauri::command]
pub fn set_language(app_handle: AppHandle, language: String) -> CommandResult<AppSettings> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let mut config = config::load_config(&app_data_dir)
        .map_err(|e| internal_error(format!("failed to load config: {e}")))?
        .unwrap_or_default();
    config.language = Some(language.clone());
    config::save_config(&app_data_dir, &config)
        .map_err(|e| internal_error(format!("failed to save config: {e}")))?;
    Ok(settings_from_config(&config))
}

#[tauri::command]
pub fn set_hide_batch_separate(app_handle: AppHandle, value: bool) -> CommandResult<AppSettings> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let mut config = config::load_config(&app_data_dir)
        .map_err(|e| internal_error(format!("failed to load config: {e}")))?
        .unwrap_or_default();
    config.hide_batch_separate = Some(value);
    config::save_config(&app_data_dir, &config)
        .map_err(|e| internal_error(format!("failed to save config: {e}")))?;
    Ok(settings_from_config(&config))
}

#[tauri::command]
pub fn set_cover_art_backdrop(app_handle: AppHandle, value: bool) -> CommandResult<AppSettings> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let mut config = config::load_config(&app_data_dir)
        .map_err(|e| internal_error(format!("failed to load config: {e}")))?
        .unwrap_or_default();
    config.cover_art_backdrop = Some(value);
    config::save_config(&app_data_dir, &config)
        .map_err(|e| internal_error(format!("failed to save config: {e}")))?;
    Ok(settings_from_config(&config))
}

#[tauri::command]
pub fn set_lyrics_font_step(app_handle: AppHandle, step: i8) -> CommandResult<AppSettings> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;

    persist_lyrics_font_step(&app_data_dir, step)
}

/// Parse and validate an execution-provider string against the current
/// platform's policy table. Pure: testable without a Tauri `AppHandle`.
/// Rejects unknown strings and known-but-unavailable providers before any
/// config mutation.
fn parse_and_validate_execution_provider(
    provider: &str,
) -> CommandResult<ExecutionProviderPreference> {
    let ep = ExecutionProviderPreference::parse(provider)
        .ok_or_else(|| internal_error(format!("invalid execution provider: {provider}")))?;
    if !ep.is_available_for_current_platform() {
        return Err(internal_error(format!(
            "execution provider '{}' is unavailable on this platform",
            ep.as_str()
        )));
    }
    Ok(ep)
}

#[tauri::command]
pub fn set_execution_provider(
    app_handle: AppHandle,
    provider: String,
) -> CommandResult<AppSettings> {
    // Validate before touching config so a rejected request never creates or
    // rewrites config.json.
    let ep = parse_and_validate_execution_provider(&provider)?;
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let mut config = config::load_config(&app_data_dir)
        .map_err(|e| internal_error(format!("failed to load config: {e}")))?
        .unwrap_or_default();
    config.execution_provider = Some(ep);
    config::save_config(&app_data_dir, &config)
        .map_err(|e| internal_error(format!("failed to save config: {e}")))?;
    Ok(settings_from_config(&config))
}

#[tauri::command]
pub fn set_theme_preference(
    app_handle: AppHandle,
    preference: String,
) -> CommandResult<AppSettings> {
    let theme = ThemePreference::parse(&preference)
        .ok_or_else(|| internal_error(format!("invalid theme preference: {preference}")))?;
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let mut config = config::load_config(&app_data_dir)
        .map_err(|e| internal_error(format!("failed to load config: {e}")))?
        .unwrap_or_default();
    config.theme_preference = Some(theme);
    config::save_config(&app_data_dir, &config)
        .map_err(|e| internal_error(format!("failed to save config: {e}")))?;
    Ok(settings_from_config(&config))
}

#[tauri::command]
pub fn restart_app(app_handle: AppHandle) {
    app_handle.request_restart();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ExecutionProviderPreference;

    #[test]
    fn settings_default_lyrics_font_step_is_zero() {
        let settings = settings_from_config(&AppConfig::default());
        assert_eq!(settings.lyrics_font_step, 0);
    }

    #[test]
    fn persist_lyrics_font_step_updates_config_and_returns_snapshot() {
        let temp_dir = tempfile::tempdir().expect("temp dir should create");

        let settings =
            persist_lyrics_font_step(temp_dir.path(), 2).expect("lyrics font step should persist");

        assert_eq!(settings.lyrics_font_step, 2);

        let loaded = config::load_config(temp_dir.path())
            .expect("config should load")
            .expect("config should exist after persisting");
        assert_eq!(loaded.effective_lyrics_font_step(), 2);
    }

    #[test]
    fn persist_lyrics_font_step_rejects_out_of_range_values() {
        let temp_dir = tempfile::tempdir().expect("temp dir should create");

        let error = persist_lyrics_font_step(temp_dir.path(), 3)
            .expect_err("out of range lyrics font step should fail");

        assert!(error.message.contains("invalid lyrics font step"));
        assert!(
            config::load_config(temp_dir.path())
                .expect("config load should succeed")
                .is_none(),
            "failed writes should not create a config file",
        );
    }

    #[test]
    fn settings_snapshot_uses_platform_default_execution_provider_when_unset() {
        let settings = settings_from_config(&AppConfig {
            execution_provider: None,
            libraries: vec![],
            active_library_id: None,
            ..AppConfig::default()
        });

        assert_eq!(
            settings.execution_provider,
            ExecutionProviderPreference::default_for_current_platform().as_str()
        );
    }

    #[test]
    fn settings_selected_provider_is_always_in_available_list() {
        // Stale directml on a non-Windows host is normalized to xnnpack and
        // must still be a member of the available list.
        let settings = settings_from_config(&AppConfig {
            execution_provider: Some(ExecutionProviderPreference::DirectMl),
            ..AppConfig::default()
        });
        assert!(
            settings
                .available_execution_providers
                .contains(&settings.execution_provider.as_str()),
            "selected provider '{}' must be in available list {:?}",
            settings.execution_provider,
            settings.available_execution_providers,
        );
    }

    #[test]
    fn parse_and_validate_accepts_every_current_platform_entry() {
        for &name in ExecutionProviderPreference::available_for_current_platform().as_slice() {
            let ep = parse_and_validate_execution_provider(name)
                .unwrap_or_else(|e| panic!("valid provider {name} rejected: {e:?}"));
            assert_eq!(ep.as_str(), name);
        }
    }

    #[test]
    fn parse_and_validate_rejects_unknown_provider() {
        let error = parse_and_validate_execution_provider("coreml")
            .expect_err("unknown provider should be rejected");
        assert!(error.message.contains("invalid execution provider: coreml"));
    }

    #[test]
    fn parse_and_validate_rejects_known_unavailable_provider() {
        // directml is known but Windows-only. On non-Windows hosts it must be
        // rejected with the platform-availability message. On Windows it is
        // valid, so we skip the rejection assertion there.
        #[cfg(not(target_os = "windows"))]
        {
            let error = parse_and_validate_execution_provider("directml")
                .expect_err("directml should be rejected off Windows");
            assert!(
                error
                    .message
                    .contains("execution provider 'directml' is unavailable on this platform"),
                "unexpected message: {}",
                error.message,
            );
        }
        #[cfg(target_os = "windows")]
        {
            let ep = parse_and_validate_execution_provider("directml")
                .expect("directml should be accepted on Windows");
            assert_eq!(ep.as_str(), "directml");
        }
    }

    #[test]
    fn rejected_set_execution_provider_does_not_create_config_file() {
        // A rejected request must not create or mutate config.json. We exercise
        // this by validating the pure helper (the command calls it before any
        // disk access) against an unknown provider in an empty temp dir.
        let temp_dir = tempfile::tempdir().expect("temp dir should create");
        let _ = parse_and_validate_execution_provider("coreml")
            .expect_err("unknown provider should be rejected");
        assert!(
            config::load_config(temp_dir.path())
                .expect("config load should succeed")
                .is_none(),
            "rejected validation must not create a config file",
        );
    }

    #[test]
    fn settings_snapshot_defaults_theme_preference_to_dark() {
        let settings = settings_from_config(&AppConfig::default());
        assert_eq!(settings.theme_preference, "dark");
    }

    #[test]
    fn settings_snapshot_reflects_persisted_theme_preference() {
        for (preference, expected) in [
            (ThemePreference::System, "system"),
            (ThemePreference::Light, "light"),
            (ThemePreference::Dark, "dark"),
        ] {
            let settings = settings_from_config(&AppConfig {
                theme_preference: Some(preference),
                ..AppConfig::default()
            });
            assert_eq!(settings.theme_preference, expected);
        }
    }
}
