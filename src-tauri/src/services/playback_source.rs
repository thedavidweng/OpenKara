use crate::{
    audio::{
        decode,
        error::PlaybackError,
        playback::{LoadedStems, StemSet},
        remote_source::{self, FetchEvent, RemoteMediaSource},
        streaming::{self, StreamMetadata, StreamingTrack},
    },
    cache,
    library::Song,
    library_root::LibraryRoot,
    media_g::{self, MEDIA_G_ZIP},
    remote,
    remote::cache_catalog::{CacheCatalog, CacheIdentity, CachePinGuard},
    remote::provider::RemoteProvider,
};
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::{
    fs,
    path::{Path, PathBuf},
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
        // Download missing stem files before decoding. The streaming path
        // (load_playback_source_streaming) returns Ok(None) for remote stems,
        // so this fallback path is the one that actually caches them. Without
        // this call, a cold-cache remote-stems song would reach
        // load_remote_stems_playback_source with no files on disk and fail
        // to decode. request_id=0 is safe here because the fallback path is
        // not guarded by the streaming stale-guard.
        ensure_remote_stem_files_cached(app_data_dir, library_root, connection, song, 0)
            .map_err(|e| PlaybackError::Internal(e.to_string()))?;
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
    request_id: u64,
) -> Result<LoadedStems, PlaybackError> {
    if song.is_remote_stems() {
        ensure_remote_stem_files_cached(app_data_dir, library_root, connection, song, request_id)
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

pub(crate) struct StreamingStemsSource {
    pub(crate) streaming_track: StreamingTrack,
    pub(crate) decode_handles: Vec<std::thread::JoinHandle<Result<(), decode::DecodeError>>>,
}

/// Returns `None` for remote stems (which need caching first) or Media+G
/// containers.
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
        decode_handles: result.decode_handles,
    }))
}

pub(crate) struct StreamingPlaybackSource {
    pub(crate) streaming_track: StreamingTrack,
    pub(crate) metadata: StreamMetadata,
    pub(crate) decode_handle: std::thread::JoinHandle<Result<(), decode::DecodeError>>,
    /// Receiver for fetch events (only present for remote streaming).
    /// The caller should consume these to handle ConsecutiveFailures, etc.
    pub(crate) fetch_event_rx: Option<mpsc::Receiver<FetchEvent>>,
    /// RAII pin guard for the remote cache entry. When dropped (on source
    /// release / track skip / stop), the pin count decrements so the entry
    /// becomes eligible for eviction. `None` for local sources. The field is
    /// never read directly — it exists only for its `Drop` side effect.
    pub(crate) cache_pin_guard: Option<CachePinGuard>,
}

/// For local files, decodes directly from disk. For remote songs, creates a
/// `RemoteMediaSource` that fetches byte ranges on demand via HTTP Range
/// requests, enabling edge-downloaded playback without pre-downloading the
/// entire file.
///
/// Falls back to full decode for Media+G containers (which require in-memory
/// byte extraction).
pub(crate) fn load_playback_source_streaming(
    app_data_dir: Option<&Path>,
    remote_chunk_cache: &Arc<Mutex<CacheCatalog>>,
    library_root: &LibraryRoot,
    song: &Song,
) -> Result<Option<StreamingPlaybackSource>, PlaybackError> {
    // Media+G containers require in-memory extraction — can't stream from disk.
    if song.media_g_container.as_deref() == Some(MEDIA_G_ZIP) {
        return Ok(None);
    }

    // Remote stems must not enter the single-file remote streaming branch.
    // `update_remote_song(..., "stems_remote")` clears `song.file_path`, so
    // `resolve_song_file_path()` would fail inside `load_remote_streaming_source`
    // before the stem-caching path is ever reached.  Returning `Ok(None)` makes
    // the caller fall back to the non-streaming `load_playback_source` path,
    // which downloads missing stems via `ensure_remote_stem_files_cached` and
    // then decodes them via `load_remote_stems_playback_source`.  Remote stems
    // use local file streaming after complete caching — they do NOT use
    // network-backed per-stem readers in this PR.
    if song.is_remote_stems() {
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
        cache_pin_guard: None,
    }))
}

/// Returns `Ok(None)` if the provider doesn't support Range
/// requests (caller should fall back to full-file download).
pub(crate) fn load_remote_streaming_source(
    app_data_dir: Option<&Path>,
    remote_chunk_cache: &Arc<Mutex<CacheCatalog>>,
    _library_root: &LibraryRoot,
    song: &Song,
) -> Result<Option<StreamingPlaybackSource>, PlaybackError> {
    let Some(app_data_dir) = app_data_dir else {
        return Ok(None);
    };

    let song_path =
        resolve_song_file_path(song).map_err(|e| PlaybackError::Internal(e.to_string()))?;

    let library = remote::active_remote_library(app_data_dir)
        .map_err(|e| PlaybackError::Internal(e.message.clone()))?;
    let Some(library) = library else {
        return Ok(None);
    };
    let provider = remote::provider::create_provider(app_data_dir, &library)
        .map_err(|e| PlaybackError::Internal(e.message.clone()))?;

    let fetcher = match provider.create_range_fetcher(song_path) {
        Ok(Some(f)) => f,
        Ok(None) => return Ok(None), // Provider doesn't support Range — fall back.
        Err(_) => return Ok(None),   // Can't create fetcher — fall back.
    };

    let file_size = provider
        .get_file_size(song_path)
        .map_err(|e| PlaybackError::Internal(e.message.clone()))?
        .unwrap_or(0);
    if file_size == 0 {
        return Ok(None); // Can't determine size — fall back.
    }

    // Build a revision-aware cache identity so a replaced remote object (new
    // provider revision or changed size) does not reuse bytes from an older
    // version. The cache key is the SHA-256 of (library_id, relative_path,
    // provider_revision, expected_size). When the provider does not expose a
    // revision token, fall back to the library's stored remote_revision; if
    // that is also unavailable, the content-digest fallback path in the
    // catalog computes a SHA-256 of the cached file after the first full
    // download and uses that for future lookups.
    let provider_revision = provider.get_revision(song_path).ok().flatten();
    let revision = provider_revision.or_else(|| library.remote_revision().map(str::to_owned));
    let identity = CacheIdentity {
        library_id: library.id().to_owned(),
        relative_path: song_path.to_owned(),
        provider_revision: revision,
        expected_size: file_size,
    };
    let cache_key = identity.cache_key();

    let cache = {
        let mut manager = remote_chunk_cache.lock().map_err(|_| {
            PlaybackError::Internal("remote chunk cache manager lock was poisoned".to_owned())
        })?;
        manager.get_or_create(&identity).map_err(map_cache_error)?
    };

    // Pin the cache entry so eviction cannot remove the file while playback is
    // active. The guard decrements the pin count on drop (source release /
    // track skip / stop), making the entry eligible for eviction again.
    let cache_pin_guard = {
        Some(
            CacheCatalog::pin_cache_entry(remote_chunk_cache, &cache_key)
                .map_err(map_cache_error)?,
        )
    };

    // Create a persistence callback so the fetch thread can persist
    // download progress to the cache catalog after each successful range
    // write. Without this, downloaded_ranges_json stays empty and complete
    // stays false, so cached remote audio is re-downloaded after restart.
    let persist_catalog = Arc::clone(remote_chunk_cache);
    let persist_key = cache_key.clone();
    let on_range_written: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        if let Ok(manager) = persist_catalog.lock() {
            let _ = manager.persist_ranges(&persist_key);
        }
    });

    let (fetch_tx, fetch_event_rx, _bandwidth_monitor, _fetch_handle) =
        remote_source::spawn_fetch_thread_with_fetcher(
            String::new(), // URL is embedded in the fetcher
            Arc::clone(&cache),
            fetcher,
            remote_source::RetryConfig::default(),
            Some(on_range_written),
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

    // Spawn the decode producer from the remote source. Slow networks should
    // surface as buffering/retry behavior, not PCM frame dropping, because
    // changing decoded samples audibly degrades karaoke playback quality.
    let (consumer, decode_handle) = streaming::spawn_decode_producer_from_source(
        Box::new(decode_source),
        extension.as_deref(),
        &probe_metadata,
        streaming::ProxyConfig::none(),
    )
    .map_err(|e| PlaybackError::AudioDecodeFailed(e.to_string()))?;

    Ok(Some(StreamingPlaybackSource {
        streaming_track: StreamingTrack::Single { consumer },
        metadata: probe_metadata,
        decode_handle,
        fetch_event_rx: Some(fetch_event_rx),
        cache_pin_guard,
    }))
}

/// Consumes the source (symphonia takes ownership of the `MediaSourceStream`).
fn probe_remote_source(
    source: RemoteMediaSource,
    extension: Option<&str>,
) -> Result<StreamMetadata, decode::DecodeError> {
    use symphonia::core::{
        formats::{probe::Hint, FormatOptions, TrackType},
        io::MediaSourceStream,
        meta::MetadataOptions,
        units::Timestamp,
    };

    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }

    let mss = MediaSourceStream::new(Box::new(source), Default::default());
    let mut probed = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| decode::DecodeError::ProbeFailed(format!("remote source: {e}")))?;

    let (codec_params, track_id, n_frames, time_base) = {
        let track = probed
            .default_track(TrackType::Audio)
            .ok_or(decode::DecodeError::NoDefaultTrack)?;
        let audio_params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or(decode::DecodeError::NoDefaultTrack)?
            .clone();
        (audio_params, track.id, track.num_frames, track.time_base)
    };

    let mut sample_rate = codec_params.sample_rate;
    let mut channels = codec_params.channels.as_ref().map(|c| c.count());

    // Some containers don't expose sample rate / channel layout in the
    // codec params.  symphonia only populates these after decoding the
    // first packet, so try that before giving up.
    if sample_rate.is_none() || channels.is_none() {
        use symphonia::core::codecs::audio::AudioDecoderOptions as DO;
        if let Ok(mut decoder) =
            symphonia::default::get_codecs().make_audio_decoder(&codec_params, &DO::default())
        {
            while let Ok(Some(packet)) = probed.next_packet() {
                if packet.track_id != track_id {
                    continue;
                }
                if let Ok(decoded) = decoder.decode(&packet) {
                    let spec = decoded.spec();
                    sample_rate.get_or_insert(spec.rate());
                    channels.get_or_insert(spec.channels().count());
                    break;
                }
            }
        }
    }

    let sample_rate = sample_rate.ok_or(decode::DecodeError::MissingSampleRate)?;
    let channels = channels.ok_or(decode::DecodeError::MissingChannels)?;

    // Try to get duration from container metadata.
    // Return None when unavailable so playback can start immediately.
    let duration_ms = if let (Some(n_frames), Some(tb)) = (n_frames, time_base) {
        let time = tb.calc_time(Timestamp::new(n_frames as i64));
        time.map(|t| t.as_millis() as u64)
    } else {
        None
    };

    Ok(StreamMetadata {
        sample_rate_hz: sample_rate,
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
        remote::ensure_remote_file_cached(app_data_dir, file_path)
            .map_err(|error| anyhow::anyhow!(error.message.clone()))?;
    }
    if let Some(cdg_path) = song.cdg_path.as_deref() {
        remote::ensure_remote_file_cached(app_data_dir, cdg_path)
            .map_err(|error| anyhow::anyhow!(error.message.clone()))?;
    }
    Ok(())
}

pub(crate) fn ensure_remote_stem_files_cached(
    app_data_dir: Option<&Path>,
    library_root: &LibraryRoot,
    connection: &Connection,
    song: &Song,
    request_id: u64,
) -> Result<()> {
    let Some(app_data_dir) = app_data_dir else {
        return Ok(());
    };

    let Some(library) = remote::active_remote_library(app_data_dir)
        .map_err(|error| anyhow::anyhow!(error.message.clone()))?
    else {
        return Ok(());
    };
    let provider = remote::provider::create_provider(app_data_dir, &library)
        .map_err(|error| anyhow::anyhow!(error.message.clone()))?;

    ensure_remote_stem_set_cached(
        provider.as_ref(),
        library_root,
        connection,
        song,
        request_id,
    )
}

/// Guarded variant of [`ensure_remote_stem_files_cached`] for the async
/// playback path (PR #7, issue #151 defect #11). Threads a stale-guard
/// closure through to [`ensure_remote_stem_set_cached_guarded`] so a skip
/// cancels remaining stem downloads and aborts the atomic rename. Returns a
/// typed `RemoteError` so the caller can no-op on `StaleRequest`.
pub(crate) fn ensure_remote_stem_files_cached_guarded(
    app_data_dir: Option<&Path>,
    library_root: &LibraryRoot,
    connection: &Connection,
    song: &Song,
    request_id: u64,
    is_current: impl Fn() -> bool,
) -> std::result::Result<(), remote::errors::RemoteError> {
    let Some(app_data_dir) = app_data_dir else {
        return Ok(());
    };

    let Some(library) = remote::active_remote_library(app_data_dir).map_err(|error| {
        remote::errors::RemoteError::new(
            remote::errors::RemoteErrorKind::NetworkUnavailable,
            error.message.clone(),
        )
    })?
    else {
        return Ok(());
    };
    let provider = remote::provider::create_provider(app_data_dir, &library).map_err(|error| {
        remote::errors::RemoteError::new(
            remote::errors::RemoteErrorKind::NetworkUnavailable,
            error.message.clone(),
        )
    })?;

    ensure_remote_stem_set_cached_guarded(
        provider.as_ref(),
        library_root,
        connection,
        song,
        request_id,
        is_current,
    )
}

/// A single required stem within a remote stem set, with its relative path
/// (as stored in the cache database) and a human-readable label for errors.
#[derive(Debug, Clone)]
struct RequiredStem {
    label: &'static str,
    relative_path: String,
}

/// Returns the list of stems that must be present for the given cache entry.
///
/// Two-stem sets require vocals + accompaniment; four-stem sets require
/// vocals + drums + bass + other.  The `accomp_path` column is always
/// non-null in the schema but is empty for four-stem entries, so it is
/// only included for two-stem sets.
fn required_stems(entry: &cache::stems::StemCacheEntry) -> Vec<RequiredStem> {
    if entry.has_individual_stems() {
        vec![
            RequiredStem {
                label: "vocals",
                relative_path: entry.vocals_path.clone(),
            },
            RequiredStem {
                label: "drums",
                relative_path: entry.drums_path.clone().unwrap_or_default(),
            },
            RequiredStem {
                label: "bass",
                relative_path: entry.bass_path.clone().unwrap_or_default(),
            },
            RequiredStem {
                label: "other",
                relative_path: entry.other_path.clone().unwrap_or_default(),
            },
        ]
    } else {
        vec![
            RequiredStem {
                label: "vocals",
                relative_path: entry.vocals_path.clone(),
            },
            RequiredStem {
                label: "accompaniment",
                relative_path: entry.accomp_path.clone(),
            },
        ]
    }
}

/// Downloaded-and-verified stem, waiting to be atomically installed.
struct VerifiedStem {
    /// Absolute path of the temp file that passed validation.
    temp_path: PathBuf,
    /// Absolute path of the final destination.
    final_path: PathBuf,
}

/// Decoded audio metadata used for cross-stem alignment validation.
struct StemSetMetadata {
    sample_rate_hz: u32,
    channels: usize,
    /// Actual PCM frame count (samples.len() / channels) from a full decode.
    /// This is more reliable than container-reported `num_frames`, which can
    /// be wrong for truncated files.
    frame_count: usize,
}

/// Decode a file and extract the metadata needed for set-level alignment
/// validation.  A full decode is used (rather than just a probe) because it
/// catches truncated files that have a valid header but incomplete data —
/// a probe would report the header's frame count while the actual decoded
/// frame count would be lower.
///
/// PR #3 will route this through a shared atomic-download helper that can
/// cache the decoded audio to avoid re-decoding during playback.
// TODO(PR#3): route through shared atomic download helper.
// NOTE: PR#3's `atomic_download` helper (src-tauri/src/remote/atomic_download.rs)
// downloads to a temp file, validates, and atomically renames to the final
// destination in one shot. The stem-set path cannot use it directly because
// PR#1's all-or-nothing semantics require every stem to pass the cross-set
// alignment check (Phase 3) BEFORE any final-path file is touched. Using
// `atomic_download` per-stem would rename each stem to its final path
// immediately, reintroducing the partial-set problem PR#1 fixed (e.g. vocals
// installed but accompaniment truncated). The shared helper is instead used by
// `ensure_remote_file_cached` and `atomic_database_pull`, which have no
// all-or-nothing constraint. A future refactor could split the helper into
// "download+validate to temp" and "commit temp to final" steps so the stem
// path can reuse the download half while keeping its delayed collective
// rename; that split is deferred to avoid churning the helper API in PR#3.
fn decode_stem_metadata(path: &Path) -> Result<StemSetMetadata> {
    let audio = decode::decode_file(path)
        .with_context(|| format!("failed to decode stem at {}", path.display()))?;
    let frame_count = audio.samples.len() / audio.channels.max(1);
    Ok(StemSetMetadata {
        sample_rate_hz: audio.sample_rate_hz,
        channels: audio.channels,
        frame_count,
    })
}

/// All-or-nothing download + validation + atomic-rename for a remote stem set.
///
/// This orchestrator replaces the old per-stem `ensure_remote_file_cached`
/// loop, which downloaded each stem independently to its final path with no
/// validation and no set-level consistency check.  The old approach could
/// leave a partially downloaded set on disk — e.g. vocals present but
/// accompaniment truncated — which then produced silent or glitchy playback.
///
/// ## Semantics
///
/// * **All-or-nothing**: every required stem must download and validate
///   successfully before any final-path file is touched.  If any stem fails
///   (missing, truncated, corrupt, mismatched metadata), the entire set is
///   rejected and existing final paths are left untouched.  Temp files are
///   cleaned up.
///
/// * **In-process retention**: a stem whose final path already exists and
///   decodes successfully is kept as-is — it is not re-downloaded.  This
///   means a retry after a transient failure only re-downloads the stems that
///   were missing or invalid.  Durable restart-survival (persisting which
///   stems are verified across app restarts) is PR #2/#3's job; this function
///   only retains verified stems within the same process lifetime.
///   // TODO(PR#6): route through durable verified-stem catalog (remote_cache_entries)
///   // so restart-survival works. PR#3's atomic_download helper is not used
///   // here because of the all-or-nothing set constraint (see note above).
///
/// * **Sample alignment**: every stem in the set must share the same sample
///   rate, channel count, and PCM frame count.  Mismatched stems would
///   produce phase artifacts or silent channels during playback, so the set
///   is rejected as a unit when any stem deviates.  A full decode is used
///   for validation (not just a probe) because truncated files can have a
///   valid header but incomplete data — only a full decode reveals the
///   actual PCM frame count.
///
/// * **Stale-guard**: `request_id` is an epoch counter that identifies the
///   playback request that initiated this download.  The guarded variant
///   [`ensure_remote_stem_set_cached_guarded`] accepts a `is_current` closure
///   that the orchestrator checks before each stem download and before the
///   atomic-rename phase, so a skip cancels remaining work promptly and a
///   late completion never installs a stem set for a song the user has
///   already moved past.  The synchronous [`ensure_remote_stem_set_cached`]
///   passes a guard that always returns `true` — the song cannot change
///   mid-call — but the `request_id` parameter is threaded through so the
///   async transition was a drop-in change.
fn ensure_remote_stem_set_cached(
    provider: &dyn RemoteProvider,
    library_root: &LibraryRoot,
    connection: &Connection,
    song: &Song,
    request_id: u64,
) -> Result<()> {
    ensure_remote_stem_set_cached_inner(
        provider,
        library_root,
        connection,
        song,
        request_id,
        &|| true,
    )
}

/// Async-capable variant of [`ensure_remote_stem_set_cached`] with a
/// stale-guard closure (PR #7, issue #151 defect #11).
///
/// `is_current` returns `true` while `request_id` still identifies the active
/// playback request. The orchestrator checks it:
///
/// * before each stem download (between stems) so a skip cancels remaining
///   work promptly, and
/// * before the atomic-rename phase so a late completion does not install a
///   stem set for a song the user has already skipped past.
///
/// When the guard reports the request as stale, the orchestrator discards all
/// temp files and returns a typed [`remote::errors::RemoteError`] with
/// [`RemoteErrorKind::StaleRequest`] so the caller can no-op. The function is
/// synchronous in its body (the provider's `download_file` is blocking), but
/// it is intended to be called from a dedicated background thread so the
/// guard can observe a request_id change made on another thread between
/// stems.
pub(crate) fn ensure_remote_stem_set_cached_guarded(
    provider: &dyn RemoteProvider,
    library_root: &LibraryRoot,
    connection: &Connection,
    song: &Song,
    request_id: u64,
    is_current: impl Fn() -> bool,
) -> std::result::Result<(), remote::errors::RemoteError> {
    ensure_remote_stem_set_cached_inner(
        provider,
        library_root,
        connection,
        song,
        request_id,
        &is_current,
    )
    .map_err(|error| {
        // Distinguish a stale-guard abort from any other failure so the
        // caller can no-op instead of surfacing a user-visible error.
        if error.to_string().contains(STALE_GUARD_MARKER) {
            remote::errors::RemoteError::from_kind(remote::errors::RemoteErrorKind::StaleRequest)
        } else {
            remote::errors::RemoteError::new(
                remote::errors::RemoteErrorKind::NetworkUnavailable,
                error.to_string(),
            )
        }
    })
}

/// Marker embedded in the error message when the stale-guard aborts the
/// orchestrator. Used by [`ensure_remote_stem_set_cached_guarded`] to map the
/// abort back to a typed `StaleRequest` error without threading a custom
/// error type through the shared inner body.
const STALE_GUARD_MARKER: &str = "__stale_request_guard_aborted__";

fn ensure_remote_stem_set_cached_inner(
    provider: &dyn RemoteProvider,
    library_root: &LibraryRoot,
    connection: &Connection,
    song: &Song,
    request_id: u64,
    is_current: &dyn Fn() -> bool,
) -> Result<()> {
    let Some(cached) = cache::stems::get_cached_stem_entry(connection, &song.hash)
        .context("failed to load cached stems")?
    else {
        // No stem cache entry — nothing to download.  The caller will surface
        // a "karaoke not ready" error downstream.
        return Ok(());
    };

    let required = required_stems(&cached);

    // Phase 1: determine which stems are already verified (final path exists
    // and decodes successfully) and which need downloading.
    //
    // A full decode is used for the retention check (not just a probe) so
    // that a truncated or corrupt existing file is detected and re-downloaded
    // rather than silently kept.
    let mut verified: Vec<VerifiedStem> = Vec::new();
    let mut to_download: Vec<RequiredStem> = Vec::new();
    let mut reference_metadata: Option<StemSetMetadata> = None;

    for stem in &required {
        let final_path = library_root.resolve(&stem.relative_path);

        if final_path.exists() {
            match decode_stem_metadata(&final_path) {
                Ok(metadata) => {
                    // Retain already-verified stems — they survive retry and
                    // are not re-downloaded.
                    if reference_metadata.is_none() {
                        reference_metadata = Some(metadata);
                    }
                    continue;
                }
                Err(_) => {
                    // Existing file is corrupt or undecodeable — re-download.
                }
            }
        }
        to_download.push(stem.clone());
    }

    // Phase 2: download each missing stem to a unique temp path, then decode
    // to validate content identity (fully decodable audio with non-zero
    // length).
    //
    // Temp paths are unique per request so concurrent downloads for different
    // playback requests do not collide: `<dest>.part.<stem>.<request_id>`.
    for stem in &to_download {
        // Stale-guard (PR #7, defect #11): check before each stem download so
        // a skip cancels remaining work promptly. When the active request has
        // moved on, discard any temps already collected and abort with the
        // stale-guard marker so the guarded wrapper maps it to
        // `RemoteErrorKind::StaleRequest`.
        if !is_current() {
            clean_up_temps(&verified);
            anyhow::bail!("{STALE_GUARD_MARKER}");
        }

        let final_path = library_root.resolve(&stem.relative_path);
        let temp_path = final_path.with_extension(format!("part.{}.{}", stem.label, request_id));

        // Ensure the parent directory exists (the stem directory may not yet
        // be present on a fresh working copy).
        if let Some(parent) = temp_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create stem directory {}", parent.display()))?;
        }

        // Clean up any stale temp file from a previous attempt.
        let _ = fs::remove_file(&temp_path);

        provider
            .download_file(&stem.relative_path, &temp_path)
            .map_err(|error| {
                clean_up_temps(&verified);
                anyhow::anyhow!("failed to download stem {}: {}", stem.label, error.message)
            })?;

        // Validate file size: a zero-byte download is permanently invalid.
        let size = fs::metadata(&temp_path).map(|m| m.len()).unwrap_or(0);
        if size == 0 {
            let _ = fs::remove_file(&temp_path);
            clean_up_temps(&verified);
            anyhow::bail!(
                "stem {} downloaded as a zero-byte file — permanently invalid",
                stem.label
            );
        }

        // Full decode to verify content identity.  This catches truncated
        // files (valid header but incomplete data) and corrupt files that
        // cannot be decoded at all.
        match decode_stem_metadata(&temp_path) {
            Ok(metadata) => {
                verified.push(VerifiedStem {
                    temp_path,
                    final_path,
                });
                if reference_metadata.is_none() {
                    reference_metadata = Some(metadata);
                }
            }
            Err(error) => {
                let _ = fs::remove_file(&temp_path);
                clean_up_temps(&verified);
                anyhow::bail!("stem {} failed validation decode: {}", stem.label, error);
            }
        }
    }

    // Phase 3: cross-stem sample alignment check.
    //
    // Every stem in the set must share the same sample rate, channel count,
    // and PCM frame count.  We decode each temp file and each already-verified
    // final file and compare against the reference.  A mismatch means the
    // set is inconsistent and must be rejected as a unit.
    let reference = reference_metadata
        .ok_or_else(|| anyhow::anyhow!("no stems to validate for song {}", song.hash))?;

    for stem in &required {
        let final_path = library_root.resolve(&stem.relative_path);
        let path_to_check = verified
            .iter()
            .find(|v| v.final_path == final_path)
            .map(|v| &v.temp_path)
            .unwrap_or(&final_path);

        if !path_to_check.exists() {
            clean_up_temps(&verified);
            anyhow::bail!(
                "stem {} is missing after download phase — set incomplete",
                stem.label
            );
        }

        let metadata = decode_stem_metadata(path_to_check)
            .with_context(|| format!("failed to decode stem {} for alignment check", stem.label))?;

        if metadata.sample_rate_hz != reference.sample_rate_hz {
            clean_up_temps(&verified);
            anyhow::bail!(
                "stem {} sample rate {} does not match set reference {}",
                stem.label,
                metadata.sample_rate_hz,
                reference.sample_rate_hz
            );
        }
        if metadata.channels != reference.channels {
            clean_up_temps(&verified);
            anyhow::bail!(
                "stem {} channel count {} does not match set reference {}",
                stem.label,
                metadata.channels,
                reference.channels
            );
        }
        if metadata.frame_count != reference.frame_count {
            clean_up_temps(&verified);
            anyhow::bail!(
                "stem {} PCM frame count {} does not match set reference {}",
                stem.label,
                metadata.frame_count,
                reference.frame_count
            );
        }
    }

    // Phase 4: atomic rename — install every verified temp file over its
    // final path only after ALL stems pass validation.
    //
    // `fs::rename` is atomic on POSIX (same filesystem) and overwrites the
    // destination.  On Windows, `rename` also replaces the destination when
    // both are on the same volume.  Temp files are siblings of the final
    // path so they are always on the same filesystem.
    //
    // Stale-guard (PR #7, defect #11): re-check immediately before the
    // atomic-rename phase. Even if every stem downloaded successfully, a
    // skip that arrived during the (relatively cheap) Phase 3 alignment
    // check must prevent the rename from installing a stem set for a song
    // the user has already moved past.
    if !is_current() {
        clean_up_temps(&verified);
        anyhow::bail!("{STALE_GUARD_MARKER}");
    }
    for v in &verified {
        fs::rename(&v.temp_path, &v.final_path).with_context(|| {
            format!(
                "failed to atomically install stem {} -> {}",
                v.temp_path.display(),
                v.final_path.display()
            )
        })?;
    }

    Ok(())
}

/// Remove all temp files from a list of verified stems (used on validation
/// failure to avoid leaving partial downloads on disk).
fn clean_up_temps(verified: &[VerifiedStem]) {
    for v in verified {
        let _ = fs::remove_file(&v.temp_path);
    }
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

fn map_cache_error(error: crate::commands::error::CommandError) -> PlaybackError {
    PlaybackError::Internal(error.message)
}

#[cfg(test)]
mod tests {
    use crate::cache::stems::StemCacheEntry;
    use crate::commands::error::{CommandError, CommandResult};
    use crate::library::Song;
    use crate::library_root::LibraryRoot;
    use crate::remote::cache_catalog::{CacheCatalog, CacheIdentity, DEFAULT_CACHE_BYTES_LIMIT};
    use crate::remote::control_db::open_control_db;
    use crate::remote::provider::RemoteProvider;
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    #[test]
    fn remote_cache_manager_evicts_lru_when_over_budget() {
        let db_dir = tempdir().expect("db temp dir");
        let cache_dir = tempdir().expect("cache temp dir");
        let conn = open_control_db(&db_dir.path().join("remote-state.db")).expect("open db");
        let control_db = Arc::new(Mutex::new(conn));
        let mut manager = CacheCatalog::open(cache_dir.path().to_path_buf(), control_db, 200)
            .expect("open catalog");

        let id_a = CacheIdentity {
            library_id: "lib-1".to_owned(),
            relative_path: "media/a.mp3".to_owned(),
            provider_revision: Some("rev-1".to_owned()),
            expected_size: 150,
        };
        let id_b = CacheIdentity {
            library_id: "lib-1".to_owned(),
            relative_path: "media/b.mp3".to_owned(),
            provider_revision: Some("rev-1".to_owned()),
            expected_size: 150,
        };

        let c1 = manager.get_or_create(&id_a).expect("cache a");
        c1.write_at(0, &[0u8; 150]).expect("write a");
        manager
            .persist_ranges(&id_a.cache_key())
            .expect("persist a");

        let c2 = manager.get_or_create(&id_b).expect("cache b");
        c2.write_at(0, &[0u8; 150]).expect("write b");
        manager
            .persist_ranges(&id_b.cache_key())
            .expect("persist b");

        // A (oldest) should be evicted; B remains.
        assert!(
            manager.get_entry(&id_a.cache_key()).unwrap().is_none(),
            "oldest entry must be evicted"
        );
        assert!(manager.get_entry(&id_b.cache_key()).unwrap().is_some());
    }

    // ---- Test infrastructure for remote stem set caching ----

    /// In-memory fake provider that serves files from a `HashMap`.
    ///
    /// Implements only the `download_file` and `get_file_size` methods that
    /// `ensure_remote_stem_set_cached` needs.  All other trait methods return
    /// empty/Ok defaults — this is test-only and never used in production.
    struct FakeRemoteProvider {
        files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    }

    impl FakeRemoteProvider {
        fn with_files(files: HashMap<String, Vec<u8>>) -> Self {
            Self {
                files: Arc::new(Mutex::new(files)),
            }
        }
    }

    impl RemoteProvider for FakeRemoteProvider {
        fn get_revision(&self, _relative_path: &str) -> CommandResult<Option<String>> {
            Ok(None)
        }

        fn download_file(&self, relative_path: &str, destination: &Path) -> CommandResult<()> {
            let files = self.files.lock().unwrap();
            let data = files.get(relative_path).cloned().ok_or_else(|| {
                CommandError::from(crate::library::error::LibraryError::Internal(format!(
                    "fake provider: file {relative_path} not found"
                )))
            })?;
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(destination, &data).map_err(|e| {
                CommandError::from(crate::library::error::LibraryError::Internal(format!(
                    "fake provider: failed to write {}: {e}",
                    destination.display()
                )))
            })?;
            Ok(())
        }

        fn upload_file(&self, _relative_path: &str) -> CommandResult<()> {
            Ok(())
        }

        fn delete_path(&self, _relative_path: &str) -> CommandResult<()> {
            Ok(())
        }

        fn initialize_or_sync(&self) -> CommandResult<Option<String>> {
            Ok(None)
        }

        fn get_file_size(&self, relative_path: &str) -> CommandResult<Option<u64>> {
            Ok(self
                .files
                .lock()
                .unwrap()
                .get(relative_path)
                .map(|d| d.len() as u64))
        }

        fn refresh_existing(&self) -> CommandResult<Option<String>> {
            Ok(None)
        }
    }

    /// Generate a minimal valid WAV file with the given sample rate, channel
    /// count, and number of PCM frames.  Each sample is a deterministic
    /// non-zero value so symphonia can probe and decode it.
    fn make_wav(sample_rate: u32, channels: u16, frames: u32) -> Vec<u8> {
        let bits_per_sample: u16 = 16;
        let bytes_per_sample = (bits_per_sample / 8) as u32;
        let data_size = frames * channels as u32 * bytes_per_sample;
        let file_size = 36 + data_size;

        let mut buf = Vec::with_capacity(44 + data_size as usize);

        // RIFF header
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&file_size.to_le_bytes());
        buf.extend_from_slice(b"WAVE");

        // fmt chunk
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes()); // chunk size
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
        buf.extend_from_slice(&channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        let byte_rate = sample_rate * channels as u32 * bytes_per_sample;
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        let block_align = channels * bytes_per_sample as u16;
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());

        // data chunk
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_size.to_le_bytes());

        // PCM samples — deterministic pattern, non-zero
        for i in 0..(frames * channels as u32) {
            let sample: i16 = ((i % 8000) as i16) - 4000;
            buf.extend_from_slice(&sample.to_le_bytes());
        }

        buf
    }

    /// Create a test library root with an initialized database.
    fn test_library_root() -> (tempfile::TempDir, LibraryRoot) {
        let dir = tempdir().expect("temp dir");
        let lib = LibraryRoot::create(&dir.path().join("Lib")).expect("create library");
        crate::cache::initialize_library_database(&lib.database_path()).expect("init database");
        (dir, lib)
    }

    /// Insert a stem cache entry into the database for the given song hash.
    /// Also inserts a minimal song row to satisfy the foreign key constraint.
    fn insert_stem_entry(connection: &rusqlite::Connection, entry: &StemCacheEntry) {
        let song = remote_stems_song(&entry.song_hash);
        crate::cache::upsert_song(connection, &song).expect("test song upsert should succeed");
        crate::cache::stems::upsert_stem_entry_test(connection, entry);
    }

    /// Build a `Song` with `audio_source_kind = "stems_remote"` and no
    /// `file_path` (mirroring what `update_remote_song` produces).
    fn remote_stems_song(hash: &str) -> Song {
        Song {
            hash: hash.to_owned(),
            file_path: None,
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: "stems_remote".to_owned(),
            title: Some("Test".to_owned()),
            artist: None,
            album: None,
            duration_ms: 1000,
            cover_art: None,
            has_cover_art: false,
            artwork_thumb_path: None,
            imported_at: 1,
            original_ext: Some("wav".to_owned()),
        }
    }

    fn two_stem_entry(hash: &str) -> StemCacheEntry {
        StemCacheEntry {
            song_hash: hash.to_owned(),
            vocals_path: format!("stems/{hash}/vocals.wav"),
            accomp_path: format!("stems/{hash}/accompaniment.wav"),
            separated_at: 1,
            drums_path: None,
            bass_path: None,
            other_path: None,
            model_variant: "test".to_owned(),
        }
    }

    fn four_stem_entry(hash: &str) -> StemCacheEntry {
        StemCacheEntry {
            song_hash: hash.to_owned(),
            vocals_path: format!("stems/{hash}/vocals.wav"),
            accomp_path: String::new(),
            separated_at: 1,
            drums_path: Some(format!("stems/{hash}/drums.wav")),
            bass_path: Some(format!("stems/{hash}/bass.wav")),
            other_path: Some(format!("stems/{hash}/other.wav")),
            model_variant: "test".to_owned(),
        }
    }

    // ---- Test cases ----

    #[test]
    fn stems_remote_bypasses_resolve_song_file_path_in_streaming() {
        // A stems_remote song has file_path = None.  The streaming path must
        // return Ok(None) WITHOUT calling resolve_song_file_path, which would
        // fail.  Ok(None) makes the caller fall back to the non-streaming
        // load_playback_source that handles remote stems.
        let db_dir = tempdir().expect("db temp dir");
        let cache_dir = tempdir().expect("cache temp dir");
        let lib = LibraryRoot::create(&cache_dir.path().join("Lib")).expect("library");
        let song = remote_stems_song("song-a");
        let conn = open_control_db(&db_dir.path().join("remote-state.db")).expect("open db");
        let control_db = Arc::new(Mutex::new(conn));
        let catalog = CacheCatalog::open(
            db_dir.path().join("cache"),
            control_db,
            DEFAULT_CACHE_BYTES_LIMIT,
        )
        .expect("open catalog");
        let cache = Arc::new(Mutex::new(catalog));

        let result =
            super::load_playback_source_streaming(Some(cache_dir.path()), &cache, &lib, &song);

        assert!(
            result.is_ok(),
            "streaming load should not error: {:?}",
            result.err()
        );
        assert!(
            result.unwrap().is_none(),
            "stems_remote should return Ok(None)"
        );
    }

    #[test]
    fn two_stem_set_downloads_every_required_file() {
        let (_dir, lib) = test_library_root();
        let connection = crate::cache::open_database(&lib.database_path()).expect("open db");
        let song = remote_stems_song("song-2s");
        let entry = two_stem_entry("song-2s");
        insert_stem_entry(&connection, &entry);

        let wav = make_wav(44100, 2, 1000);
        let mut files = HashMap::new();
        files.insert(entry.vocals_path.clone(), wav.clone());
        files.insert(entry.accomp_path.clone(), wav.clone());
        let provider = FakeRemoteProvider::with_files(files);

        let result = super::ensure_remote_stem_set_cached(&provider, &lib, &connection, &song, 1);

        assert!(
            result.is_ok(),
            "two-stem download should succeed: {:?}",
            result.err()
        );

        // Both final paths should exist.
        assert!(lib.resolve(&entry.vocals_path).exists());
        assert!(lib.resolve(&entry.accomp_path).exists());

        // No temp files should remain.
        let stem_dir = lib.resolve("stems/song-2s");
        if let Ok(entries) = std::fs::read_dir(&stem_dir) {
            for e in entries.flatten() {
                assert!(
                    !e.file_name().to_string_lossy().contains(".part."),
                    "temp file left behind: {}",
                    e.path().display()
                );
            }
        }
    }

    #[test]
    fn four_stem_set_downloads_every_required_file() {
        let (_dir, lib) = test_library_root();
        let connection = crate::cache::open_database(&lib.database_path()).expect("open db");
        let song = remote_stems_song("song-4s");
        let entry = four_stem_entry("song-4s");
        insert_stem_entry(&connection, &entry);

        let wav = make_wav(44100, 2, 1000);
        let mut files = HashMap::new();
        files.insert(entry.vocals_path.clone(), wav.clone());
        files.insert(entry.drums_path.clone().unwrap(), wav.clone());
        files.insert(entry.bass_path.clone().unwrap(), wav.clone());
        files.insert(entry.other_path.clone().unwrap(), wav.clone());
        let provider = FakeRemoteProvider::with_files(files);

        let result = super::ensure_remote_stem_set_cached(&provider, &lib, &connection, &song, 1);

        assert!(
            result.is_ok(),
            "four-stem download should succeed: {:?}",
            result.err()
        );
        assert!(lib.resolve(&entry.vocals_path).exists());
        assert!(lib.resolve(entry.drums_path.as_deref().unwrap()).exists());
        assert!(lib.resolve(entry.bass_path.as_deref().unwrap()).exists());
        assert!(lib.resolve(entry.other_path.as_deref().unwrap()).exists());
    }

    #[test]
    fn missing_stem_prevents_entire_set_from_installing() {
        let (_dir, lib) = test_library_root();
        let connection = crate::cache::open_database(&lib.database_path()).expect("open db");
        let song = remote_stems_song("song-missing");
        let entry = two_stem_entry("song-missing");
        insert_stem_entry(&connection, &entry);

        let wav = make_wav(44100, 2, 1000);
        let mut files = HashMap::new();
        files.insert(entry.vocals_path.clone(), wav.clone());
        // accomp is missing from the provider — download will fail.
        let provider = FakeRemoteProvider::with_files(files);

        let result = super::ensure_remote_stem_set_cached(&provider, &lib, &connection, &song, 1);

        assert!(result.is_err(), "set with missing stem should fail");

        // Neither final path should be installed (all-or-nothing).
        assert!(!lib.resolve(&entry.vocals_path).exists());
        assert!(!lib.resolve(&entry.accomp_path).exists());
    }

    #[test]
    fn truncated_stem_prevents_entire_set_from_installing() {
        let (_dir, lib) = test_library_root();
        let connection = crate::cache::open_database(&lib.database_path()).expect("open db");
        let song = remote_stems_song("song-trunc");
        let entry = two_stem_entry("song-trunc");
        insert_stem_entry(&connection, &entry);

        let good_wav = make_wav(44100, 2, 1000);
        // Truncated WAV: valid header but data chunk cut short.
        let mut truncated = make_wav(44100, 2, 1000);
        truncated.truncate(truncated.len() / 2);

        let mut files = HashMap::new();
        files.insert(entry.vocals_path.clone(), good_wav);
        files.insert(entry.accomp_path.clone(), truncated);
        let provider = FakeRemoteProvider::with_files(files);

        let result = super::ensure_remote_stem_set_cached(&provider, &lib, &connection, &song, 1);

        assert!(result.is_err(), "set with truncated stem should fail");

        // Neither final path should be installed.
        assert!(!lib.resolve(&entry.vocals_path).exists());
        assert!(!lib.resolve(&entry.accomp_path).exists());
    }

    #[test]
    fn corrupt_stem_prevents_entire_set_from_installing() {
        let (_dir, lib) = test_library_root();
        let connection = crate::cache::open_database(&lib.database_path()).expect("open db");
        let song = remote_stems_song("song-corrupt");
        let entry = two_stem_entry("song-corrupt");
        insert_stem_entry(&connection, &entry);

        let good_wav = make_wav(44100, 2, 1000);
        // Corrupt: random bytes that are not valid audio.
        let corrupt: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();

        let mut files = HashMap::new();
        files.insert(entry.vocals_path.clone(), good_wav);
        files.insert(entry.accomp_path.clone(), corrupt);
        let provider = FakeRemoteProvider::with_files(files);

        let result = super::ensure_remote_stem_set_cached(&provider, &lib, &connection, &song, 1);

        assert!(result.is_err(), "set with corrupt stem should fail");
        assert!(!lib.resolve(&entry.vocals_path).exists());
        assert!(!lib.resolve(&entry.accomp_path).exists());
    }

    #[test]
    fn mismatched_sample_rate_rejects_set() {
        let (_dir, lib) = test_library_root();
        let connection = crate::cache::open_database(&lib.database_path()).expect("open db");
        let song = remote_stems_song("song-mismatch");
        let entry = two_stem_entry("song-mismatch");
        insert_stem_entry(&connection, &entry);

        let wav_44100 = make_wav(44100, 2, 1000);
        let wav_48000 = make_wav(48000, 2, 1000);

        let mut files = HashMap::new();
        files.insert(entry.vocals_path.clone(), wav_44100);
        files.insert(entry.accomp_path.clone(), wav_48000);
        let provider = FakeRemoteProvider::with_files(files);

        let result = super::ensure_remote_stem_set_cached(&provider, &lib, &connection, &song, 1);

        let err = result.expect_err("mismatched sample rate should reject set");
        assert!(
            err.to_string().contains("sample rate"),
            "error should mention sample rate: {err}"
        );

        // Neither file should be installed.
        assert!(!lib.resolve(&entry.vocals_path).exists());
        assert!(!lib.resolve(&entry.accomp_path).exists());
    }

    #[test]
    fn already_verified_stems_are_retained_on_retry() {
        let (_dir, lib) = test_library_root();
        let connection = crate::cache::open_database(&lib.database_path()).expect("open db");
        let song = remote_stems_song("song-retry");
        let entry = two_stem_entry("song-retry");
        insert_stem_entry(&connection, &entry);

        // Pre-place a valid vocals file at its final path (simulating a
        // previously verified stem from a prior partial run).
        let wav = make_wav(44100, 2, 1000);
        let vocals_final = lib.resolve(&entry.vocals_path);
        std::fs::create_dir_all(vocals_final.parent().unwrap()).unwrap();
        std::fs::write(&vocals_final, &wav).unwrap();

        // Provider only has accomp — vocals should NOT be re-downloaded.
        let mut files = HashMap::new();
        files.insert(entry.accomp_path.clone(), wav);
        let provider = FakeRemoteProvider::with_files(files);

        let result = super::ensure_remote_stem_set_cached(&provider, &lib, &connection, &song, 1);

        assert!(
            result.is_ok(),
            "retry with retained vocals should succeed: {:?}",
            result.err()
        );
        assert!(vocals_final.exists(), "retained vocals should still exist");
        assert!(
            lib.resolve(&entry.accomp_path).exists(),
            "accomp should be downloaded"
        );
    }

    #[test]
    fn stale_download_for_song_a_does_not_overwrite_song_b_files() {
        // PR #1's stale-guard is structural: the function is synchronous so
        // the song cannot change mid-call.  This test verifies that calling
        // ensure_remote_stem_set_cached for song A does not touch song B's
        // already-installed stem files.  When PR #7 makes this async, the
        // request_id guard will prevent a late completion from song A's
        // download from installing after song B is current.
        let (_dir, lib) = test_library_root();
        let connection = crate::cache::open_database(&lib.database_path()).expect("open db");

        // Set up song B with already-installed stems.
        let entry_b = two_stem_entry("song-b");
        insert_stem_entry(&connection, &entry_b);
        let wav = make_wav(44100, 2, 1000);
        let vocals_b = lib.resolve(&entry_b.vocals_path);
        let accomp_b = lib.resolve(&entry_b.accomp_path);
        std::fs::create_dir_all(vocals_b.parent().unwrap()).unwrap();
        std::fs::write(&vocals_b, &wav).unwrap();
        std::fs::write(&accomp_b, &wav).unwrap();

        // Now download stems for song A with a different request_id.
        let song_a = remote_stems_song("song-a");
        let entry_a = two_stem_entry("song-a");
        insert_stem_entry(&connection, &entry_a);

        let mut files = HashMap::new();
        files.insert(entry_a.vocals_path.clone(), wav.clone());
        files.insert(entry_a.accomp_path.clone(), wav.clone());
        let provider = FakeRemoteProvider::with_files(files);

        let result = super::ensure_remote_stem_set_cached(
            &provider,
            &lib,
            &connection,
            &song_a,
            99, // different request_id
        );

        assert!(
            result.is_ok(),
            "song A download should succeed: {:?}",
            result.err()
        );

        // Song B's files should be untouched — different song_hash, different
        // stem directory.
        assert!(vocals_b.exists(), "song B vocals should still exist");
        assert!(accomp_b.exists(), "song B accompaniment should still exist");

        // Song A's files should now exist.
        assert!(lib.resolve(&entry_a.vocals_path).exists());
        assert!(lib.resolve(&entry_a.accomp_path).exists());
    }

    // ---- PR #7 stale-guard tests (defect #11) ----

    /// A fake provider that counts how many stems it has downloaded, so a
    /// test can flip the stale guard after the first stem completes and
    /// assert the remaining stems are not downloaded.
    struct CountingFakeProvider {
        files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        download_count: Arc<std::sync::atomic::AtomicU32>,
    }

    impl CountingFakeProvider {
        fn with_files(files: HashMap<String, Vec<u8>>) -> Self {
            Self {
                files: Arc::new(Mutex::new(files)),
                download_count: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            }
        }
    }

    impl RemoteProvider for CountingFakeProvider {
        fn get_revision(&self, _relative_path: &str) -> CommandResult<Option<String>> {
            Ok(None)
        }

        fn download_file(&self, relative_path: &str, destination: &Path) -> CommandResult<()> {
            self.download_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let files = self.files.lock().unwrap();
            let data = files.get(relative_path).cloned().ok_or_else(|| {
                CommandError::from(crate::library::error::LibraryError::Internal(format!(
                    "fake provider: file {relative_path} not found"
                )))
            })?;
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(destination, &data).map_err(|e| {
                CommandError::from(crate::library::error::LibraryError::Internal(format!(
                    "fake provider: failed to write {}: {e}",
                    destination.display()
                )))
            })?;
            Ok(())
        }

        fn upload_file(&self, _relative_path: &str) -> CommandResult<()> {
            Ok(())
        }

        fn delete_path(&self, _relative_path: &str) -> CommandResult<()> {
            Ok(())
        }

        fn initialize_or_sync(&self) -> CommandResult<Option<String>> {
            Ok(None)
        }

        fn get_file_size(&self, relative_path: &str) -> CommandResult<Option<u64>> {
            Ok(self
                .files
                .lock()
                .unwrap()
                .get(relative_path)
                .map(|d| d.len() as u64))
        }

        fn refresh_existing(&self) -> CommandResult<Option<String>> {
            Ok(None)
        }
    }

    #[test]
    fn stale_guard_aborts_atomic_rename_when_active_song_changed() {
        // PR #7, defect #11: a late stem-set completion must NOT install
        // files when the active request has moved on. The stale guard
        // returns false before the atomic-rename phase, so temps are
        // discarded and the result is StaleRequest.
        let (_dir, lib) = test_library_root();
        let connection = crate::cache::open_database(&lib.database_path()).expect("open db");
        let song = remote_stems_song("song-stale-rename");
        let entry = two_stem_entry("song-stale-rename");
        insert_stem_entry(&connection, &entry);

        let wav = make_wav(44100, 2, 1000);
        let mut files = HashMap::new();
        files.insert(entry.vocals_path.clone(), wav.clone());
        files.insert(entry.accomp_path.clone(), wav);
        let provider = CountingFakeProvider::with_files(files);

        // Guard is always stale — aborts before any download/rename.
        let result = super::ensure_remote_stem_set_cached_guarded(
            &provider,
            &lib,
            &connection,
            &song,
            1,
            || false,
        );

        let err = result.expect_err("stale guard should abort");
        assert_eq!(
            err.kind,
            crate::remote::errors::RemoteErrorKind::StaleRequest
        );
        // No final paths installed.
        assert!(!lib.resolve(&entry.vocals_path).exists());
        assert!(!lib.resolve(&entry.accomp_path).exists());
        // No temp files left behind.
        let stem_dir = lib.resolve("stems/song-stale-rename");
        if let Ok(entries) = std::fs::read_dir(&stem_dir) {
            for e in entries.flatten() {
                assert!(
                    !e.file_name().to_string_lossy().contains(".part."),
                    "temp file left behind: {}",
                    e.path().display()
                );
            }
        }
    }

    #[test]
    fn stale_guard_cancels_remaining_stem_downloads_mid_set() {
        // PR #7, defect #11: a 4-stem set starts; after stem 1 completes,
        // the request becomes stale. Stems 2-4 must NOT be downloaded.
        let (_dir, lib) = test_library_root();
        let connection = crate::cache::open_database(&lib.database_path()).expect("open db");
        let song = remote_stems_song("song-stale-mid");
        let entry = four_stem_entry("song-stale-mid");
        insert_stem_entry(&connection, &entry);

        let wav = make_wav(44100, 2, 1000);
        let mut files = HashMap::new();
        files.insert(entry.vocals_path.clone(), wav.clone());
        files.insert(entry.drums_path.clone().unwrap(), wav.clone());
        files.insert(entry.bass_path.clone().unwrap(), wav.clone());
        files.insert(entry.other_path.clone().unwrap(), wav);
        let provider = CountingFakeProvider::with_files(files);
        let download_count = Arc::clone(&provider.download_count);

        // The guard flips to stale after the first stem downloads. The
        // orchestrator checks the guard before EACH stem download, so only
        // stem 1 (vocals) is downloaded before the abort.
        let guard = move || download_count.load(std::sync::atomic::Ordering::SeqCst) < 1;
        let result = super::ensure_remote_stem_set_cached_guarded(
            &provider,
            &lib,
            &connection,
            &song,
            1,
            guard,
        );

        let err = result.expect_err("stale guard should abort mid-set");
        assert_eq!(
            err.kind,
            crate::remote::errors::RemoteErrorKind::StaleRequest
        );
        // Only one stem (vocals) was downloaded before the guard flipped.
        assert_eq!(
            provider
                .download_count
                .load(std::sync::atomic::Ordering::SeqCst),
            1,
            "remaining stems must not be downloaded after stale guard flips"
        );
        // No final paths installed (rename aborted).
        assert!(!lib.resolve(&entry.vocals_path).exists());
        assert!(!lib.resolve(entry.drums_path.as_deref().unwrap()).exists());
        assert!(!lib.resolve(entry.bass_path.as_deref().unwrap()).exists());
        assert!(!lib.resolve(entry.other_path.as_deref().unwrap()).exists());
    }

    #[test]
    fn guarded_download_succeeds_when_request_stays_current() {
        // Control: when the guard always returns true, the guarded variant
        // behaves like the synchronous one and installs all stems.
        let (_dir, lib) = test_library_root();
        let connection = crate::cache::open_database(&lib.database_path()).expect("open db");
        let song = remote_stems_song("song-guarded-ok");
        let entry = two_stem_entry("song-guarded-ok");
        insert_stem_entry(&connection, &entry);

        let wav = make_wav(44100, 2, 1000);
        let mut files = HashMap::new();
        files.insert(entry.vocals_path.clone(), wav.clone());
        files.insert(entry.accomp_path.clone(), wav);
        let provider = FakeRemoteProvider::with_files(files);

        let result = super::ensure_remote_stem_set_cached_guarded(
            &provider,
            &lib,
            &connection,
            &song,
            1,
            || true,
        );

        assert!(
            result.is_ok(),
            "guarded download should succeed when current: {:?}",
            result.err()
        );
        assert!(lib.resolve(&entry.vocals_path).exists());
        assert!(lib.resolve(&entry.accomp_path).exists());
    }
}
