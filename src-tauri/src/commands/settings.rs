use crate::audio::coordinator::PlaybackCommand;
use crate::audio::eq::validate_gains_db;
use crate::audio::playback::CrossfadeState;
use crate::commands::error::{internal_error, invalid_playback_state, CommandResult};
use crate::config::{self, AppConfig, ExecutionProviderPreference, ModelVariant, StemMode};
use crate::AppState;
use serde::Serialize;
use std::path::Path;
use std::sync::mpsc;
use tauri::{AppHandle, Manager, State};

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
    pub crossfade_enabled: bool,
    pub crossfade_duration_ms: u32,
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
        crossfade_enabled: config.effective_crossfade_enabled(),
        crossfade_duration_ms: config.effective_crossfade_duration_ms(),
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

/// Apply an EQ-enabled change through the coordinator first, then persist.
/// If the coordinator fails, the config is not touched. If persistence fails
/// after a successful coordinator apply, the coordinator is reverted to the
/// old value so the running engine and stored config stay consistent.
///
/// Returns the updated config on success. Testable without a Tauri
/// `AppHandle` by passing a temp dir and a command sender directly.
async fn apply_eq_enabled_atomically(
    app_data_dir: &Path,
    command_tx: &mpsc::Sender<PlaybackCommand>,
    enabled: bool,
) -> CommandResult<AppConfig> {
    let mut config = config::load_config(app_data_dir)
        .map_err(|e| internal_error(format!("failed to load config: {e}")))?
        .unwrap_or_default();

    let old_enabled = config.effective_eq_enabled();

    // Apply through the coordinator FIRST so the running audio engine and
    // persisted config never diverge. If the coordinator rejects the update
    // or the channel is disconnected, we return an error without touching
    // disk — the stored config and the engine both remain at the old value.
    let (tx, rx) = tokio::sync::oneshot::channel();
    let command = PlaybackCommand::SetEqEnabled { enabled, reply: tx };
    command_tx
        .send(command)
        .map_err(|_| internal_error("playback coordinator disconnected"))?;
    rx.await
        .map_err(|_| internal_error("playback coordinator dropped reply"))?
        .map_err(|e| internal_error(format!("failed to apply eq enabled: {e}")))?;

    // Coordinator succeeded — persist the new value. If persistence fails,
    // revert the coordinator to the old value so the engine and config stay
    // consistent.
    config.eq_enabled = Some(enabled);
    if let Err(e) = config::save_config(app_data_dir, &config) {
        let (revert_tx, revert_rx) = tokio::sync::oneshot::channel();
        let revert_command = PlaybackCommand::SetEqEnabled {
            enabled: old_enabled,
            reply: revert_tx,
        };
        let _ = command_tx.send(revert_command);
        let _ = revert_rx.await;
        return Err(internal_error(format!("failed to save config: {e}")));
    }

    Ok(config)
}

/// Apply an EQ-gains change through the coordinator first, then persist.
/// Same failure-atomic contract as `apply_eq_enabled_atomically`.
async fn apply_eq_gains_atomically(
    app_data_dir: &Path,
    command_tx: &mpsc::Sender<PlaybackCommand>,
    gains_db: [f32; 5],
) -> CommandResult<AppConfig> {
    validate_gains_db(&gains_db)
        .map_err(|e| invalid_playback_state(format!("invalid eq gains: {e}")))?;

    let mut config = config::load_config(app_data_dir)
        .map_err(|e| internal_error(format!("failed to load config: {e}")))?
        .unwrap_or_default();

    let old_gains = config.effective_eq_gains_db();

    let (tx, rx) = tokio::sync::oneshot::channel();
    let command = PlaybackCommand::SetEqGains {
        gains_db,
        reply: tx,
    };
    command_tx
        .send(command)
        .map_err(|_| internal_error("playback coordinator disconnected"))?;
    rx.await
        .map_err(|_| internal_error("playback coordinator dropped reply"))?
        .map_err(|e| internal_error(format!("failed to apply eq gains: {e}")))?;

    config.eq_gains_db = Some(gains_db);
    if let Err(e) = config::save_config(app_data_dir, &config) {
        let (revert_tx, revert_rx) = tokio::sync::oneshot::channel();
        let revert_command = PlaybackCommand::SetEqGains {
            gains_db: old_gains,
            reply: revert_tx,
        };
        let _ = command_tx.send(revert_command);
        let _ = revert_rx.await;
        return Err(internal_error(format!("failed to save config: {e}")));
    }

    Ok(config)
}

#[tauri::command]
pub async fn set_eq_enabled(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<AppSettings> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let config =
        apply_eq_enabled_atomically(&app_data_dir, &state.playback.command_tx, enabled).await?;
    Ok(settings_from_config(&config))
}

#[tauri::command]
pub async fn set_eq_gains(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    gains_db: [f32; 5],
) -> CommandResult<AppSettings> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let config =
        apply_eq_gains_atomically(&app_data_dir, &state.playback.command_tx, gains_db).await?;
    Ok(settings_from_config(&config))
}

// ── #89: Crossfade settings commands ─────────────────────────────────────

/// Accepted crossfade duration range in milliseconds.
const CROSSFADE_MIN_MS: u32 = 500;
const CROSSFADE_MAX_MS: u32 = 10_000;

fn validate_crossfade_duration(duration_ms: u32) -> CommandResult<()> {
    if !(CROSSFADE_MIN_MS..=CROSSFADE_MAX_MS).contains(&duration_ms) {
        return Err(internal_error(format!(
            "invalid crossfade duration: {duration_ms} ms out of range [{CROSSFADE_MIN_MS}, {CROSSFADE_MAX_MS}]"
        )));
    }
    Ok(())
}

/// Send a crossfade coordinator command and await its reply.
async fn send_crossfade_command(
    state: &AppState,
    make_command: impl FnOnce(
        tokio::sync::oneshot::Sender<Result<CrossfadeState, crate::audio::error::PlaybackError>>,
    ) -> PlaybackCommand,
) -> CommandResult<CrossfadeState> {
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

fn current_runtime_crossfade_state(state: &AppState) -> CrossfadeState {
    let Ok(playback) = state.playback.playback.lock() else {
        return CrossfadeState {
            enabled: false,
            duration_ms: 3_000,
        };
    };
    playback.crossfade_state()
}

async fn rollback_crossfade_state(state: &AppState, previous: CrossfadeState) {
    if let Err(e) = send_crossfade_command(state, |reply| PlaybackCommand::SetCrossfadeEnabled {
        enabled: previous.enabled,
        reply,
    })
    .await
    {
        eprintln!(
            "settings: failed to rollback crossfade_enabled: {}",
            e.message
        );
    }
    if let Err(e) = send_crossfade_command(state, |reply| PlaybackCommand::SetCrossfadeDuration {
        duration_ms: previous.duration_ms,
        reply,
    })
    .await
    {
        eprintln!(
            "settings: failed to rollback crossfade_duration: {}",
            e.message
        );
    }
}

#[tauri::command]
pub async fn set_crossfade_enabled(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<AppSettings> {
    let previous = current_runtime_crossfade_state(state.inner());

    send_crossfade_command(state.inner(), |reply| {
        PlaybackCommand::SetCrossfadeEnabled { enabled, reply }
    })
    .await?;

    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let mut config = config::load_config(&app_data_dir)
        .map_err(|e| internal_error(format!("failed to load config: {e}")))?
        .unwrap_or_default();
    config.crossfade_enabled = Some(enabled);
    if let Err(e) = config::save_config(&app_data_dir, &config) {
        rollback_crossfade_state(state.inner(), previous).await;
        return Err(internal_error(format!("failed to save config: {e}")));
    }

    Ok(settings_from_config(&config))
}

#[tauri::command]
pub async fn set_crossfade_duration_ms(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    duration_ms: u32,
) -> CommandResult<AppSettings> {
    validate_crossfade_duration(duration_ms)?;

    let previous = current_runtime_crossfade_state(state.inner());

    send_crossfade_command(state.inner(), |reply| {
        PlaybackCommand::SetCrossfadeDuration { duration_ms, reply }
    })
    .await?;

    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| internal_error(format!("failed to get app data dir: {e}")))?;
    let mut config = config::load_config(&app_data_dir)
        .map_err(|e| internal_error(format!("failed to load config: {e}")))?
        .unwrap_or_default();
    config.crossfade_duration_ms = Some(duration_ms);
    if let Err(e) = config::save_config(&app_data_dir, &config) {
        rollback_crossfade_state(state.inner(), previous).await;
        return Err(internal_error(format!("failed to save config: {e}")));
    }

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

    // ── Failure-atomic EQ settings tests ───────────────────────────────
    //
    // These tests verify the coordinator-first / persist-second contract:
    // if the coordinator fails, the config is not touched; if persistence
    // fails after a successful coordinator apply, the coordinator is
    // reverted to the old value.

    /// Helper: drain the command channel and respond to SetEqEnabled with
    /// the given result. Returns the `enabled` value from the command.
    fn respond_to_set_eq_enabled(
        rx: &std::sync::mpsc::Receiver<PlaybackCommand>,
        result: Result<(), crate::audio::error::PlaybackError>,
    ) -> bool {
        match rx.recv().expect("command should arrive") {
            PlaybackCommand::SetEqEnabled { enabled, reply } => {
                if let Err(e) = result {
                    let _ = reply.send(Err(e));
                } else {
                    let _ = reply.send(Ok(crate::audio::playback::PlaybackStateSnapshot::idle()));
                }
                enabled
            }
            _ => panic!("expected SetEqEnabled command"),
        }
    }

    /// Helper: drain the command channel and respond to SetEqGains with
    /// the given result. Returns the gains from the command.
    fn respond_to_set_eq_gains(
        rx: &std::sync::mpsc::Receiver<PlaybackCommand>,
        result: Result<(), crate::audio::error::PlaybackError>,
    ) -> [f32; 5] {
        match rx.recv().expect("command should arrive") {
            PlaybackCommand::SetEqGains { gains_db, reply } => {
                if let Err(e) = result {
                    let _ = reply.send(Err(e));
                } else {
                    let _ = reply.send(Ok(crate::audio::playback::PlaybackStateSnapshot::idle()));
                }
                gains_db
            }
            _ => panic!("expected SetEqGains command"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn eq_enabled_coordinator_send_failure_does_not_persist() {
        let temp_dir = tempfile::tempdir().expect("temp dir should create");
        // Drop the receiver immediately so send fails.
        let (tx, _) = mpsc::channel::<PlaybackCommand>();

        let error = apply_eq_enabled_atomically(temp_dir.path(), &tx, true)
            .await
            .expect_err("disconnected coordinator should fail");

        assert!(error.message.contains("playback coordinator disconnected"));
        assert!(
            config::load_config(temp_dir.path())
                .expect("config load should succeed")
                .is_none(),
            "config must not be created when coordinator send fails",
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn eq_enabled_coordinator_reply_dropped_does_not_persist() {
        let temp_dir = tempfile::tempdir().expect("temp dir should create");
        let (tx, rx) = mpsc::channel::<PlaybackCommand>();

        // Spawn a task that receives the command but drops the reply sender
        // without responding, simulating a coordinator that crashes mid-apply.
        let handle = tokio::spawn(async move {
            match rx.recv() {
                Ok(PlaybackCommand::SetEqEnabled {
                    enabled: _,
                    reply: _,
                }) => {
                    // Drop reply without sending — simulates coordinator crash.
                }
                _ => panic!("expected SetEqEnabled"),
            }
        });

        let error = apply_eq_enabled_atomically(temp_dir.path(), &tx, true)
            .await
            .expect_err("dropped reply should fail");

        assert!(error.message.contains("playback coordinator dropped reply"));
        assert!(
            config::load_config(temp_dir.path())
                .expect("config load should succeed")
                .is_none(),
            "config must not be created when coordinator reply is dropped",
        );

        let _ = handle.await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn eq_enabled_coordinator_apply_failure_does_not_persist() {
        let temp_dir = tempfile::tempdir().expect("temp dir should create");
        let (tx, rx) = mpsc::channel::<PlaybackCommand>();

        // Spawn a background task that responds with an error.
        let handle = tokio::spawn(async move {
            respond_to_set_eq_enabled(
                &rx,
                Err(crate::audio::error::PlaybackError::Internal(
                    "coordinator apply failed".to_owned(),
                )),
            );
        });

        let error = apply_eq_enabled_atomically(temp_dir.path(), &tx, true)
            .await
            .expect_err("coordinator apply failure should propagate");

        assert!(error.message.contains("failed to apply eq enabled"));
        assert!(
            config::load_config(temp_dir.path())
                .expect("config load should succeed")
                .is_none(),
            "config must not be created when coordinator apply fails",
        );

        let _ = handle.await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn eq_enabled_success_persists_and_returns_config() {
        let temp_dir = tempfile::tempdir().expect("temp dir should create");
        let (tx, rx) = mpsc::channel::<PlaybackCommand>();

        let handle = tokio::spawn(async move {
            let enabled = respond_to_set_eq_enabled(&rx, Ok(()));
            assert!(enabled);
        });

        let config = apply_eq_enabled_atomically(temp_dir.path(), &tx, true)
            .await
            .expect("success should return config");

        assert!(config.effective_eq_enabled());

        let loaded = config::load_config(temp_dir.path())
            .expect("config should load")
            .expect("config should exist");
        assert!(loaded.effective_eq_enabled());

        let _ = handle.await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn eq_gains_coordinator_send_failure_does_not_persist() {
        let temp_dir = tempfile::tempdir().expect("temp dir should create");
        let (tx, _) = mpsc::channel::<PlaybackCommand>();

        let error = apply_eq_gains_atomically(temp_dir.path(), &tx, [3.0, 0.0, 0.0, 0.0, 0.0])
            .await
            .expect_err("disconnected coordinator should fail");

        assert!(error.message.contains("playback coordinator disconnected"));
        assert!(config::load_config(temp_dir.path())
            .expect("config load should succeed")
            .is_none(),);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn eq_gains_coordinator_apply_failure_does_not_persist() {
        let temp_dir = tempfile::tempdir().expect("temp dir should create");
        let (tx, rx) = mpsc::channel::<PlaybackCommand>();

        let handle = tokio::spawn(async move {
            respond_to_set_eq_gains(
                &rx,
                Err(crate::audio::error::PlaybackError::Internal(
                    "coordinator apply failed".to_owned(),
                )),
            );
        });

        let error = apply_eq_gains_atomically(temp_dir.path(), &tx, [3.0, 0.0, 0.0, 0.0, 0.0])
            .await
            .expect_err("coordinator apply failure should propagate");

        assert!(error.message.contains("failed to apply eq gains"));
        assert!(config::load_config(temp_dir.path())
            .expect("config load should succeed")
            .is_none(),);

        let _ = handle.await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn eq_gains_invalid_values_fail_before_coordinator_or_persist() {
        let temp_dir = tempfile::tempdir().expect("temp dir should create");
        let (tx, rx) = mpsc::channel::<PlaybackCommand>();

        // NaN gains should be rejected by validate_gains_db before any
        // coordinator command is sent or config is touched.
        let error = apply_eq_gains_atomically(temp_dir.path(), &tx, [f32::NAN, 0.0, 0.0, 0.0, 0.0])
            .await
            .expect_err("invalid gains should fail");

        assert!(error.message.contains("invalid eq gains"));
        assert!(
            rx.try_recv().is_err(),
            "no coordinator command should be sent for invalid gains",
        );
        assert!(config::load_config(temp_dir.path())
            .expect("config load should succeed")
            .is_none(),);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn eq_gains_success_persists_and_returns_config() {
        let temp_dir = tempfile::tempdir().expect("temp dir should create");
        let (tx, rx) = mpsc::channel::<PlaybackCommand>();

        let handle = tokio::spawn(async move {
            let gains = respond_to_set_eq_gains(&rx, Ok(()));
            assert_eq!(gains, [3.0, -6.0, 0.0, 12.0, -12.0]);
        });

        let config = apply_eq_gains_atomically(temp_dir.path(), &tx, [3.0, -6.0, 0.0, 12.0, -12.0])
            .await
            .expect("success should return config");

        assert_eq!(
            config.effective_eq_gains_db(),
            [3.0, -6.0, 0.0, 12.0, -12.0]
        );

        let loaded = config::load_config(temp_dir.path())
            .expect("config should load")
            .expect("config should exist");
        assert_eq!(
            loaded.effective_eq_gains_db(),
            [3.0, -6.0, 0.0, 12.0, -12.0]
        );

        let _ = handle.await;
    }

    /// Persistence failure: coordinator succeeds, but save_config fails.
    /// The coordinator should be reverted to the old value, and the error
    /// should propagate. We simulate this by making the config file
    /// read-only so save_config's fs::write fails.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn eq_enabled_persistence_failure_reverts_coordinator() {
        use std::os::unix::fs::PermissionsExt;
        let temp_dir = tempfile::tempdir().expect("temp dir should create");

        // First, write a valid config with eq_enabled = false so we have
        // an old value to revert to.
        let initial_config = AppConfig {
            eq_enabled: Some(false),
            ..AppConfig::default()
        };
        config::save_config(temp_dir.path(), &initial_config).expect("initial config should save");

        let (tx, rx) = mpsc::channel::<PlaybackCommand>();

        // Make the config file read-only so save_config's fs::write fails.
        // On Unix, writing an existing file requires the file's write bit,
        // not the directory's.
        let config_path = temp_dir.path().join("config.json");
        let mut perms = std::fs::metadata(&config_path)
            .expect("config metadata")
            .permissions();
        perms.set_mode(0o444);
        std::fs::set_permissions(&config_path, perms).expect("should set config read-only");

        // The coordinator should receive the forward command (enabled=true)
        // and then a revert command (enabled=false).
        let handle = tokio::spawn(async move {
            // Forward command: respond Ok.
            let forward_enabled = respond_to_set_eq_enabled(&rx, Ok(()));
            assert!(forward_enabled, "forward command should set enabled=true");

            // Revert command: respond Ok.
            let revert_enabled = respond_to_set_eq_enabled(&rx, Ok(()));
            assert!(
                !revert_enabled,
                "revert command should set enabled=false (old value)",
            );
        });

        let error = apply_eq_enabled_atomically(temp_dir.path(), &tx, true)
            .await
            .expect_err("persistence failure should propagate");

        assert!(error.message.contains("failed to save config"));

        // The stored config should still have the old value.
        let loaded = config::load_config(temp_dir.path())
            .expect("config should load")
            .expect("config should exist");
        assert!(
            !loaded.effective_eq_enabled(),
            "stored config should remain at old value after persistence failure",
        );

        let _ = handle.await;

        // Restore permissions so temp_dir cleanup works.
        let mut perms = std::fs::metadata(&config_path)
            .expect("metadata")
            .permissions();
        perms.set_mode(0o644);
        let _ = std::fs::set_permissions(&config_path, perms);
    }

    /// Same persistence-failure revert test for eq_gains.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn eq_gains_persistence_failure_reverts_coordinator() {
        use std::os::unix::fs::PermissionsExt;
        let temp_dir = tempfile::tempdir().expect("temp dir should create");

        let initial_config = AppConfig {
            eq_gains_db: Some([0.0; 5]),
            ..AppConfig::default()
        };
        config::save_config(temp_dir.path(), &initial_config).expect("initial config should save");

        let (tx, rx) = mpsc::channel::<PlaybackCommand>();

        let config_path = temp_dir.path().join("config.json");
        let mut perms = std::fs::metadata(&config_path)
            .expect("config metadata")
            .permissions();
        perms.set_mode(0o444);
        std::fs::set_permissions(&config_path, perms).expect("should set config read-only");

        let handle = tokio::spawn(async move {
            let forward_gains = respond_to_set_eq_gains(&rx, Ok(()));
            assert_eq!(forward_gains, [3.0, 0.0, 0.0, 0.0, 0.0]);

            let revert_gains = respond_to_set_eq_gains(&rx, Ok(()));
            assert_eq!(
                revert_gains, [0.0; 5],
                "revert command should restore old gains",
            );
        });

        let error = apply_eq_gains_atomically(temp_dir.path(), &tx, [3.0, 0.0, 0.0, 0.0, 0.0])
            .await
            .expect_err("persistence failure should propagate");

        assert!(error.message.contains("failed to save config"));

        let loaded = config::load_config(temp_dir.path())
            .expect("config should load")
            .expect("config should exist");
        assert_eq!(
            loaded.effective_eq_gains_db(),
            [0.0; 5],
            "stored config should remain at old gains after persistence failure",
        );

        let _ = handle.await;

        let mut perms = std::fs::metadata(&config_path)
            .expect("metadata")
            .permissions();
        perms.set_mode(0o644);
        let _ = std::fs::set_permissions(&config_path, perms);
    }
}
