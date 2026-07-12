use crate::audio::coordinator::PlaybackCommand;
use crate::audio::eq::{EQ_MAX_GAIN_DB, EQ_MIN_GAIN_DB};
use crate::audio::playback::EqState;
use crate::commands::error::{internal_error, CommandResult};
use crate::config::{self, AppConfig, ExecutionProviderPreference, ModelVariant, StemMode};
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
    pub eq_enabled: bool,
    pub eq_gains_db: [f32; 5],
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
        eq_enabled: config.effective_eq_enabled(),
        eq_gains_db: config.effective_eq_gains_db(),
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

#[tauri::command]
pub fn set_execution_provider(
    app_handle: AppHandle,
    provider: String,
) -> CommandResult<AppSettings> {
    let ep = ExecutionProviderPreference::parse(&provider)
        .ok_or_else(|| internal_error(format!("invalid execution provider: {provider}")))?;
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

/// Validate an EQ gains array: every value must be finite and within
/// [-12, 12]. Rejects the whole request instead of clamping, per the issue
/// spec.
fn validate_eq_gains(gains_db: &[f32; 5]) -> CommandResult<()> {
    for (i, &g) in gains_db.iter().enumerate() {
        if !g.is_finite() {
            return Err(internal_error(format!(
                "invalid eq gain at index {i}: not finite ({g})"
            )));
        }
        if !(EQ_MIN_GAIN_DB..=EQ_MAX_GAIN_DB).contains(&g) {
            return Err(internal_error(format!(
                "invalid eq gain at index {i}: {g} dB out of range [{EQ_MIN_GAIN_DB}, {EQ_MAX_GAIN_DB}]"
            )));
        }
    }
    Ok(())
}

/// Send an EQ coordinator command and await its `EqState` reply.
async fn send_eq_command(
    state: &AppState,
    make_command: impl FnOnce(
        tokio::sync::oneshot::Sender<Result<EqState, crate::audio::error::PlaybackError>>,
    ) -> PlaybackCommand,
) -> CommandResult<EqState> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let command = make_command(tx);
    state
        .playback
        .command_tx
        .send(command)
        .map_err(|_| internal_error("playback coordinator disconnected"))?;
    rx.await
        .map_err(|_| internal_error("playback coordinator dropped reply"))?
        .map_err(Into::into)
}

/// Read the current runtime EQ state (enabled + gains) for rollback purposes.
fn current_runtime_eq_state(state: &AppState) -> EqState {
    let Ok(playback) = state.playback.playback.lock() else {
        return EqState {
            enabled: false,
            gains_db: [0.0; 5],
        };
    };
    playback.eq_state()
}

/// Best-effort rollback of the coordinator EQ state after a persistence
/// failure. Logs but does not propagate the rollback error — the caller
/// already has a persistence error to return.
async fn rollback_eq_state(state: &AppState, previous: EqState) {
    if let Err(e) = send_eq_command(state, |reply| PlaybackCommand::SetEqEnabled {
        enabled: previous.enabled,
        reply,
    })
    .await
    {
        eprintln!("settings: failed to rollback eq_enabled: {}", e.message);
    }
    if let Err(e) = send_eq_command(state, |reply| PlaybackCommand::SetEqGains {
        gains_db: previous.gains_db,
        reply,
    })
    .await
    {
        eprintln!("settings: failed to rollback eq_gains: {}", e.message);
    }
}

#[tauri::command]
pub async fn set_eq_enabled(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<AppSettings> {
    // 2. Read previous runtime values for rollback.
    let previous = current_runtime_eq_state(state.inner());

    // 3. Send coordinator update and await acknowledgement.
    send_eq_command(state.inner(), |reply| PlaybackCommand::SetEqEnabled {
        enabled,
        reply,
    })
    .await?;

    // 4. Persist config.
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let mut config = config::load_config(&app_data_dir)
        .map_err(|e| internal_error(format!("failed to load config: {e}")))?
        .unwrap_or_default();
    config.eq_enabled = Some(enabled);
    if let Err(e) = config::save_config(&app_data_dir, &config) {
        // 5. Persistence failed — best-effort rollback.
        rollback_eq_state(state.inner(), previous).await;
        return Err(internal_error(format!("failed to save config: {e}")));
    }

    // 6. Return settings_from_config.
    Ok(settings_from_config(&config))
}

#[tauri::command]
pub async fn set_eq_gains(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    gains_db: [f32; 5],
) -> CommandResult<AppSettings> {
    // 1. Validate input — reject the whole request instead of clamping.
    validate_eq_gains(&gains_db)?;

    // 2. Read previous runtime values for rollback.
    let previous = current_runtime_eq_state(state.inner());

    // 3. Send coordinator update and await acknowledgement.
    send_eq_command(state.inner(), |reply| PlaybackCommand::SetEqGains {
        gains_db,
        reply,
    })
    .await?;

    // 4. Persist config.
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let mut config = config::load_config(&app_data_dir)
        .map_err(|e| internal_error(format!("failed to load config: {e}")))?
        .unwrap_or_default();
    config.eq_gains_db = Some(gains_db);
    if let Err(e) = config::save_config(&app_data_dir, &config) {
        // 5. Persistence failed — best-effort rollback.
        rollback_eq_state(state.inner(), previous).await;
        return Err(internal_error(format!("failed to save config: {e}")));
    }

    // 6. Return settings_from_config.
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
    fn settings_snapshot_includes_eq_defaults() {
        let settings = settings_from_config(&AppConfig::default());
        assert!(!settings.eq_enabled);
        assert_eq!(settings.eq_gains_db, [0.0; 5]);
    }

    #[test]
    fn settings_snapshot_includes_eq_from_config() {
        let settings = settings_from_config(&AppConfig {
            eq_enabled: Some(true),
            eq_gains_db: Some([3.0, -6.0, 0.0, 9.0, -12.0]),
            ..AppConfig::default()
        });
        assert!(settings.eq_enabled);
        assert_eq!(settings.eq_gains_db, [3.0, -6.0, 0.0, 9.0, -12.0]);
    }

    #[test]
    fn validate_eq_gains_accepts_in_range_values() {
        let gains = [-12.0, -6.0, 0.0, 6.0, 12.0];
        assert!(validate_eq_gains(&gains).is_ok());
    }

    #[test]
    fn validate_eq_gains_rejects_out_of_range_values() {
        let gains = [12.5, 0.0, 0.0, 0.0, 0.0];
        let error = validate_eq_gains(&gains).expect_err("out of range gain should fail");
        assert!(error.message.contains("out of range"));
    }

    #[test]
    fn validate_eq_gains_rejects_non_finite_values() {
        let gains = [f32::NAN, 0.0, 0.0, 0.0, 0.0];
        let error = validate_eq_gains(&gains).expect_err("non-finite gain should fail");
        assert!(error.message.contains("not finite"));
    }

    #[test]
    fn validate_eq_gains_rejects_infinity() {
        let gains = [f32::INFINITY, 0.0, 0.0, 0.0, 0.0];
        let error = validate_eq_gains(&gains).expect_err("infinity gain should fail");
        assert!(error.message.contains("not finite"));
    }
}
