use crate::{
    audio::{
        decode,
        playback::{
            monotonic_now_ms, playback_position_event, PlaybackController, PlaybackStateSnapshot,
            PLAYBACK_POSITION_EVENT,
        },
    },
    cache, AppState,
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
) -> Result<PlaybackStateSnapshot, String> {
    let connection =
        cache::open_database(&state.database_path).map_err(|error| error.to_string())?;
    let mut playback = state
        .playback
        .lock()
        .map_err(|_| "playback controller lock was poisoned".to_string())?;
    let snapshot = play_song_from_library(&connection, &mut playback, &song_id, monotonic_now_ms())
        .map_err(|error| error.to_string())?;

    emit_playback_position(&app_handle, &snapshot).map_err(|error| error.to_string())?;

    Ok(snapshot)
}

#[tauri::command]
pub fn pause(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<PlaybackStateSnapshot, String> {
    let mut playback = state
        .playback
        .lock()
        .map_err(|_| "playback controller lock was poisoned".to_string())?;
    let snapshot = playback
        .pause(monotonic_now_ms())
        .map_err(|error| error.to_string())?;

    emit_playback_position(&app_handle, &snapshot).map_err(|error| error.to_string())?;

    Ok(snapshot)
}

#[tauri::command]
pub fn seek(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    ms: u64,
) -> Result<PlaybackStateSnapshot, String> {
    let mut playback = state
        .playback
        .lock()
        .map_err(|_| "playback controller lock was poisoned".to_string())?;
    let snapshot = playback
        .seek(ms, monotonic_now_ms())
        .map_err(|error| error.to_string())?;

    emit_playback_position(&app_handle, &snapshot).map_err(|error| error.to_string())?;

    Ok(snapshot)
}

#[tauri::command]
pub fn set_volume(state: State<'_, AppState>, level: f32) -> Result<PlaybackStateSnapshot, String> {
    let mut playback = state
        .playback
        .lock()
        .map_err(|_| "playback controller lock was poisoned".to_string())?;

    playback
        .set_volume(level)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_playback_state(state: State<'_, AppState>) -> Result<PlaybackStateSnapshot, String> {
    let mut playback = state
        .playback
        .lock()
        .map_err(|_| "playback controller lock was poisoned".to_string())?;

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
