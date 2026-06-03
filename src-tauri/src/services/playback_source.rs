use crate::{
    audio::{
        chunked_cache::{CacheError, CacheManager},
        decode,
        error::PlaybackError,
        playback::{LoadedStems, StemSet},
        remote_source::{self, BandwidthMonitor, FetchEvent, RemoteMediaSource},
        streaming::{self, StreamMetadata, StreamingTrack},
    },
    cache,
    commands::remote_library,
    library::Song,
    library_root::LibraryRoot,
    media_g::{self, MEDIA_G_ZIP},
};
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::{
    path::Path,
    sync::{mpsc, Arc, Mutex},
};

pub(crate) struct PlaybackSourceLoad {
    pub(crate) decoded_audio: decode::DecodedAudio,
    pub(crate) stems: Option<LoadedStems>,
}

pub(crate) fn probe_song_audio(library_root: &LibraryRoot, song: &Song) -> Result<()> {
    let song_path = resolve_song_file_path(song)?;
    let absolute_path = library_root.resolve(song_path);
    if song.media_g_container.as_deref() == Some(MEDIA_G_ZIP) {
        let asset = media_g::inspect_zip_for_media_g(&absolute_path)?;
        return decode::probe_bytes(asset.audio_bytes, &asset.audio_extension)
            .map_err(|e| anyhow::anyhow!("failed to probe audio for {}: {}", song_path, e));
    }

    decode::probe_file(&absolute_path)
        .map_err(|e| anyhow::anyhow!("failed to probe audio for {}: {}", song_path, e))
}

pub(crate) fn load_song_audio(
    library_root: &LibraryRoot,
    song: &Song,
) -> Result<decode::DecodedAudio> {
    let song_path = resolve_song_file_path(song)?;
    let absolute_path = library_root.resolve(song_path);
    if song.media_g_container.as_deref() == Some(MEDIA_G_ZIP) {
        let asset = media_g::inspect_zip_for_media_g(&absolute_path)?;
        return decode::decode_bytes(asset.audio_bytes, &asset.audio_extension)
            .map_err(|e| anyhow::anyhow!("failed to decode audio for {}: {}", song_path, e));
    }

    decode::decode_file(&absolute_path)
        .map_err(|e| anyhow::anyhow!("failed to decode audio for {}: {}", song_path, e))
}

pub(crate) fn load_playback_source(
    app_data_dir: Option<&Path>,
    connection: &Connection,
    library_root: &LibraryRoot,
    song: &Song,
) -> Result<PlaybackSourceLoad, PlaybackError> {
    if song.is_remote_stems() {
        return load_remote_stems_playback_source(connection, library_root, song)
            .map_err(|e| PlaybackError::Internal(e.to_string()));
    }

    if song.is_remote() {
        ensure_remote_song_files_cached(app_data_dir, song)
            .map_err(|e| PlaybackError::Internal(e.to_string()))?;
    }

    Ok(PlaybackSourceLoad {
        decoded_audio: load_song_audio(library_root, song)
            .map_err(|e| PlaybackError::AudioDecodeFailed(e.to_string()))?,
        stems: None,
    })
}

pub(crate) fn load_cached_stems_for_song(
    app_data_dir: Option<&Path>,
    connection: &Connection,
    library_root: &LibraryRoot,
    song: &Song,
) -> Result<LoadedStems, PlaybackError> {
    if song.is_remote_stems() {
        ensure_remote_stem_files_cached(app_data_dir, connection, song)
            .map_err(|e| PlaybackError::Internal(e.to_string()))?;
        return load_remote_stems_playback_source(connection, library_root, song)
            .map_err(|e| PlaybackError::Internal(e.to_string()))?
            .stems
            .ok_or_else(|| {
                PlaybackError::KaraokeNotReady(
                    "remote stems song did not yield attached stems".to_owned(),
                )
            });
    }

    let cached = cache::stems::get_cached_stem_entry(connection, &song.hash)
        .map_err(|e| PlaybackError::Internal(format!("failed to load cached stems: {e}")))?
        .ok_or_else(|| {
            PlaybackError::KaraokeNotReady(format!("no cached stems for song {}", song.hash))
        })?;

    decode_stem_entry(library_root, &cached)
        .map_err(|e| PlaybackError::AudioDecodeFailed(e.to_string()))
}

/// Result of loading stems in streaming mode.
pub(crate) struct StreamingStemsSource {
    pub(crate) streaming_track: StreamingTrack,
    #[allow(dead_code)]
    pub(crate) metadata: Vec<streaming::StreamMetadata>,
    pub(crate) decode_handles: Vec<std::thread::JoinHandle<Result<(), decode::DecodeError>>>,
}

/// Load cached stems for streaming playback. Spawns one decode thread per stem
/// file, each writing into its own ring buffer. Returns `None` for remote stems
/// (which need caching first) or Media+G containers.
pub(crate) fn load_cached_stems_for_song_streaming(
    _app_data_dir: Option<&Path>,
    connection: &Connection,
    library_root: &LibraryRoot,
    song: &Song,
) -> Result<Option<StreamingStemsSource>, PlaybackError> {
    let Some(cached) = cache::stems::get_cached_stem_entry(connection, &song.hash)
        .map_err(|e| PlaybackError::Internal(format!("failed to load cached stems: {e}")))?
    else {
        return Ok(None);
    };

    let paths: Vec<std::path::PathBuf> =
        if cached.has_individual_stems() {
            vec![
                library_root.resolve(&cached.vocals_path),
                library_root.resolve(cached.drums_path.as_deref().ok_or_else(|| {
                    PlaybackError::Internal("missing drums stem path".to_owned())
                })?),
                library_root.resolve(
                    cached.bass_path.as_deref().ok_or_else(|| {
                        PlaybackError::Internal("missing bass stem path".to_owned())
                    })?,
                ),
                library_root.resolve(cached.other_path.as_deref().ok_or_else(|| {
                    PlaybackError::Internal("missing other stem path".to_owned())
                })?),
            ]
        } else {
            vec![
                library_root.resolve(&cached.vocals_path),
                library_root.resolve(&cached.accomp_path),
            ]
        };

    let result = streaming::spawn_multi_stem_decode_producers(&paths)
        .map_err(|e| PlaybackError::AudioDecodeFailed(e.to_string()))?;

    Ok(Some(StreamingStemsSource {
        streaming_track: result.track,
        metadata: result.metadata,
        decode_handles: result.decode_handles,
    }))
}

/// Result of loading a playback source in streaming mode.
pub(crate) struct StreamingPlaybackSource {
    pub(crate) streaming_track: StreamingTrack,
    pub(crate) metadata: StreamMetadata,
    pub(crate) decode_handle: std::thread::JoinHandle<Result<(), decode::DecodeError>>,
    /// Receiver for fetch events (only present for remote streaming).
    /// The caller should consume these to handle ConsecutiveFailures, etc.
    pub(crate) fetch_event_rx: Option<mpsc::Receiver<FetchEvent>>,
    /// Bandwidth monitor for the fetch thread (only present for remote streaming).
    /// Stored so the caller can inspect bandwidth or reconfigure thresholds.
    #[allow(dead_code)]
    pub(crate) bandwidth_monitor: Option<Arc<BandwidthMonitor>>,
}

/// Load a song for streaming playback. Returns the ring-buffer consumer,
/// metadata, and a join handle for the decode thread.
///
/// For local files, decodes directly from disk. For remote songs, creates a
/// `RemoteMediaSource` that fetches byte ranges on demand via HTTP Range
/// requests, enabling edge-downloaded playback without pre-downloading the
/// entire file.
///
/// Falls back to full decode for Media+G containers (which require in-memory
/// byte extraction).
pub(crate) fn load_playback_source_streaming(
    app_data_dir: Option<&Path>,
    remote_chunk_cache: &Mutex<CacheManager>,
    library_root: &LibraryRoot,
    song: &Song,
) -> Result<Option<StreamingPlaybackSource>, PlaybackError> {
    // Media+G containers require in-memory extraction — can't stream from disk.
    if song.media_g_container.as_deref() == Some(MEDIA_G_ZIP) {
        return Ok(None);
    }

    if song.is_remote() {
        return load_remote_streaming_source(app_data_dir, remote_chunk_cache, library_root, song);
    }

    let song_path =
        resolve_song_file_path(song).map_err(|e| PlaybackError::Internal(e.to_string()))?;
    let absolute_path = library_root.resolve(song_path);

    let (consumer, metadata, decode_handle) = streaming::spawn_decode_producer(&absolute_path)
        .map_err(|e| PlaybackError::AudioDecodeFailed(e.to_string()))?;

    Ok(Some(StreamingPlaybackSource {
        streaming_track: StreamingTrack::Single { consumer },
        metadata,
        decode_handle,
        fetch_event_rx: None,
        bandwidth_monitor: None,
    }))
}

/// Load a remote song for streaming playback via HTTP Range requests.
///
/// Creates a `RemoteMediaSource` backed by a `ChunkedCache` and a background
/// fetch thread. Returns `Ok(None)` if the provider doesn't support Range
/// requests (caller should fall back to full-file download).
fn load_remote_streaming_source(
    app_data_dir: Option<&Path>,
    remote_chunk_cache: &Mutex<CacheManager>,
    _library_root: &LibraryRoot,
    song: &Song,
) -> Result<Option<StreamingPlaybackSource>, PlaybackError> {
    let Some(app_data_dir) = app_data_dir else {
        return Ok(None);
    };

    let song_path =
        resolve_song_file_path(song).map_err(|e| PlaybackError::Internal(e.to_string()))?;

    // Create provider and check Range support.
    let library = remote_library::active_remote_library(app_data_dir)
        .map_err(|e| PlaybackError::Internal(e.message.clone()))?;
    let Some(library) = library else {
        return Ok(None);
    };
    let provider = remote_library::provider::create_provider(app_data_dir, &library)
        .map_err(|e| PlaybackError::Internal(e.message.clone()))?;

    let fetcher = match provider.create_range_fetcher(song_path) {
        Ok(Some(f)) => f,
        Ok(None) => return Ok(None), // Provider doesn't support Range — fall back.
        Err(_) => return Ok(None),   // Can't create fetcher — fall back.
    };

    // Get file size for the cache.
    let file_size = provider
        .get_file_size(song_path)
        .map_err(|e| PlaybackError::Internal(e.message.clone()))?
        .unwrap_or(0);
    if file_size == 0 {
        return Ok(None); // Can't determine size — fall back.
    }

    let cache_key = format!("remote-{}", song.hash);
    let cache = {
        let mut manager = remote_chunk_cache.lock().map_err(|_| {
            PlaybackError::Internal("remote chunk cache manager lock was poisoned".to_owned())
        })?;
        manager
            .get_or_create(&cache_key, file_size)
            .map_err(map_cache_error)?
    };

    // Spawn the fetch thread.
    let (fetch_tx, fetch_event_rx, bandwidth_monitor, _fetch_handle) =
        remote_source::spawn_fetch_thread_with_fetcher(
            String::new(), // URL is embedded in the fetcher
            Arc::clone(&cache),
            fetcher,
            remote_source::RetryConfig::default(),
        );

    let extension = std::path::Path::new(song_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_owned());

    // Create a RemoteMediaSource for probing. The probe consumes the source,
    // but the underlying ChunkedCache is shared, so data fetched during
    // probing is available to the second source used for decoding.
    let probe_source = RemoteMediaSource::new(Arc::clone(&cache), fetch_tx.clone());
    let probe_metadata = probe_remote_source(probe_source, extension.as_deref())
        .map_err(|e| PlaybackError::AudioDecodeFailed(e.to_string()))?;

    // Create the decode source with startup buffering (~1s at 128kbps).
    let startup_bytes = file_size.min(16 * 1024);
    let decode_source = RemoteMediaSource::new(cache, fetch_tx).with_startup_buffer(startup_bytes);

    // Spawn the decode producer from the remote source.
    // Pass the bandwidth monitor's slow flag so the decode producer can
    // dynamically switch to frame decimation when the connection is slow.
    let slow_flag = bandwidth_monitor.slow_flag();
    let (consumer, decode_handle) = streaming::spawn_decode_producer_from_source(
        Box::new(decode_source),
        extension.as_deref(),
        &probe_metadata,
        streaming::ProxyConfig::none(),
        Some(slow_flag),
    )
    .map_err(|e| PlaybackError::AudioDecodeFailed(e.to_string()))?;

    Ok(Some(StreamingPlaybackSource {
        streaming_track: StreamingTrack::Single { consumer },
        metadata: probe_metadata,
        decode_handle,
        fetch_event_rx: Some(fetch_event_rx),
        bandwidth_monitor: Some(bandwidth_monitor),
    }))
}

/// Probe a `RemoteMediaSource` for audio metadata. Consumes the source
/// (symphonia takes ownership of the `MediaSourceStream`).
fn probe_remote_source(
    source: RemoteMediaSource,
    extension: Option<&str>,
) -> Result<StreamMetadata, decode::DecodeError> {
    use symphonia::core::{
        formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
    };

    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }

    let mss = MediaSourceStream::new(Box::new(source), Default::default());
    let mut probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| decode::DecodeError::ProbeFailed(format!("remote source: {e}")))?;

    let (codec_params, track_id) = {
        let track = probed
            .format
            .default_track()
            .ok_or(decode::DecodeError::NoDefaultTrack)?;
        (track.codec_params.clone(), track.id)
    };

    let mut sample_rate = codec_params.sample_rate;
    let mut channels = codec_params.channels.map(|c| c.count());

    // Some containers don't expose sample rate / channel layout in the
    // codec params.  symphonia only populates these after decoding the
    // first packet, so try that before giving up.
    if sample_rate.is_none() || channels.is_none() {
        use symphonia::core::codecs::DecoderOptions as DO;
        if let Ok(mut decoder) =
            symphonia::default::get_codecs().make(&codec_params, &DO::default())
        {
            while let Ok(packet) = probed.format.next_packet() {
                if packet.track_id() != track_id {
                    continue;
                }
                if let Ok(decoded) = decoder.decode(&packet) {
                    let spec = *decoded.spec();
                    sample_rate.get_or_insert(spec.rate);
                    channels.get_or_insert(spec.channels.count());
                    break;
                }
            }
        }
    }

    let sample_rate = sample_rate.ok_or(decode::DecodeError::MissingSampleRate)?;
    let channels = channels.ok_or(decode::DecodeError::MissingChannels)?;

    // Try to get duration from container metadata.
    let duration_ms =
        if let (Some(n_frames), Some(tb)) = (codec_params.n_frames, codec_params.time_base) {
            let time = tb.calc_time(n_frames);
            (time.seconds * 1000) + (time.frac * 1000.0) as u64
        } else {
            // For remote sources, we can't fall back to full decode for duration.
            // Use 0 and let the UI handle it gracefully.
            0
        };

    Ok(StreamMetadata {
        sample_rate,
        channels,
        duration_ms,
    })
}

pub(crate) fn resolve_song_file_path(song: &Song) -> Result<&str> {
    song.file_path
        .as_deref()
        .with_context(|| format!("song {} does not have a local file path", song.hash))
}

pub(crate) fn ensure_remote_song_files_cached(
    app_data_dir: Option<&Path>,
    song: &Song,
) -> Result<()> {
    let Some(app_data_dir) = app_data_dir else {
        return Ok(());
    };
    if let Some(file_path) = song.file_path.as_deref() {
        remote_library::ensure_remote_file_cached(app_data_dir, file_path)
            .map_err(|error| anyhow::anyhow!(error.message.clone()))?;
    }
    if let Some(cdg_path) = song.cdg_path.as_deref() {
        remote_library::ensure_remote_file_cached(app_data_dir, cdg_path)
            .map_err(|error| anyhow::anyhow!(error.message.clone()))?;
    }
    Ok(())
}

pub(crate) fn ensure_remote_stem_files_cached(
    app_data_dir: Option<&Path>,
    connection: &Connection,
    song: &Song,
) -> Result<()> {
    let Some(app_data_dir) = app_data_dir else {
        return Ok(());
    };
    let Some(cached) = cache::stems::get_cached_stem_entry(connection, &song.hash)
        .context("failed to load cached stems")?
    else {
        return Ok(());
    };

    remote_library::ensure_remote_file_cached(app_data_dir, &cached.vocals_path)
        .map_err(|error| anyhow::anyhow!(error.message.clone()))?;
    remote_library::ensure_remote_file_cached(app_data_dir, &cached.accomp_path)
        .map_err(|error| anyhow::anyhow!(error.message.clone()))?;
    if let Some(drums_path) = cached.drums_path.as_deref() {
        remote_library::ensure_remote_file_cached(app_data_dir, drums_path)
            .map_err(|error| anyhow::anyhow!(error.message.clone()))?;
    }
    if let Some(bass_path) = cached.bass_path.as_deref() {
        remote_library::ensure_remote_file_cached(app_data_dir, bass_path)
            .map_err(|error| anyhow::anyhow!(error.message.clone()))?;
    }
    if let Some(other_path) = cached.other_path.as_deref() {
        remote_library::ensure_remote_file_cached(app_data_dir, other_path)
            .map_err(|error| anyhow::anyhow!(error.message.clone()))?;
    }
    Ok(())
}

fn load_remote_stems_playback_source(
    connection: &Connection,
    library_root: &LibraryRoot,
    song: &Song,
) -> Result<PlaybackSourceLoad> {
    let cached = cache::stems::get_cached_stem_entry(connection, &song.hash)
        .context("failed to load cached stems")?
        .with_context(|| format!("no cached stems for song {}", song.hash))?;

    if cached.has_individual_stems() {
        let LoadedStems::FourStem(StemSet {
            vocals,
            drums,
            bass,
            other,
        }) = decode_stem_entry(library_root, &cached)?
        else {
            unreachable!("individual stem cache entries decode to four stems");
        };
        Ok(PlaybackSourceLoad {
            decoded_audio: vocals.clone(),
            stems: Some(LoadedStems::FourStem(StemSet {
                vocals,
                drums,
                bass,
                other,
            })),
        })
    } else {
        let LoadedStems::TwoStem {
            vocals,
            accompaniment,
        } = decode_stem_entry(library_root, &cached)?
        else {
            unreachable!("two stem cache entries decode to two stems");
        };
        Ok(PlaybackSourceLoad {
            decoded_audio: accompaniment.clone(),
            stems: Some(LoadedStems::TwoStem {
                vocals,
                accompaniment,
            }),
        })
    }
}

fn decode_stem_entry(
    library_root: &LibraryRoot,
    cached: &cache::stems::StemCacheEntry,
) -> Result<LoadedStems> {
    let load_stem = |path: &str| -> Result<decode::DecodedAudio> {
        let abs = library_root.resolve(path);
        decode::decode_file(&abs)
            .map_err(|e| anyhow::anyhow!("failed to decode stem {}: {}", path, e))
    };

    if cached.has_individual_stems() {
        Ok(LoadedStems::FourStem(StemSet {
            vocals: load_stem(&cached.vocals_path)?,
            drums: load_stem(
                cached
                    .drums_path
                    .as_deref()
                    .context("missing drums stem path")?,
            )?,
            bass: load_stem(
                cached
                    .bass_path
                    .as_deref()
                    .context("missing bass stem path")?,
            )?,
            other: load_stem(
                cached
                    .other_path
                    .as_deref()
                    .context("missing other stem path")?,
            )?,
        }))
    } else {
        Ok(LoadedStems::TwoStem {
            vocals: load_stem(&cached.vocals_path)?,
            accompaniment: load_stem(&cached.accomp_path)?,
        })
    }
}

fn map_cache_error(error: CacheError) -> PlaybackError {
    PlaybackError::Internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::chunked_cache::CacheManager;
    use tempfile::tempdir;

    #[test]
    fn remote_cache_manager_evicts_lru_when_over_budget() {
        let dir = tempdir().expect("temp dir");
        let mut manager = CacheManager::new(dir.path().to_path_buf(), 200);

        let c1 = manager.get_or_create("song-a", 150).expect("cache a");
        c1.write_at(0, &[0u8; 150]).expect("write a");

        // Opening a second 150-byte cache should evict song-a (LRU).
        let c2 = manager.get_or_create("song-b", 150).expect("cache b");
        c2.write_at(0, &[0u8; 150]).expect("write b");

        assert_eq!(manager.len(), 1);
        assert!(dir.path().join("song-b.cache").exists());
        assert!(!dir.path().join("song-a.cache").exists());
    }
}
