use crate::{
    audio::playback::{PlaybackStateSnapshot, StemName},
    commands::error::{internal_error, CommandResult},
    services,
    state::AppState,
};
use tauri::{AppHandle, State};

pub use crate::services::playback::play_song_from_library;

#[tauri::command]
pub async fn play(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_id: String,
) -> CommandResult<PlaybackStateSnapshot> {
    let background_state = state.inner().clone();
    let background_handle = app_handle.clone();
    Ok(tauri::async_runtime::spawn_blocking(move || {
        services::playback::play(&background_state, &background_handle, &song_id)
    })
    .await
    .map_err(|error| internal_error(format!("playback task failed: {error}")))??)
}

#[tauri::command]
pub fn resume(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> CommandResult<PlaybackStateSnapshot> {
    Ok(services::playback::resume(&state, &app_handle)?)
}

#[tauri::command]
pub fn pause(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> CommandResult<PlaybackStateSnapshot> {
    Ok(services::playback::pause(&state, &app_handle)?)
}

#[tauri::command]
pub fn seek(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    ms: u64,
) -> CommandResult<PlaybackStateSnapshot> {
    Ok(services::playback::seek(&state, &app_handle, ms)?)
}

#[tauri::command]
pub fn set_volume(state: State<'_, AppState>, level: f32) -> CommandResult<PlaybackStateSnapshot> {
    Ok(services::playback::set_volume(&state, level)?)
}

#[tauri::command]
pub fn set_stem_volume(
    state: State<'_, AppState>,
    stem: StemName,
    level: f32,
) -> CommandResult<PlaybackStateSnapshot> {
    Ok(services::playback::set_stem_volume(&state, stem, level)?)
}

#[tauri::command]
pub async fn load_stems(state: State<'_, AppState>) -> CommandResult<PlaybackStateSnapshot> {
    let background_state = state.inner().clone();
    Ok(tauri::async_runtime::spawn_blocking(move || {
        services::playback::load_stems(&background_state)
    })
    .await
    .map_err(|error| internal_error(format!("load stems task failed: {error}")))??)
}

#[tauri::command]
pub fn get_playback_state(state: State<'_, AppState>) -> CommandResult<PlaybackStateSnapshot> {
    Ok(services::playback::get_state(&state)?)
}
