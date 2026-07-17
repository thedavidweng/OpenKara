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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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

/// #88: Preload the next queue head for gapless playback. The frontend calls
/// this whenever the queue head changes (song added, reordered, skipped, or
/// the current song changes). Passing `null` cancels any pending preload.
///
/// The command returns immediately; decoding happens on a background thread.
/// If the candidate is not eligible for gapless (remote, Media+G, or format
/// mismatch), the preload is silently skipped and the frontend will fall
/// back to calling `play()` when `track-transitioned` does not arrive.
#[tauri::command]
pub async fn set_preload_candidate(
    state: State<'_, AppState>,
    song_id: Option<String>,
) -> CommandResult<()> {
    let inner = state.inner().clone();
    let app_data_dir = inner.shell.app_data_dir.clone();

    // Bump the preload request generation. This generation is captured by
    // the new preload thread and included in the `PrepareNext` command. The
    // coordinator stamps it onto `expected_preload_request_generation` via
    // `CancelPreparedNext`, so any `PrepareNext` from an older preload thread
    // (which passed its shutdown check before the flag was set but sends
    // after the cancel) is rejected as stale.
    let preload_generation = inner
        .playback
        .preload_request_generation
        .fetch_add(1, Ordering::SeqCst)
        + 1;

    // Cancel any existing preload by signalling the old preload shutdown flag
    // and sending CancelPreparedNext to the coordinator. This uses a separate
    // flag from `background_shutdown` (used by `play()`) so that cancelling a
    // preload does not kill an in-flight play() background decode thread.
    //
    // The shutdown flag replacement and the CancelPreparedNext send are both
    // performed while holding the `preload_shutdown` lock so that two
    // concurrent calls serialize atomically. If the cancel send were deferred
    // until after the lock is released, rapid successive preload requests
    // could have their CancelPreparedNext commands arrive at the coordinator
    // out of order, silently dropping the gapless candidate.
    let shutdown = {
        let mut guard = inner
            .playback
            .preload_shutdown
            .lock()
            .map_err(|_| internal_error("preload_shutdown lock was poisoned"))?;
        guard.store(true, Ordering::Relaxed);
        let new_shutdown = Arc::new(AtomicBool::new(false));
        *guard = new_shutdown.clone();

        // Send CancelPreparedNext to clear any installed prepared track and
        // stamp the new expected generation onto the controller. Done under
        // the lock so the flag swap and cancel are atomic w.r.t. other calls.
        let _ = inner
            .playback
            .command_tx
            .send(PlaybackCommand::CancelPreparedNext {
                expected_generation: preload_generation,
            });

        new_shutdown
    };

    let Some(song_id) = song_id else {
        return Ok(());
    };

    // spawn_preload_next spawns its own std::thread and returns immediately,
    // so we can call it directly without spawn_blocking.
    services::next_track::spawn_preload_next(
        inner,
        app_data_dir,
        song_id,
        shutdown,
        preload_generation,
    );

    Ok(())
}
