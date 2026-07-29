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

pub use crate::services::playback::play_song_from_library;

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

    // Cancel any existing preload by signalling the old preload shutdown flag
    // and sending CancelPreparedNext to the coordinator. This uses a separate
    // flag from `background_shutdown` (used by `play()`) so that cancelling a
    // preload does not kill an in-flight play() background decode thread.
    //
    // The generation assignment, shutdown flag replacement, and the
    // CancelPreparedNext send are all performed while holding the
    // `preload_shutdown` lock so that two concurrent calls serialize
    // atomically. If the generation were assigned before the lock, two
    // concurrent invocations could obtain generations in one order (A=1, B=2)
    // yet acquire the lock in the opposite order (B first), causing the
    // coordinator to end up with an older expected generation than the newest
    // request — the newest preload's PrepareNext would be rejected as stale
    // while an older preload's PrepareNext is accepted. Likewise, if the
    // cancel send were deferred until after the lock is released, rapid
    // successive preload requests could have their CancelPreparedNext commands
    // arrive at the coordinator out of order, silently dropping the gapless
    // candidate.
    let (shutdown, preload_generation) = {
        let mut guard = inner
            .playback
            .preload_shutdown
            .lock()
            .map_err(|_| internal_error("preload_shutdown lock was poisoned"))?;
        // Bump the preload request generation inside the lock so the
        // generation value and the CancelPreparedNext send are ordered
        // consistently for concurrent callers. This generation is captured
        // by the new preload thread and included in the `PrepareNext`
        // command. The coordinator stamps it onto
        // `expected_preload_request_generation` via `CancelPreparedNext`, so
        // any `PrepareNext` from an older preload thread (which passed its
        // shutdown check before the flag was set but sends after the cancel)
        // is rejected as stale.
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

        // Send CancelPreparedNext to clear any installed prepared track and
        // stamp the new expected generation onto the controller. Done under
        // the lock so the flag swap and cancel are atomic w.r.t. other calls.
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

/// #90: Fetch a cached or freshly-computed waveform for a song.
///
/// Returns `Vec<f32>` of length `buckets` (clamped to `24..=1000`) for a
/// local source, or `[]` for a remote source. Every value is finite and in
/// `0.0..=1.0`. Unknown songs produce a structured `song_not_found` error;
/// decode/database failures produce a sanitized internal error with no raw
/// absolute paths.
///
/// The command performs these exact steps:
/// 1. derive effective buckets;
/// 2. clone `LibraryRoot` from `AppState` without retaining its mutex;
/// 3. open the library DB in a short blocking task and fetch the song;
/// 4. return unknown-song error or `[]` for a remote song;
/// 5. call `WaveformSingleflight::register(key, library_root)`;
/// 6. convert the shared slice to `Vec<f32>` only at the IPC boundary.
///
/// The singleflight value lives in `PlaybackState` solely for process-wide
/// sharing; the command never takes the playback-controller lock.
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

    // 4. Remote sources: return [] before path resolution. Do not download,
    //    decode, enter singleflight, or cache the empty result.
    if song.is_remote() {
        return Ok(Vec::new());
    }

    let (rx, inserted) = state.playback.waveform_singleflight.register(key.clone());

    if inserted {
        let library_root = library_root.clone();
        let key_for_task = key.clone();
        let singleflight = state.playback.waveform_singleflight.clone();
        // Spawn the computation task. The task owns completion: it always
        // removes the key and fan-outs the result (or a sanitized error) to
        // all waiters, even on panic/JoinError/cancellation. A task-owned
        // completion guard ensures the key is never permanently stranded:
        // if the task is dropped before `complete()`, the guard's `Drop`
        // removes the key and sends a sanitized error to remaining waiters.
        tauri::async_runtime::spawn(async move {
            // Create the guard before any await so cancellation at any point
            // still cleans up the pending-map entry.
            let mut guard = SingleflightCompletionGuard::new(singleflight, key.clone());
            let result = tauri::async_runtime::spawn_blocking(move || {
                services::waveform::compute_waveform_blocking(library_root, key_for_task)
            })
            .await;
            // Take waiters before sending so no send occurs under the map lock.
            // `complete()` marks the guard as done so its `Drop` is a no-op.
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
