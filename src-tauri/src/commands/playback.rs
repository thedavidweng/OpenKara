use crate::{
    audio::{
        coordinator::PlaybackCommand,
        peaks::AudioPeakSnapshot,
        playback::{PlaybackStateSnapshot, StemName},
    },
    cache::{self, waveforms},
    commands::error::{internal_error, CommandError, CommandResult, ErrorCode, FallbackAction},
    services,
    state::{AppState, SingleflightCompletionGuard, WaveformKey, SANITIZED_WAVEFORM_ERROR},
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, State};

const DEFAULT_WAVEFORM_BUCKETS: usize = 200;

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
        services::track_load::start(&background_state, &background_handle, &song_id)
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
        services::track_load::attach_stems(&background_state)
    })
    .await
    .map_err(|error| internal_error(format!("load stems task failed: {error}")))??)
}

#[tauri::command]
pub fn get_playback_state(state: State<'_, AppState>) -> CommandResult<PlaybackStateSnapshot> {
    Ok(services::playback::get_state(&state)?)
}

/// Peak ring snapshot; does not take the playback mutex.
#[tauri::command]
pub fn get_audio_peaks(state: State<'_, AppState>) -> AudioPeakSnapshot {
    let (write_index, peaks) = state.playback.peak_ring.snapshot();
    AudioPeakSnapshot { write_index, peaks }
}

/// Preload the next queue head for gapless playback. `None` cancels preload.
/// Decode runs on a background thread; ineligible candidates are skipped.
#[tauri::command]
pub async fn set_preload_candidate(
    state: State<'_, AppState>,
    song_id: Option<String>,
) -> CommandResult<()> {
    let inner = state.inner().clone();
    let app_data_dir = inner.shell.app_data_dir.clone();

    // Under preload_shutdown: bump generation, swap flag, and CancelPreparedNext
    // together so concurrent calls cannot invert cancel vs PrepareNext order.
    // Separate from play()'s background_shutdown so cancel does not kill decode.
    let (shutdown, preload_generation) = {
        let mut guard = inner
            .playback
            .preload_shutdown
            .lock()
            .map_err(|_| internal_error("preload_shutdown lock was poisoned"))?;
        let preload_generation = inner
            .playback
            .preload_request_generation
            .fetch_add(1, Ordering::SeqCst)
            + 1;
        let preload_generation =
            crate::audio::playback::PreloadRequestGeneration(preload_generation);
        guard.store(true, Ordering::Relaxed);
        let new_shutdown = Arc::new(AtomicBool::new(false));
        *guard = new_shutdown.clone();

        let _ = inner
            .playback
            .command_tx
            .send(PlaybackCommand::CancelPreparedNext {
                expected_generation: preload_generation,
            });

        (new_shutdown, preload_generation)
    };

    let Some(song_id) = song_id else {
        return Ok(());
    };

    services::next_track::spawn_preload_next(
        inner,
        app_data_dir,
        song_id,
        shutdown,
        preload_generation,
    );

    Ok(())
}

/// Waveform peaks for a song (`buckets` clamped to 24..=1000). Remote → `[]`.
/// Never takes the playback-controller lock; uses process-wide singleflight.
#[tauri::command]
pub async fn get_waveform(
    state: State<'_, AppState>,
    hash: String,
    buckets: Option<usize>,
) -> CommandResult<Vec<f32>> {
    let requested = buckets.unwrap_or(DEFAULT_WAVEFORM_BUCKETS);
    let effective = waveforms::clamp_buckets(requested);
    let key = WaveformKey {
        song_hash: hash.clone(),
        buckets: effective,
    };

    let library_root = state.library_root()?;

    let song_lookup = {
        let library_root = library_root.clone();
        let hash = hash.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let connection = cache::open_database(&library_root.database_path())
                .map_err(|e| internal_error(format!("failed to open library database: {e}")))?;
            let song = cache::get_song_by_hash(&connection, &hash)
                .map_err(|e| internal_error(format!("failed to read song: {e}")))?;
            Ok::<_, CommandError>(song)
        })
        .await
        .map_err(|e| internal_error(format!("song lookup task failed: {e}")))?
    };

    let song = match song_lookup {
        Ok(Some(song)) => song,
        Ok(None) => {
            return Err(CommandError::new(
                ErrorCode::SongNotFound,
                format!("song {hash} not found"),
                false,
                FallbackAction::RefreshLibrary,
            ));
        }
        Err(err) => return Err(err),
    };

    if song.is_remote() {
        return Ok(Vec::new());
    }

    let (rx, inserted) = state.playback.waveform_singleflight.register(key.clone());

    if inserted {
        let library_root = library_root.clone();
        let key_for_task = key.clone();
        let singleflight = state.playback.waveform_singleflight.clone();
        // Task owns singleflight completion (Drop cleans stranded keys).
        tauri::async_runtime::spawn(async move {
            // Guard before any await so cancel still removes the pending entry.
            let mut guard = SingleflightCompletionGuard::new(singleflight, key.clone());
            let result = tauri::async_runtime::spawn_blocking(move || {
                services::waveform::compute_waveform_blocking(library_root, key_for_task)
            })
            .await;
            let Some(waiters) = guard.complete() else {
                return;
            };
            let payload = match result {
                Ok(Ok(peaks)) => Ok(peaks),
                Ok(Err(_)) | Err(_) => Err(SANITIZED_WAVEFORM_ERROR.to_owned()),
            };
            for waiter in waiters {
                let _ = waiter.send(payload.clone());
            }
        });
    }

    let shared = rx
        .await
        .map_err(|_| internal_error("waveform computation was cancelled"))?
        .map_err(|_| internal_error(SANITIZED_WAVEFORM_ERROR))?;
    Ok(shared.to_vec())
}
