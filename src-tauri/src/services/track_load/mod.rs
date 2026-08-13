//! Track-load request lifecycle.
//!
//! The only producer of the coordinator's load commands (`BeginLoad`,
//! `InstallReady`, `FailLoad`, `AttachStems`, `ReplaceStreamingSource`).
//! Callers ask for a song to start playing or for stems to be attached to the
//! track that is playing; request allocation, staleness adjudication, source
//! resolution, streaming runtime and mid-song reconnect all live behind that.

pub(crate) mod reconnect;
mod request;
pub(crate) mod source;
mod streaming;

#[cfg(test)]
mod tests;

use crate::{
    audio::{
        coordinator::{PlaybackCommand, ReadyTrack},
        error::PlaybackError,
        playback::PlaybackStateSnapshot,
    },
    cache,
    commands::cdg::CdgErrorCode,
    library::Song,
    library_root::LibraryRoot,
    services::cdg::{load_cdg_packets_for_song, CdgLoadResult},
    state::AppState,
};
use request::{PlaybackRequest, StalenessGuard};
use rusqlite::Connection;
use source::{PlaybackSourceLoad, RemoteContent, StreamingPlaybackSource};
use std::{path::PathBuf, sync::Arc};
use tauri::{AppHandle, Runtime};

/// Start loading `song_id` and return the loading snapshot the coordinator
/// published. Decode, download and installation continue on a background
/// thread; a later call supersedes this one.
pub fn start<R: Runtime>(
    state: &AppState,
    app_handle: &AppHandle<R>,
    song_id: &str,
) -> Result<PlaybackStateSnapshot, PlaybackError> {
    let library_root = state
        .shell
        .library_root()
        .map_err(|error| PlaybackError::Internal(error.message))?;
    let connection = open_library_database(&library_root)?;

    let request = PlaybackRequest::begin(&state.playback)?;

    let song = cache::get_song_by_hash(&connection, song_id)
        .map_err(|e| PlaybackError::Internal(e.to_string()))?
        .ok_or_else(|| PlaybackError::SongNotFound(song_id.to_owned()))?;

    let ctx = LoadContext {
        state: state.clone(),
        app_handle: app_handle.clone(),
        app_data_dir: state.shell.app_data_dir.clone(),
        library_root,
        song_id: song.hash.clone(),
        request,
    };

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    ctx.send(PlaybackCommand::BeginLoad {
        request_id: ctx.request.id(),
        song_id: song.hash.clone(),
        reply: reply_tx,
    })?;
    let snapshot = reply_rx
        .blocking_recv()
        .map_err(|_| PlaybackError::Internal("playback coordinator dropped reply".to_owned()))??;

    std::thread::spawn(move || {
        if ctx.request.is_cancelled() {
            return;
        }
        if let Err(error) = run_load(&ctx, &song) {
            ctx.fail(error);
        }
    });

    Ok(snapshot)
}

/// Decode the cached stems for the track that is playing and attach them.
/// A track that already has stems, or a request the coordinator has since
/// superseded, resolves to the current snapshot instead.
pub fn attach_stems(state: &AppState) -> Result<PlaybackStateSnapshot, PlaybackError> {
    let library_root = state
        .shell
        .library_root()
        .map_err(|error| PlaybackError::Internal(error.message))?;
    let connection = open_library_database(&library_root)?;

    let (song_id, guard) = {
        let mut playback = state.playback.playback.lock().map_err(|_| {
            PlaybackError::Internal("playback controller lock was poisoned".to_owned())
        })?;

        let song_id = playback
            .current_song_id()
            .ok_or_else(|| PlaybackError::InvalidPlaybackState("no track is loaded".to_owned()))?
            .to_owned();

        if playback.has_stems() {
            return Ok(playback.snapshot());
        }

        (song_id, StalenessGuard::current(&state.playback))
    };

    let song = cache::get_song_by_hash(&connection, &song_id)
        .map_err(|e| PlaybackError::Internal(e.to_string()))?
        .ok_or_else(|| PlaybackError::SongNotFound(song_id.clone()))?;

    let loaded_stems = match source::load_cached_stems_for_song(
        Some(&state.shell.app_data_dir),
        &connection,
        &library_root,
        &song,
        guard.request_id(),
        guard.predicate(),
    ) {
        Ok(stems) => stems,
        Err(PlaybackError::StaleRequest) => return crate::services::playback::get_state(state),
        Err(error) => return Err(error),
    };

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    state
        .playback
        .command_tx
        .send(PlaybackCommand::AttachStems {
            request_id: guard.request_id(),
            song_id,
            stems: loaded_stems,
            reply: reply_tx,
        })
        .map_err(|_| PlaybackError::Internal("playback coordinator disconnected".to_owned()))?;
    reply_rx
        .blocking_recv()
        .map_err(|_| PlaybackError::Internal("playback coordinator dropped reply".to_owned()))?
}

/// Everything one load request needs, carried unchanged into the background
/// decode thread, the fetch-event listener and the reconnect driver.
struct LoadContext<R: Runtime> {
    state: AppState,
    app_handle: AppHandle<R>,
    app_data_dir: PathBuf,
    library_root: LibraryRoot,
    song_id: String,
    request: PlaybackRequest,
}

impl<R: Runtime> Clone for LoadContext<R> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            app_handle: self.app_handle.clone(),
            app_data_dir: self.app_data_dir.clone(),
            library_root: self.library_root.clone(),
            song_id: self.song_id.clone(),
            request: self.request.clone(),
        }
    }
}

impl<R: Runtime> LoadContext<R> {
    fn guard(&self) -> StalenessGuard {
        self.request.guard()
    }

    fn open_database(&self) -> Result<Connection, PlaybackError> {
        open_library_database(&self.library_root)
    }

    fn send(&self, command: PlaybackCommand) -> Result<(), PlaybackError> {
        self.state
            .playback
            .command_tx
            .send(command)
            .map_err(|_| PlaybackError::Internal("playback coordinator disconnected".to_owned()))
    }

    fn install(&self, ready: ReadyTrack) -> Result<(), PlaybackError> {
        self.send(PlaybackCommand::InstallReady {
            request_id: self.request.id(),
            song_id: self.song_id.clone(),
            ready: Box::new(ready),
        })
    }

    fn fail(&self, error: PlaybackError) {
        let _ = self.send(PlaybackCommand::FailLoad {
            request_id: self.request.id(),
            song_id: self.song_id.clone(),
            error,
        });
    }
}

fn open_library_database(library_root: &LibraryRoot) -> Result<Connection, PlaybackError> {
    cache::open_database(&library_root.database_path())
        .map_err(|e| PlaybackError::Internal(e.to_string()))
}

fn run_load<R: Runtime>(ctx: &LoadContext<R>, song: &Song) -> Result<(), PlaybackError> {
    if ctx.request.is_cancelled() {
        return Ok(());
    }

    let chunk_cache = ctx
        .state
        .remote
        .remote_chunk_cache()
        .map_err(|error| PlaybackError::Internal(error.message))?;
    let streaming_source = source::load_playback_source_streaming(
        Some(&ctx.app_data_dir),
        chunk_cache,
        &ctx.library_root,
        song,
    )?;

    if let Some(streaming_source) = streaming_source {
        return install_streaming(ctx, song, streaming_source);
    }

    let connection = ctx.open_database()?;
    install_decoded(ctx, &connection, song)
}

fn install_streaming<R: Runtime>(
    ctx: &LoadContext<R>,
    song: &Song,
    streaming_source: StreamingPlaybackSource,
) -> Result<(), PlaybackError> {
    if ctx.request.is_cancelled() {
        return Ok(());
    }

    let connection = ctx.open_database()?;
    if song.is_remote_stems() {
        // Guarded cache: a stale guard skips the remaining stem downloads and the rename.
        let _ = RemoteContent::new(Some(&ctx.app_data_dir)).ensure_stem_files_cached(
            &ctx.library_root,
            &connection,
            song,
            ctx.request.id(),
            ctx.guard().predicate(),
        );
    }

    let stems_track = match source::load_cached_stems_for_song_streaming(
        Some(&ctx.app_data_dir),
        &connection,
        &ctx.library_root,
        song,
    ) {
        Ok(Some(stems_source)) => {
            for handle in stems_source.decode_handles {
                let song_id = song.hash.clone();
                std::thread::spawn(move || {
                    if let Err(e) = handle.join() {
                        eprintln!("stem decode thread panicked for {song_id}: {e:?}");
                    }
                });
            }
            Some(Box::new(stems_source.streaming_track))
        }
        Ok(None) => None,
        Err(e) => {
            eprintln!("streaming stem load failed for {}: {e}", song.hash);
            None
        }
    };

    let (cdg, cdg_error) = load_cdg_packets(&ctx.library_root, song);

    ctx.install(ReadyTrack::Streaming {
        sample_rate: streaming_source.metadata.sample_rate_hz,
        channels: streaming_source.metadata.channels,
        duration_ms: streaming_source.metadata.duration_ms.unwrap_or(0),
        original: streaming_source.streaming_track,
        stems: stems_track,
        cdg,
        cdg_error,
    })?;

    if let Some(fetch_event_rx) = streaming_source.fetch_event_rx {
        streaming::spawn_fetch_event_listener(
            ctx.clone(),
            streaming_source.cache_pin_guard,
            fetch_event_rx,
            streaming::INITIAL_FETCH,
        );
    }

    let song_id = song.hash.clone();
    let decode_handle = streaming_source.decode_handle;
    std::thread::spawn(move || {
        if let Err(e) = decode_handle.join() {
            eprintln!("decode thread panicked for {song_id}: {e:?}");
        }
    });

    Ok(())
}

fn install_decoded<R: Runtime>(
    ctx: &LoadContext<R>,
    connection: &Connection,
    song: &Song,
) -> Result<(), PlaybackError> {
    let PlaybackSourceLoad {
        decoded_audio,
        stems,
    } = source::load_playback_source(
        Some(&ctx.app_data_dir),
        connection,
        &ctx.library_root,
        song,
        ctx.request.id(),
        ctx.guard().predicate(),
    )?;

    if ctx.request.is_cancelled() {
        return Ok(());
    }

    let (cdg, cdg_error) = load_cdg_packets(&ctx.library_root, song);

    ctx.install(ReadyTrack::Decoded {
        audio: decoded_audio,
        stems,
        cdg,
        cdg_error,
    })
}

/// `(packets, error_code)`; audio continues when CDG load fails.
fn load_cdg_packets(
    library_root: &LibraryRoot,
    song: &Song,
) -> (Option<Arc<[crate::cdg::CdgPacket]>>, Option<CdgErrorCode>) {
    match load_cdg_packets_for_song(library_root, song) {
        CdgLoadResult::Loaded(result) => {
            if let Some(diag) = &result.diagnostic {
                eprintln!(
                    "warning: CDG parse diagnostic for {}: {:?}",
                    song.hash, diag
                );
            }
            (Some(Arc::from(result.packets.into_boxed_slice())), None)
        }
        CdgLoadResult::Missing => (None, None),
        CdgLoadResult::ReadFailed => (None, Some(CdgErrorCode::ReadFailed)),
        CdgLoadResult::ZipFailed => (None, Some(CdgErrorCode::ZipFailed)),
    }
}
