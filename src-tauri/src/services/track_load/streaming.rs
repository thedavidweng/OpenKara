//! Streaming runtime that follows a successful load: fetch-event handling,
//! mid-song reconnect, and the full-file fallback for sources that turn out
//! not to support range requests.

use super::{
    reconnect::{
        reconnect_production, EventSink, ReconnectConfig, ReconnectError, ReconnectEvent,
        RemoteStreamingRuntime, ReresolvedSource, SeekOutcome,
    },
    request::StalenessGuard,
    source, LoadContext,
};
use crate::{
    audio::{
        coordinator::PlaybackCommand,
        error::PlaybackError,
        playback::{
            PlaybackController, PLAYBACK_ERROR_EVENT, REMOTE_PLAYBACK_FAILED_EVENT,
            REMOTE_PLAYBACK_RECONNECT_EVENT, REMOTE_PLAYBACK_RESYNC_EVENT,
        },
        remote_source::FetchEvent,
        streaming::StreamingTrack,
    },
    cache,
    commands::error::{CommandError, ErrorCode, FallbackAction},
    remote::cache_catalog::CachePinGuard,
    services::playback::PlaybackErrorEvent,
};
use std::{ops::ControlFlow, sync::mpsc, sync::Arc, time::Duration};
use tauri::{AppHandle, Emitter, Runtime};

pub(super) const INITIAL_FETCH: &str = "remote fetch";
const RECONNECTED_FETCH: &str = "remote fetch (reconnect)";

/// Drains the fetch-event channel for the lifetime of one streaming source
/// and owns its cache pin. `label` distinguishes the source installed by the
/// initial load from one installed by a reconnect in the logs.
///
/// The listener stops as soon as its source stops being the installed one —
/// a successful reconnect hands the new receiver to a fresh listener, the
/// full-file fallback replaces streaming entirely, and a superseded request
/// never reinstalls this source. Exiting drops the cache pin and the
/// receiver, so exactly one listener is live per installed source.
pub(super) fn spawn_fetch_event_listener<R: Runtime>(
    ctx: LoadContext<R>,
    cache_pin_guard: Option<CachePinGuard>,
    fetch_event_rx: mpsc::Receiver<FetchEvent>,
    label: &'static str,
) {
    std::thread::spawn(move || {
        let _pin = cache_pin_guard;
        let song_id = ctx.song_id.clone();
        for event in fetch_event_rx {
            let flow = match event {
                FetchEvent::ConsecutiveFailures { count } => {
                    eprintln!("{label}: {count} consecutive failures for {song_id}");
                    attempt_reconnect(&ctx, ReconnectError::Transient)
                }
                FetchEvent::RangeNotSupported => {
                    eprintln!(
                        "{label}: Range requests not supported for {song_id}, falling back to full-file playback"
                    );
                    if let Err(error) = fallback_to_full_file(&ctx) {
                        eprintln!("{label} fallback failed for {song_id}: {error:#}");
                        ctx.fail(error);
                    }
                    ControlFlow::Break(())
                }
                FetchEvent::UrlExpired => {
                    eprintln!(
                        "{label}: download URL expired for {song_id}, attempting reconnect with credential refresh"
                    );
                    attempt_reconnect(&ctx, ReconnectError::CredentialExpired)
                }
            };
            if flow.is_break() {
                break;
            }
        }
    });
}

/// Fully decodes from the provider-backed cached full-file path (or an
/// equivalent non-range route) and installs it.
fn fallback_to_full_file<R: Runtime>(ctx: &LoadContext<R>) -> Result<(), PlaybackError> {
    let connection = ctx.open_database()?;
    let song = cache::get_song_by_hash(&connection, &ctx.song_id)
        .map_err(|e| PlaybackError::Internal(e.to_string()))?
        .ok_or_else(|| PlaybackError::SongNotFound(ctx.song_id.clone()))?;
    super::install_decoded(ctx, &connection, &song)
}

/// Mid-song reconnect after a transient or URL-expired fetch failure.
/// Re-resolve plus an atomic source swap; `refresh_credentials` is a no-op
/// (ProviderFetcher single-flights 401 refresh; re-resolve refreshes provider).
///
/// Returns `Break` when the calling listener's source is out of service — the
/// swap installed a new source (whose listener was spawned here) or the
/// request is stale — and `Continue` when the old source stays installed.
fn attempt_reconnect<R: Runtime>(ctx: &LoadContext<R>, failure: ReconnectError) -> ControlFlow<()> {
    let song_id = ctx.song_id.clone();
    eprintln!("remote playback reconnect triggered for {song_id} (cause: {failure:?})");

    let position_ms = {
        let Ok(playback) = ctx.state.playback.playback.lock() else {
            return ControlFlow::Continue(());
        };
        playback_position_for_reconnect(&playback, &song_id)
    };
    let Some(position_ms) = position_ms else {
        // No matching active track. A current request may still be installing
        // this source; a superseded one never will, so its listener stops.
        return if ctx.guard().is_current() {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        };
    };

    let Ok(cache_catalog) = ctx.state.remote.remote_chunk_cache() else {
        return ControlFlow::Continue(());
    };
    let cache = Arc::clone(cache_catalog);

    let sink = IpcReconnectSink {
        app_handle: ctx.app_handle.clone(),
    };
    let config = ReconnectConfig::default();
    let guard = ctx.guard();

    let library_root = ctx.library_root.clone();
    let app_data_dir = ctx.app_data_dir.clone();
    let resolve_song_id = song_id.clone();
    let re_resolve = move || -> Result<ReresolvedSource<StreamingTrack>, ReconnectError> {
        let connection = cache::open_database(&library_root.database_path())
            .map_err(|_| ReconnectError::Permanent)?;
        let song = cache::get_song_by_hash(&connection, &resolve_song_id)
            .map_err(|_| ReconnectError::NotFound)?
            .ok_or(ReconnectError::NotFound)?;
        // Ok(None) = Range unsupported → permanent fallback.
        let source =
            source::load_remote_streaming_source(Some(&app_data_dir), &cache, &library_root, &song)
                .map_err(ReconnectError::from_playback_error)?
                .ok_or(ReconnectError::Permanent)?;

        Ok(ReresolvedSource {
            source: source.streaming_track,
            from_cache: false,
            runtime: RemoteStreamingRuntime {
                cache_pin_guard: source.cache_pin_guard,
                fetch_event_rx: source.fetch_event_rx,
            },
        })
    };
    // Exact seek is applied by the coordinator swap.
    let seek_source = |source: &mut StreamingTrack, pos_ms: u64| {
        let _ = source;
        SeekOutcome {
            requested_ms: pos_ms,
            actual_ms: pos_ms,
        }
    };
    let refresh_credentials = || true;

    let result = reconnect_production(
        &song_id,
        ctx.request.id(),
        position_ms,
        &config,
        None,
        re_resolve,
        seek_source,
        refresh_credentials,
        guard.predicate(),
        &sink,
    );

    match result {
        Ok(success) => {
            let RemoteStreamingRuntime {
                cache_pin_guard,
                fetch_event_rx,
            } = success.runtime;
            match fetch_event_rx {
                Some(rx) => {
                    spawn_fetch_event_listener(ctx.clone(), cache_pin_guard, rx, RECONNECTED_FETCH)
                }
                None => {
                    if let Some(pin) = cache_pin_guard {
                        spawn_cache_pin_hold(pin, guard);
                    }
                }
            }

            let _ = ctx.send(PlaybackCommand::ReplaceStreamingSource {
                request_id: ctx.request.id(),
                song_id,
                position_ms,
                new_source: Box::new(success.source),
            });
            ControlFlow::Break(())
        }
        Err(ReconnectError::Stale) => ControlFlow::Break(()),
        Err(error) => {
            eprintln!("remote playback reconnect failed for {song_id}: {error:?}");
            let _ = ctx.app_handle.emit(
                PLAYBACK_ERROR_EVENT,
                PlaybackErrorEvent {
                    song_id,
                    error: CommandError::new(
                        ErrorCode::NetworkUnavailable,
                        format!("remote playback reconnect failed: {error:?}"),
                        true,
                        FallbackAction::Retry,
                    ),
                },
            );
            ControlFlow::Continue(())
        }
    }
}

/// Cache fast path: the reconnected source has no fetch thread, so nothing
/// else keeps the entry pinned for the rest of the track.
fn spawn_cache_pin_hold(pin: CachePinGuard, guard: StalenessGuard) {
    std::thread::spawn(move || {
        let _pin = pin;
        while guard.is_current() {
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

/// Position for reconnect when the active track still matches `song_id`.
fn playback_position_for_reconnect(playback: &PlaybackController, song_id: &str) -> Option<u64> {
    let track = playback.current_track_ref()?;
    if track.song_id != song_id {
        return None;
    }
    Some(track.position_ms_for_reconnect())
}

impl ReconnectError {
    fn from_playback_error(error: PlaybackError) -> Self {
        match error {
            PlaybackError::SongNotFound(_) => ReconnectError::NotFound,
            // Decode/internal on re-resolve may be transient (partial fetch).
            _ => ReconnectError::Transient,
        }
    }
}

/// Forwards reconnect events to the frontend IPC surface.
struct IpcReconnectSink<R: Runtime> {
    app_handle: AppHandle<R>,
}

impl<R: Runtime> EventSink for IpcReconnectSink<R> {
    fn emit(&self, event: ReconnectEvent) {
        match event {
            ReconnectEvent::Reconnecting {
                song_id,
                request_id,
                attempt,
                max_attempts,
                reason,
            } => {
                let _ = self.app_handle.emit(
                    REMOTE_PLAYBACK_RECONNECT_EVENT,
                    crate::audio::playback::RemotePlaybackReconnectEvent {
                        song_id,
                        request_id,
                        attempt,
                        max_attempts,
                        reason,
                    },
                );
            }
            ReconnectEvent::Resync {
                song_id,
                requested_position_ms,
                actual_position_ms,
            } => {
                let _ = self.app_handle.emit(
                    REMOTE_PLAYBACK_RESYNC_EVENT,
                    crate::audio::playback::RemotePlaybackResyncEvent {
                        song_id,
                        requested_position_ms,
                        actual_position_ms,
                    },
                );
            }
            ReconnectEvent::Failed {
                song_id,
                request_id,
                reason,
            } => {
                let _ = self.app_handle.emit(
                    REMOTE_PLAYBACK_FAILED_EVENT,
                    crate::audio::playback::RemotePlaybackFailedEvent {
                        song_id,
                        request_id,
                        reason,
                    },
                );
            }
        }
    }
}
