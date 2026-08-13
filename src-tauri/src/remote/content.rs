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
    remote,
    remote::cache_catalog::{CacheCatalog, CacheIdentity, CachePinGuard},
    remote::provider::{create_remote_media_source, create_repository_storage, RepositoryStorage},
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

pub(crate) struct StreamingPlaybackSource {
    pub(crate) streaming_track: StreamingTrack,
    pub(crate) metadata: StreamMetadata,
    pub(crate) decode_handle: std::thread::JoinHandle<Result<(), decode::DecodeError>>,
    pub(crate) fetch_event_rx: Option<mpsc::Receiver<FetchEvent>>,
    pub(crate) cache_pin_guard: Option<CachePinGuard>,
}

pub(crate) struct RemoteContent<'a> {
    app_data_dir: Option<&'a Path>,
}

impl<'a> RemoteContent<'a> {
    pub(crate) fn new(app_data_dir: Option<&'a Path>) -> Self {
        Self { app_data_dir }
    }

    pub(crate) fn ensure_song_files_cached(&self, song: &Song) -> Result<()> {
        let Some(app_data_dir) = self.app_data_dir else {
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

    pub(crate) fn ensure_stem_files_cached(
        &self,
        library_root: &LibraryRoot,
        connection: &Connection,
        song: &Song,
        request_id: u64,
        is_current: impl Fn() -> bool,
    ) -> std::result::Result<(), remote::errors::RemoteError> {
        let Some(app_data_dir) = self.app_data_dir else {
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
        let provider = create_repository_storage(app_data_dir, &library).map_err(|error| {
            remote::errors::RemoteError::new(
                remote::errors::RemoteErrorKind::NetworkUnavailable,
                error.message.clone(),
            )
        })?;
        ensure_stem_set_cached(
            provider.as_ref(),
            library_root,
            connection,
            song,
            request_id,
            is_current,
        )
    }

    pub(crate) fn load_stems_playback_source(
        &self,
        connection: &Connection,
        library_root: &LibraryRoot,
        song: &Song,
    ) -> Result<(decode::DecodedAudio, LoadedStems)> {
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
            let decoded_audio = vocals.clone();
            let stems = LoadedStems::FourStem(StemSet {
                vocals,
                drums,
                bass,
                other,
            });
            Ok((decoded_audio, stems))
        } else {
            let LoadedStems::TwoStem {
                vocals,
                accompaniment,
            } = decode_stem_entry(library_root, &cached)?
            else {
                unreachable!("two stem cache entries decode to two stems");
            };
            let decoded_audio = accompaniment.clone();
            let stems = LoadedStems::TwoStem {
                vocals,
                accompaniment,
            };
            Ok((decoded_audio, stems))
        }
    }

    pub(crate) fn load_streaming_source(
        &self,
        remote_chunk_cache: &Arc<Mutex<CacheCatalog>>,
        song: &Song,
    ) -> Result<Option<StreamingPlaybackSource>, PlaybackError> {
        let Some(app_data_dir) = self.app_data_dir else {
            return Ok(None);
        };
        let song_path = song
            .file_path
            .as_deref()
            .with_context(|| format!("song {} does not have a local file path", song.hash))
            .map_err(|error| PlaybackError::Internal(error.to_string()))?;
        let Some(library) = remote::active_remote_library(app_data_dir)
            .map_err(|error| PlaybackError::Internal(error.message.clone()))?
        else {
            return Ok(None);
        };
        let media_source = create_remote_media_source(app_data_dir, &library)
            .map_err(|error| PlaybackError::Internal(error.message.clone()))?;
        if !media_source.capabilities().range_download {
            return Ok(None);
        }
        let repository = create_repository_storage(app_data_dir, &library)
            .map_err(|error| PlaybackError::Internal(error.message.clone()))?;
        let fetcher = match media_source.create_range_fetcher(song_path) {
            Ok(Some(fetcher)) => fetcher,
            Ok(None) | Err(_) => return Ok(None),
        };
        let Some(file_size) = media_source
            .get_file_size(song_path)
            .map_err(|error| PlaybackError::Internal(error.message.clone()))?
        else {
            return Ok(None);
        };
        if file_size == 0 {
            return Ok(None);
        }

        let provider_revision = repository.get_revision(song_path).ok().flatten();
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
        let cache_pin_guard = Some(
            CacheCatalog::pin_cache_entry(remote_chunk_cache, &cache_key)
                .map_err(map_cache_error)?,
        );
        let persist_catalog = Arc::clone(remote_chunk_cache);
        let persist_key = cache_key.clone();
        let on_range_written: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            if let Ok(manager) = persist_catalog.lock() {
                let _ = manager.persist_ranges(&persist_key);
            }
        });
        let (fetch_tx, fetch_event_rx, _bandwidth_monitor, _fetch_handle) =
            remote_source::spawn_fetch_thread_with_fetcher(
                String::new(),
                Arc::clone(&cache),
                fetcher,
                remote_source::RetryConfig::default(),
                Some(on_range_written),
            );
        let extension = Path::new(song_path)
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_owned);
        let probe_source = RemoteMediaSource::new(Arc::clone(&cache), fetch_tx.clone());
        let probe_metadata = probe_remote_source(probe_source, extension.as_deref())
            .map_err(|error| PlaybackError::AudioDecodeFailed(error.to_string()))?;
        let startup_bytes = file_size.min(16 * 1024);
        let decode_source =
            RemoteMediaSource::new(cache, fetch_tx).with_startup_buffer(startup_bytes);
        let (consumer, decode_handle) = streaming::spawn_decode_producer_from_source(
            Box::new(decode_source),
            extension.as_deref(),
            &probe_metadata,
            streaming::ProxyConfig::none(),
        )
        .map_err(|error| PlaybackError::AudioDecodeFailed(error.to_string()))?;

        Ok(Some(StreamingPlaybackSource {
            streaming_track: StreamingTrack::Single { consumer },
            metadata: probe_metadata,
            decode_handle,
            fetch_event_rx: Some(fetch_event_rx),
            cache_pin_guard,
        }))
    }
}

#[derive(Debug, Clone)]
struct RequiredStem {
    label: &'static str,
    relative_path: String,
}

fn required_stems(entry: &cache::stems::StemCacheEntry) -> Result<Vec<RequiredStem>> {
    let required_path = |label: &'static str, path: Option<&str>| -> Result<RequiredStem> {
        let relative_path = path
            .filter(|path| !path.trim().is_empty())
            .with_context(|| format!("stem cache entry has no {label} path"))?;
        Ok(RequiredStem {
            label,
            relative_path: relative_path.to_owned(),
        })
    };

    if entry.has_individual_stems() {
        Ok(vec![
            required_path("vocals", Some(entry.vocals_path.as_str()))?,
            required_path("drums", entry.drums_path.as_deref())?,
            required_path("bass", entry.bass_path.as_deref())?,
            required_path("other", entry.other_path.as_deref())?,
        ])
    } else {
        Ok(vec![
            required_path("vocals", Some(entry.vocals_path.as_str()))?,
            required_path("accompaniment", Some(entry.accomp_path.as_str()))?,
        ])
    }
}

struct VerifiedStem {
    temp_path: PathBuf,
    final_path: PathBuf,
}

struct StemSetMetadata {
    sample_rate_hz: u32,
    channels: usize,
    frame_count: usize,
}

fn decode_stem_metadata(path: &Path) -> Result<StemSetMetadata> {
    let audio = decode::decode_file(path)
        .with_context(|| format!("failed to decode stem at {}", path.display()))?;
    if audio.sample_rate_hz == 0 || audio.channels == 0 || audio.samples.is_empty() {
        anyhow::bail!("decoded stem has no usable audio metadata");
    }
    if audio.samples.len() % audio.channels != 0 {
        anyhow::bail!("decoded stem samples are not aligned to its channels");
    }
    Ok(StemSetMetadata {
        sample_rate_hz: audio.sample_rate_hz,
        channels: audio.channels,
        frame_count: audio.samples.len() / audio.channels,
    })
}

pub(crate) fn ensure_stem_set_cached(
    provider: &dyn RepositoryStorage,
    library_root: &LibraryRoot,
    connection: &Connection,
    song: &Song,
    request_id: u64,
    is_current: impl Fn() -> bool,
) -> std::result::Result<(), remote::errors::RemoteError> {
    ensure_stem_set_cached_inner(
        provider,
        library_root,
        connection,
        song,
        request_id,
        &is_current,
    )
    .map_err(|error| match error {
        StemMaterializationError::StaleRequest => {
            remote::errors::RemoteError::from_kind(remote::errors::RemoteErrorKind::StaleRequest)
        }
        StemMaterializationError::Failed(error) => remote::errors::RemoteError::new(
            remote::errors::RemoteErrorKind::NetworkUnavailable,
            error.to_string(),
        ),
    })
}

#[derive(Debug)]
enum StemMaterializationError {
    StaleRequest,
    Failed(anyhow::Error),
}

impl From<anyhow::Error> for StemMaterializationError {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed(error)
    }
}

fn ensure_stem_set_cached_inner(
    provider: &dyn RepositoryStorage,
    library_root: &LibraryRoot,
    connection: &Connection,
    song: &Song,
    request_id: u64,
    is_current: &dyn Fn() -> bool,
) -> std::result::Result<(), StemMaterializationError> {
    let Some(cached) = cache::stems::get_cached_stem_entry(connection, &song.hash)
        .context("failed to load cached stems")?
    else {
        return Ok(());
    };
    let required = required_stems(&cached)?;
    let materialization_id = format!("{request_id}-{}", uuid::Uuid::new_v4());
    let mut verified = Vec::new();
    let mut to_download = Vec::new();
    let mut reference_metadata = None;

    for stem in &required {
        let final_path = library_root.resolve(&stem.relative_path);
        if final_path.exists() {
            if let Ok(metadata) = decode_stem_metadata(&final_path) {
                reference_metadata.get_or_insert(metadata);
                continue;
            }
        }
        to_download.push(stem.clone());
    }

    for stem in &to_download {
        if !is_current() {
            clean_up_temps(&verified);
            return Err(StemMaterializationError::StaleRequest);
        }
        let final_path = library_root.resolve(&stem.relative_path);
        let temp_path =
            final_path.with_extension(format!("part.{}.{}", stem.label, materialization_id));
        if let Some(parent) = temp_path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                clean_up_temps(&verified);
                return Err(StemMaterializationError::Failed(anyhow::anyhow!(
                    "failed to create stem directory {}: {}",
                    parent.display(),
                    error
                )));
            }
        }
        let _ = fs::remove_file(&temp_path);
        if let Err(error) = provider.download_file(&stem.relative_path, &temp_path) {
            let _ = fs::remove_file(&temp_path);
            clean_up_temps(&verified);
            return Err(StemMaterializationError::Failed(anyhow::anyhow!(
                "failed to download stem {}: {}",
                stem.label,
                error.message
            )));
        }
        if fs::metadata(&temp_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0)
            == 0
        {
            let _ = fs::remove_file(&temp_path);
            clean_up_temps(&verified);
            return Err(StemMaterializationError::Failed(anyhow::anyhow!(
                "stem {} downloaded as a zero-byte file",
                stem.label
            )));
        }
        match decode_stem_metadata(&temp_path) {
            Ok(metadata) => {
                reference_metadata.get_or_insert(metadata);
                verified.push(VerifiedStem {
                    temp_path,
                    final_path,
                });
            }
            Err(error) => {
                let _ = fs::remove_file(&temp_path);
                clean_up_temps(&verified);
                return Err(StemMaterializationError::Failed(anyhow::anyhow!(
                    "stem {} failed validation decode: {}",
                    stem.label,
                    error
                )));
            }
        }
    }

    let reference = reference_metadata
        .ok_or_else(|| anyhow::anyhow!("no stems to validate for song {}", song.hash))?;
    for stem in &required {
        let final_path = library_root.resolve(&stem.relative_path);
        let path_to_check = verified
            .iter()
            .find(|value| value.final_path == final_path)
            .map(|value| value.temp_path.as_path())
            .unwrap_or(final_path.as_path());
        if !path_to_check.exists() {
            clean_up_temps(&verified);
            return Err(StemMaterializationError::Failed(anyhow::anyhow!(
                "stem {} is missing after download phase",
                stem.label
            )));
        }
        let metadata = decode_stem_metadata(path_to_check)
            .with_context(|| format!("failed to decode stem {} for alignment check", stem.label))?;
        if metadata.sample_rate_hz != reference.sample_rate_hz {
            clean_up_temps(&verified);
            return Err(StemMaterializationError::Failed(anyhow::anyhow!(
                "stem {} sample rate {} does not match set reference {}",
                stem.label,
                metadata.sample_rate_hz,
                reference.sample_rate_hz
            )));
        }
        if metadata.channels != reference.channels {
            clean_up_temps(&verified);
            return Err(StemMaterializationError::Failed(anyhow::anyhow!(
                "stem {} channel count {} does not match set reference {}",
                stem.label,
                metadata.channels,
                reference.channels
            )));
        }
        if metadata.frame_count != reference.frame_count {
            clean_up_temps(&verified);
            return Err(StemMaterializationError::Failed(anyhow::anyhow!(
                "stem {} PCM frame count {} does not match set reference {}",
                stem.label,
                metadata.frame_count,
                reference.frame_count
            )));
        }
    }

    if !is_current() {
        clean_up_temps(&verified);
        return Err(StemMaterializationError::StaleRequest);
    }
    atomically_install_stem_set(
        &required,
        &verified,
        library_root,
        &song.hash,
        &materialization_id,
    )?;
    Ok(())
}

fn atomically_install_stem_set(
    required: &[RequiredStem],
    verified: &[VerifiedStem],
    library_root: &LibraryRoot,
    song_id: &str,
    materialization_id: &str,
) -> Result<()> {
    let final_paths = required
        .iter()
        .map(|stem| library_root.resolve(&stem.relative_path))
        .collect::<Vec<_>>();
    let stem_dir = final_paths
        .first()
        .and_then(|path| path.parent())
        .context("stem set has no parent directory")?;
    if final_paths
        .iter()
        .any(|path| path.parent() != Some(stem_dir))
    {
        anyhow::bail!("stem set paths must share one directory");
    }
    let repository_dir = stem_dir
        .parent()
        .context("stem directory has no repository parent")?;
    fs::create_dir_all(repository_dir).with_context(|| {
        format!(
            "failed to create stem repository directory {}",
            repository_dir.display()
        )
    })?;

    let staging_dir = repository_dir.join(format!(".{}.staging.{}", song_id, materialization_id));
    let backup_dir = repository_dir.join(format!(".{}.backup.{}", song_id, materialization_id));
    let _ = fs::remove_dir_all(&staging_dir);
    let _ = fs::remove_dir_all(&backup_dir);
    fs::create_dir(&staging_dir).with_context(|| {
        format!(
            "failed to create stem staging directory {}",
            staging_dir.display()
        )
    })?;
    let _staging_cleanup = StagingDirectoryCleanup {
        path: staging_dir.clone(),
    };

    for final_path in &final_paths {
        let file_name = final_path
            .file_name()
            .context("stem path has no file name")?;
        let staging_path = staging_dir.join(file_name);
        if let Some(value) = verified
            .iter()
            .find(|value| value.final_path == *final_path)
        {
            fs::rename(&value.temp_path, &staging_path).with_context(|| {
                format!(
                    "failed to stage verified stem {}",
                    value.temp_path.display()
                )
            })?;
        } else {
            fs::copy(final_path, &staging_path)
                .with_context(|| format!("failed to copy cached stem {}", final_path.display()))?;
        }
    }

    if stem_dir.exists() {
        fs::rename(stem_dir, &backup_dir)
            .with_context(|| format!("failed to move current stem set {}", stem_dir.display()))?;
    }
    if let Err(error) = fs::rename(&staging_dir, stem_dir) {
        if backup_dir.exists() {
            let _ = fs::rename(&backup_dir, stem_dir);
        }
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(error).with_context(|| {
            format!(
                "failed to atomically install stem set {}",
                stem_dir.display()
            )
        });
    }
    let _ = fs::remove_dir_all(&backup_dir);
    Ok(())
}

struct StagingDirectoryCleanup {
    path: PathBuf,
}

impl Drop for StagingDirectoryCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn clean_up_temps(verified: &[VerifiedStem]) {
    for value in verified {
        let _ = fs::remove_file(&value.temp_path);
    }
}

pub(crate) fn decode_stem_entry(
    library_root: &LibraryRoot,
    cached: &cache::stems::StemCacheEntry,
) -> Result<LoadedStems> {
    let load_stem = |path: &str| -> Result<decode::DecodedAudio> {
        let absolute_path = library_root.resolve(path);
        decode::decode_file(&absolute_path)
            .map_err(|error| anyhow::anyhow!("failed to decode stem {}: {}", path, error))
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

fn probe_remote_source(
    source: RemoteMediaSource,
    extension: Option<&str>,
) -> Result<StreamMetadata, decode::DecodeError> {
    use symphonia::core::{
        codecs::audio::AudioDecoderOptions,
        formats::{probe::Hint, FormatOptions, TrackType},
        io::MediaSourceStream,
        meta::MetadataOptions,
        units::Timestamp,
    };

    let mut hint = Hint::new();
    if let Some(extension) = extension {
        hint.with_extension(extension);
    }
    let mut probed = symphonia::default::get_probe()
        .probe(
            &hint,
            MediaSourceStream::new(Box::new(source), Default::default()),
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| decode::DecodeError::ProbeFailed(format!("remote source: {error}")))?;
    let track = probed
        .default_track(TrackType::Audio)
        .ok_or(decode::DecodeError::NoDefaultTrack)?;
    let audio_params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or(decode::DecodeError::NoDefaultTrack)?
        .clone();
    let track_id = track.id;
    let n_frames = track.num_frames;
    let time_base = track.time_base;
    let mut sample_rate = audio_params.sample_rate;
    let mut channels = audio_params.channels.as_ref().map(|value| value.count());
    if sample_rate.is_none() || channels.is_none() {
        if let Ok(mut decoder) = symphonia::default::get_codecs()
            .make_audio_decoder(&audio_params, &AudioDecoderOptions::default())
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
    let sample_rate = sample_rate
        .filter(|rate| *rate > 0)
        .ok_or_else(|| decode::DecodeError::MissingSampleRate("remote source".to_owned()))?;
    let channels = channels
        .filter(|count| *count > 0)
        .ok_or_else(|| decode::DecodeError::MissingChannels("remote source".to_owned()))?;
    let duration_ms = n_frames
        .zip(time_base)
        .and_then(|(frames, time_base)| time_base.calc_time(Timestamp::new(frames as i64)))
        .map(|time| time.as_millis() as u64);
    Ok(StreamMetadata {
        sample_rate_hz: sample_rate,
        channels,
        duration_ms,
    })
}

fn map_cache_error(error: crate::commands::error::CommandError) -> PlaybackError {
    PlaybackError::Internal(error.message)
}
