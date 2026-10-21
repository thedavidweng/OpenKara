//! Source-bound Online Preview through the shared playback coordinator.
//!
//! SourceHost validates every media request and redirect. One sequential GET
//! fills a seekable chunk cache. A preview has no library row and refreshes
//! its descriptor at most once. The work lease survives until playback and
//! all decoder/download workers stop.

use super::{
    preview_stream::{self, Alive, PreviewMedia, PreviewSource},
    request::PlaybackRequest,
    streaming::playback_position_for_reconnect,
};
use crate::{
    audio::{
        chunked_cache::ChunkedCache,
        coordinator::{PlaybackCommand, ReadyTrack},
        error::PlaybackError,
        playback::{PlaybackStateSnapshot, PLAYBACK_ERROR_EVENT},
        remote_source::FetchEvent,
        streaming::{self, StreamMetadata, StreamingTrack},
    },
    catalog::{streaming_preview_queue_id, ImportRefusalReason, StreamingPreviewOutcome},
    commands::{
        catalog::resolve_streaming_preview,
        error::{CommandError, ErrorCode, FallbackAction},
    },
    remote::content::probe_remote_source,
    services::playback::PlaybackErrorEvent,
    source_host::{Cancellation, SourceHost, WorkKind, WorkLease},
    state::AppState,
};
use std::{
    path::Path,
    sync::{mpsc, Arc},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};

const PREVIEW_CACHE_DIR: &str = "preview-cache";
/// Longest wait for the startup buffer before decoding starts anyway.
const STARTUP_BUFFER_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone)]
struct PreviewContext {
    state: AppState,
    app_handle: AppHandle,
    request: PlaybackRequest,
    song_id: String,
    source_id: String,
    host: SourceHost,
    work: Arc<WorkLease>,
    cancellation: Cancellation,
    remote_track_id: String,
}

impl PreviewContext {
    fn is_active(&self) -> bool {
        self.request.guard().is_current() && !self.cancellation.is_cancelled()
    }

    fn alive(&self) -> Alive {
        let guard = self.request.guard();
        let cancellation = self.cancellation.clone();
        let work = Arc::clone(&self.work);
        Arc::new(move || {
            let _work = &work;
            guard.is_current() && !cancellation.is_cancelled()
        })
    }

    fn send(&self, command: PlaybackCommand) -> Result<(), PlaybackError> {
        self.state
            .playback
            .command_tx
            .send(command)
            .map_err(|_| PlaybackError::Internal("playback coordinator disconnected".to_owned()))
    }

    fn fail_load(&self, error: PlaybackError) {
        let _ = self.send(PlaybackCommand::FailLoad {
            request_id: self.request.id(),
            song_id: self.song_id.clone(),
            error,
        });
    }

    /// Failure after playback began: `FailLoad` only applies while loading,
    /// so tell the frontend directly.
    fn report_stream_failure(&self, message: String) {
        let _ = self.app_handle.emit(
            PLAYBACK_ERROR_EVENT,
            PlaybackErrorEvent {
                song_id: self.song_id.clone(),
                error: CommandError::new(
                    ErrorCode::NetworkUnavailable,
                    message,
                    true,
                    FallbackAction::Retry,
                ),
            },
        );
    }
}

/// Begin a source-bound preview without creating a library song.
pub(crate) fn start(
    state: &AppState,
    app_handle: &AppHandle,
    source_id: &str,
    remote_track_id: &str,
) -> Result<PlaybackStateSnapshot, PlaybackError> {
    let host = app_handle.state::<SourceHost>().inner().clone();
    let work = Arc::new(
        host.acquire_work(source_id, WorkKind::Preview)
            .map_err(|error| PlaybackError::Internal(error.message))?,
    );
    let cancellation = work.cancellation();
    let request = PlaybackRequest::begin(&state.playback)?;
    let ctx = PreviewContext {
        state: state.clone(),
        app_handle: app_handle.clone(),
        request,
        song_id: streaming_preview_queue_id(source_id, remote_track_id),
        remote_track_id: remote_track_id.to_owned(),
        source_id: source_id.to_owned(),
        host,
        work,
        cancellation,
    };

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    ctx.send(PlaybackCommand::BeginLoad {
        request_id: ctx.request.id(),
        song_id: ctx.song_id.clone(),
        reply: reply_tx,
    })?;
    let snapshot = reply_rx
        .blocking_recv()
        .map_err(|_| PlaybackError::Internal("playback coordinator dropped reply".to_owned()))??;

    supervise_preview(ctx.clone());
    std::thread::spawn(move || {
        if ctx.request.is_cancelled() {
            return;
        }
        if let Err(error) = run(&ctx) {
            ctx.fail_load(error);
        }
    });

    Ok(snapshot)
}

fn run(ctx: &PreviewContext) -> Result<(), PlaybackError> {
    let mut refreshed = false;
    let (cache, pipeline) = loop {
        if !ctx.is_active() {
            return Ok(());
        }
        let (media, extension) = resolve_url(ctx)?;
        let attempt = (|| {
            let file_size = content_length(&media, &ctx.alive())?;
            let cache_dir = ctx.state.shell.app_data_dir.join(PREVIEW_CACHE_DIR);
            let cache = open_cache(&cache_dir, &ctx.song_id, ctx.request.id(), file_size)?;
            let pipeline = open_pipeline(&cache, media, &extension, ctx.alive())?;
            Ok::<_, PlaybackError>((cache, pipeline))
        })();
        match attempt {
            Ok(ready) => break ready,
            Err(_) if !refreshed && ctx.is_active() => refreshed = true,
            Err(error) => return Err(error),
        }
    };
    if !ctx.is_active() {
        return Ok(());
    }
    ctx.send(PlaybackCommand::InstallReady {
        request_id: ctx.request.id(),
        song_id: ctx.song_id.clone(),
        ready: Box::new(ReadyTrack::Streaming {
            sample_rate: pipeline.metadata.sample_rate_hz,
            channels: pipeline.metadata.channels,
            duration_ms: pipeline.metadata.duration_ms.unwrap_or(0),
            original: pipeline.track,
            stems: None,
            cdg: None,
            cdg_error: None,
        }),
    })?;
    spawn_fetch_listener(ctx.clone(), cache, pipeline.fetch_event_rx, refreshed);
    std::thread::spawn(move || {
        let _ = pipeline.decode_handle.join();
        let _ = pipeline.download_handle.join();
        drop(pipeline.stop);
    });
    Ok(())
}

fn supervise_preview(ctx: PreviewContext) {
    std::thread::spawn(move || loop {
        let snapshot = match ctx.state.playback.playback.lock() {
            Ok(mut playback) => playback.snapshot(),
            Err(_) => return,
        };
        let active = snapshot.song_id.as_deref() == Some(ctx.song_id.as_str());
        let finished = active
            && !snapshot.is_playing
            && snapshot
                .duration_ms
                .is_some_and(|duration| snapshot.position_ms >= duration);
        if finished {
            ctx.cancellation.cancel();
            return;
        }
        if ctx.cancellation.is_cancelled() {
            ctx.cancellation.cancel();
            let (reply, receiver) = tokio::sync::oneshot::channel();
            if ctx
                .send(PlaybackCommand::CancelPreview {
                    request_id: ctx.request.id(),
                    song_id: ctx.song_id.clone(),
                    reply,
                })
                .is_ok()
            {
                let _ = receiver.blocking_recv();
            }
            return;
        }
        if !ctx.request.guard().is_current() || !active {
            ctx.cancellation.cancel();
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    });
}

/// The current audio URL and container extension for the context's track.
fn resolve_url(ctx: &PreviewContext) -> Result<(PreviewMedia, String), PlaybackError> {
    let outcome = resolve_streaming_preview(
        &ctx.app_handle,
        &ctx.source_id,
        &ctx.remote_track_id,
        ctx.cancellation.clone(),
    )
    .map_err(|error| PlaybackError::Internal(error.message))?;
    match outcome {
        StreamingPreviewOutcome::Stream(resolved) => Ok((
            PreviewMedia::Host {
                host: ctx.host.clone(),
                source_id: ctx.source_id.clone(),
                descriptor: resolved.descriptor,
                cancellation: ctx.cancellation.clone(),
            },
            resolved.extension,
        )),
        StreamingPreviewOutcome::Refusal(refusal) => Err(PlaybackError::Internal(
            match refusal.reason {
                ImportRefusalReason::TrialClip => {
                    "the source serves only a trial clip for this track"
                }
                ImportRefusalReason::NoPlayRights => "the source has no play rights for this track",
                ImportRefusalReason::EmptyUrl => "the source returned no audio for this track",
                ImportRefusalReason::Unavailable => "this track is unavailable from the source",
            }
            .to_owned(),
        )),
    }
}

fn content_length(media: &PreviewMedia, alive: &Alive) -> Result<u64, PlaybackError> {
    let response = media
        .open(0, Some(0), alive)
        .map_err(|error| PlaybackError::Internal(error.message))?;
    if response.status() != 206 {
        return Err(PlaybackError::Internal(
            "the audio host does not support ranged streaming".to_owned(),
        ));
    }
    response
        .content_range()
        .and_then(total_from_content_range)
        .filter(|size| *size > 0)
        .ok_or_else(|| PlaybackError::Internal("the audio host did not report a size".to_owned()))
}

/// `bytes 0-0/12345` -> `12345`.
fn total_from_content_range(value: &str) -> Option<u64> {
    value.rsplit('/').next()?.trim().parse().ok()
}

/// A fresh chunked cache for this request. Older preview files are removed
/// first so previews never accumulate on disk.
fn open_cache(
    dir: &Path,
    _song_id: &str,
    request_id: u64,
    file_size: u64,
) -> Result<Arc<ChunkedCache>, PlaybackError> {
    std::fs::create_dir_all(dir)
        .map_err(|error| PlaybackError::Internal(format!("preview cache unavailable: {error}")))?;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let key = format!("preview-{request_id}");
    ChunkedCache::open(dir, &key, file_size)
        .map(Arc::new)
        .map_err(|error| PlaybackError::Internal(format!("preview cache unavailable: {error}")))
}

struct Pipeline {
    track: StreamingTrack,
    metadata: StreamMetadata,
    decode_handle: std::thread::JoinHandle<Result<(), crate::audio::decode::DecodeError>>,
    download_handle: std::thread::JoinHandle<()>,
    fetch_event_rx: mpsc::Receiver<FetchEvent>,
    stop: PipelineStop,
}

struct PipelineStop(Arc<std::sync::atomic::AtomicBool>);

impl Drop for PipelineStop {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Downloader + probe + decode producer over `cache`, downloading `url`.
/// Reusing `cache` lets a re-resolved URL continue from what is downloaded.
/// Returns once about three seconds of audio are buffered.
fn open_pipeline(
    cache: &Arc<ChunkedCache>,
    media: PreviewMedia,
    extension: &str,
    alive: Alive,
) -> Result<Pipeline, PlaybackError> {
    let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop = PipelineStop(Arc::clone(&stopped));
    let local_stop = Arc::clone(&stopped);
    let alive: Alive =
        Arc::new(move || !local_stop.load(std::sync::atomic::Ordering::SeqCst) && alive());
    let download = preview_stream::spawn(media, Arc::clone(cache), Arc::clone(&alive));
    let probe_source = PreviewSource::with_alive(
        Arc::clone(cache),
        download.wanted.clone(),
        Arc::clone(&alive),
    );
    let metadata = probe_remote_source(probe_source, Some(extension))
        .map_err(|error| PlaybackError::AudioDecodeFailed(error.to_string()))?;

    let startup = preview_stream::startup_bytes(cache.file_size(), metadata.duration_ms);
    preview_stream::wait_buffered(
        cache,
        startup,
        &alive,
        &download.events,
        STARTUP_BUFFER_TIMEOUT,
    )
    .map_err(|event| {
        PlaybackError::Internal(
            match event {
                FetchEvent::UrlExpired => "the preview link expired while buffering",
                FetchEvent::RangeNotSupported => "the audio host does not support ranged streaming",
                FetchEvent::ConsecutiveFailures { .. } => "the preview could not be downloaded",
            }
            .to_owned(),
        )
    })?;

    let decode_source =
        PreviewSource::with_alive(Arc::clone(cache), download.wanted, alive).stop_on_drop(stopped);
    let (consumer, decode_handle) = streaming::spawn_decode_producer_from_source(
        Box::new(decode_source),
        Some(extension),
        &metadata,
        streaming::ProxyConfig::none(),
    )
    .map_err(|error| PlaybackError::AudioDecodeFailed(error.to_string()))?;
    Ok(Pipeline {
        track: StreamingTrack::Single { consumer },
        metadata,
        decode_handle,
        fetch_event_rx: download.events,
        download_handle: download.handle,
        stop,
    })
}

/// Watches one preview's fetch thread. The first URL expiry or run of
/// failures re-resolves the URL once and swaps the source in at the current
/// position; any later problem is reported and ends the listener.
fn spawn_fetch_listener(
    ctx: PreviewContext,
    cache: Arc<ChunkedCache>,
    fetch_event_rx: mpsc::Receiver<FetchEvent>,
    mut refreshed: bool,
) {
    std::thread::spawn(move || {
        let mut events = fetch_event_rx;
        loop {
            if !ctx.is_active() {
                return;
            }
            let event = match events.recv_timeout(Duration::from_millis(50)) {
                Ok(event) => event,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            };
            if !ctx.request.guard().is_current() {
                return;
            }
            match event {
                FetchEvent::RangeNotSupported => {
                    ctx.report_stream_failure(
                        "the audio host does not support ranged streaming".to_owned(),
                    );
                    ctx.cancellation.cancel();
                    return;
                }
                FetchEvent::ConsecutiveFailures { .. } | FetchEvent::UrlExpired => {
                    if refreshed {
                        ctx.report_stream_failure(
                            "the preview stream failed again after refreshing its link".to_owned(),
                        );
                        ctx.cancellation.cancel();
                        return;
                    }
                    refreshed = true;
                    match swap_source(&ctx, &cache) {
                        Ok(Some(next_events)) => events = next_events,
                        Ok(None) => return,
                        Err(error) => {
                            ctx.report_stream_failure(format!(
                                "could not refresh the preview link: {error}"
                            ));
                            ctx.cancellation.cancel();
                            return;
                        }
                    }
                }
            }
        }
    });
}

/// Re-resolve the URL and replace the playing source. `None` means the
/// request was superseded or the track is no longer the active one.
fn swap_source(
    ctx: &PreviewContext,
    cache: &Arc<ChunkedCache>,
) -> Result<Option<mpsc::Receiver<FetchEvent>>, PlaybackError> {
    let position_ms = {
        let playback = ctx.state.playback.playback.lock().map_err(|_| {
            PlaybackError::Internal("playback controller lock was poisoned".to_owned())
        })?;
        playback_position_for_reconnect(&playback, &ctx.song_id)
    };
    let Some(position_ms) = position_ms else {
        return Ok(None);
    };

    let (media, next_extension) = resolve_url(ctx)?;
    if !ctx.request.guard().is_current() {
        return Ok(None);
    }
    let pipeline = open_pipeline(cache, media, &next_extension, ctx.alive())?;
    ctx.send(PlaybackCommand::ReplaceStreamingSource {
        request_id: ctx.request.id(),
        song_id: ctx.song_id.clone(),
        position_ms,
        new_source: Box::new(pipeline.track),
    })?;
    std::thread::spawn(move || {
        let _ = pipeline.decode_handle.join();
        let _ = pipeline.download_handle.join();
        drop(pipeline.stop);
    });
    Ok(Some(pipeline.fetch_event_rx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn content_range_total_is_the_part_after_the_slash() {
        assert_eq!(total_from_content_range("bytes 0-0/12345"), Some(12345));
        assert_eq!(total_from_content_range("bytes 0-0/*"), None);
        assert_eq!(total_from_content_range("garbage"), None);
    }

    fn wav(sample_rate: u32, frames: u32) -> Vec<u8> {
        let data_len = frames * 4;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.extend((0..data_len).map(|index| (index % 251) as u8));
        bytes
    }

    /// Serves `body` with HTTP range support on a loopback port.
    fn serve(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let body = body.clone();
                std::thread::spawn(move || {
                    let mut buffer = [0u8; 2048];
                    let Ok(read) = stream.read(&mut buffer) else {
                        return;
                    };
                    let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                    let range = request
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("range: bytes=")
                                .map(str::to_owned)
                        })
                        .and_then(|value| {
                            let (start, end) = value.split_once('-')?;
                            Some((
                                start.parse::<usize>().ok()?,
                                end.trim().parse::<usize>().unwrap_or(usize::MAX),
                            ))
                        });
                    let (start, end) = range.unwrap_or((0, body.len() - 1));
                    let end = end.min(body.len() - 1);
                    let part = &body[start..=end];
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len(),
                        part.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(part);
                });
            }
        });
        format!("http://{address}/track.wav")
    }

    #[test]
    fn the_remote_library_pipeline_streams_a_plain_url() {
        let body = wav(44_100, 44_100);
        let url = serve(body.clone());

        let alive: Alive = Arc::new(|| true);
        let size = content_length(&PreviewMedia::Http(url.clone()), &alive).expect("size");
        assert_eq!(size, body.len() as u64);

        let dir = tempfile::tempdir().expect("dir");
        let cache = open_cache(dir.path(), "stream:fixture:7", 1, size).expect("cache");
        let pipeline = open_pipeline(&cache, PreviewMedia::Http(url), "wav", Arc::new(|| true))
            .expect("pipeline");
        assert_eq!(pipeline.metadata.sample_rate_hz, 44_100);
        assert_eq!(pipeline.metadata.channels, 2);
        pipeline
            .decode_handle
            .join()
            .expect("decode thread")
            .expect("decoded the whole file");
        assert!(cache.is_complete());
    }

    #[test]
    fn opening_a_cache_clears_earlier_previews() {
        let dir = tempfile::tempdir().expect("dir");
        let first = open_cache(dir.path(), "stream:fixture:1", 1, 64).expect("first");
        let first_path = first.path().to_path_buf();
        drop(first);
        let _second = open_cache(dir.path(), "stream:fixture:2", 2, 64).expect("second");
        assert!(!first_path.exists());
    }
}
