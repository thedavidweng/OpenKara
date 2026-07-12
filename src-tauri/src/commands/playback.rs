use crate::{
    audio::{
        coordinator::PlaybackCommand,
        peaks::AudioPeakSnapshot,
        playback::{PlaybackStateSnapshot, StemName},
    },
    commands::error::{internal_error, CommandResult},
    services,
    state::AppState,
};
use tauri::{AppHandle, State};

pub use crate::services::playback::play_song_from_library;

/// Send a synchronous command to the coordinator and await its reply.
/// Maps channel and reply errors to `CommandError`.
async fn send_and_await(
    state: &AppState,
    make_command: impl FnOnce(
        tokio::sync::oneshot::Sender<
            Result<PlaybackStateSnapshot, crate::audio::error::PlaybackError>,
        >,
    ) -> PlaybackCommand,
) -> CommandResult<PlaybackStateSnapshot> {
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
pub async fn resume(
    state: State<'_, AppState>,
    _app_handle: AppHandle,
) -> CommandResult<PlaybackStateSnapshot> {
    send_and_await(state.inner(), |reply| PlaybackCommand::Resume { reply }).await
}

#[tauri::command]
pub async fn pause(
    state: State<'_, AppState>,
    _app_handle: AppHandle,
) -> CommandResult<PlaybackStateSnapshot> {
    send_and_await(state.inner(), |reply| PlaybackCommand::Pause { reply }).await
}

#[tauri::command]
pub async fn seek(
    state: State<'_, AppState>,
    _app_handle: AppHandle,
    ms: u64,
) -> CommandResult<PlaybackStateSnapshot> {
    send_and_await(state.inner(), |reply| PlaybackCommand::Seek {
        target_ms: ms,
        reply,
    })
    .await
}

#[tauri::command]
pub async fn set_volume(
    state: State<'_, AppState>,
    level: f32,
) -> CommandResult<PlaybackStateSnapshot> {
    send_and_await(state.inner(), |reply| PlaybackCommand::SetVolume {
        level,
        reply,
    })
    .await
}

#[tauri::command]
pub async fn set_stem_volume(
    state: State<'_, AppState>,
    stem: StemName,
    level: f32,
) -> CommandResult<PlaybackStateSnapshot> {
    send_and_await(state.inner(), |reply| PlaybackCommand::SetStemVolume {
        stem,
        level,
        reply,
    })
    .await
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

/// Read-only command: copy the current peak ring snapshot without taking the
/// playback mutex. The ring is a lossy observability channel; playback must
/// never wait for a reader.
#[tauri::command]
pub fn get_audio_peaks(state: State<'_, AppState>) -> AudioPeakSnapshot {
    let (write_index, peaks) = state.playback.peak_ring.snapshot();
    AudioPeakSnapshot { write_index, peaks }
}
