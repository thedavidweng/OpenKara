use crate::{
    audio::{
        decode, output,
        playback::{
            monotonic_now_ms, playback_position_event, PlaybackController, PlaybackMode,
            PlaybackStateSnapshot, PLAYBACK_POSITION_EVENT,
        },
    },
    cache,
    commands::error::{
        database_error, internal_error, playback_error, state_lock_error, CommandResult,
    },
    AppState,
};
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub fn play(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    song_id: String,
) -> CommandResult<PlaybackStateSnapshot> {
    let connection = cache::open_database(&state.database_path).map_err(database_error)?;
    let mut playback = state
        .playback
        .lock()
        .map_err(|_| state_lock_error("playback controller lock was poisoned"))?;
    let snapshot = play_song_from_library(&connection, &mut playback, &song_id, monotonic_now_ms())
        .map_err(playback_error)?;
    drop(playback);

    output::ensure_output_thread(
        &state.audio_output_started,
        &state.audio_output_start_lock,
        state.playback.clone(),
    )
    .map_err(playback_error)?;

    emit_playback_position(&app_handle, &snapshot)
        .map_err(|error| internal_error(error.to_string()))?;

    Ok(snapshot)
}

#[tauri::command]
pub fn pause(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> CommandResult<PlaybackStateSnapshot> {
    let mut playback = state
        .playback
        .lock()
        .map_err(|_| state_lock_error("playback controller lock was poisoned"))?;
    let snapshot = playback.pause(monotonic_now_ms()).map_err(playback_error)?;

    emit_playback_position(&app_handle, &snapshot)
        .map_err(|error| internal_error(error.to_string()))?;

    Ok(snapshot)
}

#[tauri::command]
pub fn seek(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    ms: u64,
) -> CommandResult<PlaybackStateSnapshot> {
    let mut playback = state
        .playback
        .lock()
        .map_err(|_| state_lock_error("playback controller lock was poisoned"))?;
    let snapshot = playback
        .seek(ms, monotonic_now_ms())
        .map_err(playback_error)?;

    emit_playback_position(&app_handle, &snapshot)
        .map_err(|error| internal_error(error.to_string()))?;

    Ok(snapshot)
}

#[tauri::command]
pub fn set_volume(state: State<'_, AppState>, level: f32) -> CommandResult<PlaybackStateSnapshot> {
    let mut playback = state
        .playback
        .lock()
        .map_err(|_| state_lock_error("playback controller lock was poisoned"))?;

    playback.set_volume(level).map_err(playback_error)
}

#[tauri::command]
pub fn set_playback_mode(
    state: State<'_, AppState>,
    mode: PlaybackMode,
) -> CommandResult<PlaybackStateSnapshot> {
    let connection = cache::open_database(&state.database_path).map_err(database_error)?;
    let mut playback = state
        .playback
        .lock()
        .map_err(|_| state_lock_error("playback controller lock was poisoned"))?;

    set_playback_mode_from_library(&connection, &mut playback, mode).map_err(playback_error)
}

#[tauri::command]
pub fn get_playback_state(state: State<'_, AppState>) -> CommandResult<PlaybackStateSnapshot> {
    let mut playback = state
        .playback
        .lock()
        .map_err(|_| state_lock_error("playback controller lock was poisoned"))?;

    Ok(playback.snapshot(monotonic_now_ms()))
}

pub fn play_song_from_library(
    connection: &Connection,
    controller: &mut PlaybackController,
    song_id: &str,
    now_ms: u64,
) -> Result<PlaybackStateSnapshot> {
    let song = cache::get_song_by_hash(connection, song_id)
        .context("failed to load song from library")?
        .with_context(|| format!("song with hash {song_id} was not found in the library"))?;
    let decoded_audio = decode::decode_file(Path::new(&song.file_path))
        .with_context(|| format!("failed to decode audio for {}", song.file_path))?;

    Ok(controller.start_track(song.hash, decoded_audio, now_ms))
}

pub fn set_playback_mode_from_library(
    connection: &Connection,
    controller: &mut PlaybackController,
    mode: PlaybackMode,
) -> Result<PlaybackStateSnapshot> {
    if mode == PlaybackMode::Karaoke && !controller.has_karaoke_track() {
        let song_id = controller
            .current_song_id()
            .context("no track is loaded")?
            .to_owned();
        let cached_stems = cache::stems::get_cached_stem_entry(connection, &song_id)
            .context("failed to load cached stems for playback mode switch")?
            .with_context(|| format!("song with hash {song_id} does not have cached stems"))?;
        let accompaniment = decode::decode_file(Path::new(&cached_stems.accomp_path))
            .with_context(|| {
                format!(
                    "failed to decode accompaniment {}",
                    cached_stems.accomp_path
                )
            })?;
        controller.attach_karaoke_track(&song_id, accompaniment)?;
    }

    controller.set_mode(mode)
}

pub fn emit_playback_position(
    app_handle: &AppHandle,
    snapshot: &PlaybackStateSnapshot,
) -> tauri::Result<()> {
    if snapshot.song_id.is_none() {
        return Ok(());
    }

    app_handle.emit(
        PLAYBACK_POSITION_EVENT,
        playback_position_event(snapshot).map_err(|error| tauri::Error::Anyhow(error.into()))?,
    )
}
