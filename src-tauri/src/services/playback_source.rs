use crate::{
    audio::{
        decode,
        error::PlaybackError,
        playback::LoadedStems,
        streaming::{self, StreamingTrack},
    },
    cache,
    library::Song,
    library_root::LibraryRoot,
    media_g::{self, MEDIA_G_ZIP},
};
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

pub(crate) use crate::remote::content::{
    PlaybackSourceLoad, RemoteContent, StreamingPlaybackSource,
};

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
    request_id: u64,
    is_current: impl Fn() -> bool,
) -> Result<PlaybackSourceLoad, PlaybackError> {
    if song.is_remote_stems() {
        // Remote stems must be fully materialized before synchronous decode.
        RemoteContent::new(app_data_dir)
            .ensure_stem_files_cached(library_root, connection, song, request_id, is_current)
            .map_err(map_stem_materialization_error)?;
        let (decoded_audio, stems) = RemoteContent::new(app_data_dir)
            .load_stems_playback_source(connection, library_root, song)
            .map_err(|e| PlaybackError::Internal(e.to_string()))?;
        return Ok(PlaybackSourceLoad {
            decoded_audio,
            stems: Some(stems),
        });
    }

    if song.is_remote() {
        RemoteContent::new(app_data_dir)
            .ensure_song_files_cached(song)
            .map_err(|e| PlaybackError::Internal(e.to_string()))?;
    }

    Ok(PlaybackSourceLoad {
        decoded_audio: load_song_audio(library_root, song)
            .map_err(|e| PlaybackError::AudioDecodeFailed(e.to_string()))?,
        stems: None,
    })
}

fn map_stem_materialization_error(error: crate::remote::errors::RemoteError) -> PlaybackError {
    if error.kind == crate::remote::errors::RemoteErrorKind::StaleRequest {
        PlaybackError::StaleRequest
    } else {
        PlaybackError::Internal(error.detail.unwrap_or(error.code))
    }
}

pub(crate) fn load_cached_stems_for_song(
    app_data_dir: Option<&Path>,
    connection: &Connection,
    library_root: &LibraryRoot,
    song: &Song,
    request_id: u64,
    is_current: impl Fn() -> bool,
) -> Result<LoadedStems, PlaybackError> {
    if song.is_remote_stems() {
        RemoteContent::new(app_data_dir)
            .ensure_stem_files_cached(library_root, connection, song, request_id, is_current)
            .map_err(map_stem_materialization_error)?;
        return RemoteContent::new(app_data_dir)
            .load_stems_playback_source(connection, library_root, song)
            .map(|(_, stems)| stems)
            .map_err(|e| PlaybackError::Internal(e.to_string()));
    }

    let cached = cache::stems::get_cached_stem_entry(connection, &song.hash)
        .map_err(|e| PlaybackError::Internal(format!("failed to load cached stems: {e}")))?
        .ok_or_else(|| {
            PlaybackError::KaraokeNotReady(format!("no cached stems for song {}", song.hash))
        })?;

    crate::remote::content::decode_stem_entry(library_root, &cached)
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

pub(crate) fn load_playback_source_streaming(
    app_data_dir: Option<&Path>,
    remote_chunk_cache: &Arc<Mutex<crate::remote::cache_catalog::CacheCatalog>>,
    library_root: &LibraryRoot,
    song: &Song,
) -> Result<Option<StreamingPlaybackSource>, PlaybackError> {
    if song.media_g_container.as_deref() == Some(media_g::MEDIA_G_ZIP) || song.is_remote_stems() {
        return Ok(None);
    }
    if song.is_remote() {
        return RemoteContent::new(app_data_dir).load_streaming_source(remote_chunk_cache, song);
    }

    let song_path =
        resolve_song_file_path(song).map_err(|error| PlaybackError::Internal(error.to_string()))?;
    let absolute_path = library_root.resolve(song_path);
    let (consumer, metadata, decode_handle) = streaming::spawn_decode_producer(&absolute_path)
        .map_err(|error| PlaybackError::AudioDecodeFailed(error.to_string()))?;
    Ok(Some(StreamingPlaybackSource {
        streaming_track: StreamingTrack::Single { consumer },
        metadata,
        decode_handle,
        fetch_event_rx: None,
        cache_pin_guard: None,
    }))
}

pub(crate) fn load_remote_streaming_source(
    app_data_dir: Option<&Path>,
    remote_chunk_cache: &Arc<Mutex<crate::remote::cache_catalog::CacheCatalog>>,
    _library_root: &LibraryRoot,
    song: &Song,
) -> Result<Option<StreamingPlaybackSource>, PlaybackError> {
    RemoteContent::new(app_data_dir).load_streaming_source(remote_chunk_cache, song)
}

pub(crate) fn resolve_song_file_path(song: &Song) -> Result<&str> {
    song.file_path
        .as_deref()
        .with_context(|| format!("song {} does not have a local file path", song.hash))
}

#[cfg(test)]
mod tests {
    use crate::cache::stems::StemCacheEntry;
    use crate::commands::error::{CommandError, CommandResult};
    use crate::library::Song;
    use crate::library_root::LibraryRoot;
    use crate::remote::cache_catalog::{CacheCatalog, CacheIdentity, DEFAULT_CACHE_BYTES_LIMIT};
    use crate::remote::control_db::open_control_db;
    use crate::remote::provider::{
        RemoteMediaSource, RemoteMediaSourceCapabilities, RepositoryStorage,
    };
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

    impl RepositoryStorage for FakeRemoteProvider {
        fn media_source(&self) -> &dyn RemoteMediaSource {
            self
        }

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

        fn refresh_existing(&self) -> CommandResult<Option<String>> {
            Ok(None)
        }
    }

    impl RemoteMediaSource for FakeRemoteProvider {
        fn capabilities(&self) -> RemoteMediaSourceCapabilities {
            RemoteMediaSourceCapabilities {
                range_download: false,
            }
        }

        fn get_file_size(&self, relative_path: &str) -> CommandResult<Option<u64>> {
            Ok(self
                .files
                .lock()
                .unwrap()
                .get(relative_path)
                .map(|d| d.len() as u64))
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

        let result = crate::remote::content::ensure_stem_set_cached(
            &provider,
            &lib,
            &connection,
            &song,
            1,
            || true,
        );

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

        let result = crate::remote::content::ensure_stem_set_cached(
            &provider,
            &lib,
            &connection,
            &song,
            1,
            || true,
        );

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

        let result = crate::remote::content::ensure_stem_set_cached(
            &provider,
            &lib,
            &connection,
            &song,
            1,
            || true,
        );

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

        let result = crate::remote::content::ensure_stem_set_cached(
            &provider,
            &lib,
            &connection,
            &song,
            1,
            || true,
        );

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

        let result = crate::remote::content::ensure_stem_set_cached(
            &provider,
            &lib,
            &connection,
            &song,
            1,
            || true,
        );

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

        let result = crate::remote::content::ensure_stem_set_cached(
            &provider,
            &lib,
            &connection,
            &song,
            1,
            || true,
        );

        let err = result.expect_err("mismatched sample rate should reject set");
        assert!(
            err.detail
                .as_deref()
                .expect("materialization failure includes detail")
                .contains("sample rate"),
            "error should mention sample rate: {:?}",
            err
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

        let result = crate::remote::content::ensure_stem_set_cached(
            &provider,
            &lib,
            &connection,
            &song,
            1,
            || true,
        );

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
        // Materialization is scoped by song, so a request for song A must not
        // touch an already-installed stem set for song B.
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

        let result = crate::remote::content::ensure_stem_set_cached(
            &provider,
            &lib,
            &connection,
            &song_a,
            99, // different request_id
            || true,
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

    // ---- Stale-request tests ----

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

    impl RepositoryStorage for CountingFakeProvider {
        fn media_source(&self) -> &dyn RemoteMediaSource {
            self
        }

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

        fn refresh_existing(&self) -> CommandResult<Option<String>> {
            Ok(None)
        }
    }

    impl RemoteMediaSource for CountingFakeProvider {
        fn capabilities(&self) -> RemoteMediaSourceCapabilities {
            RemoteMediaSourceCapabilities {
                range_download: false,
            }
        }

        fn get_file_size(&self, relative_path: &str) -> CommandResult<Option<u64>> {
            Ok(self
                .files
                .lock()
                .unwrap()
                .get(relative_path)
                .map(|d| d.len() as u64))
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
        let result = crate::remote::content::ensure_stem_set_cached(
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
        let result = crate::remote::content::ensure_stem_set_cached(
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

        let result = crate::remote::content::ensure_stem_set_cached(
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

    #[test]
    fn remote_stem_loader_returns_required_stems() {
        let (_dir, lib) = test_library_root();
        let connection = crate::cache::open_database(&lib.database_path()).expect("open db");
        let song = remote_stems_song("song-required-stems");
        let entry = two_stem_entry("song-required-stems");
        insert_stem_entry(&connection, &entry);

        let wav = make_wav(44100, 2, 1000);
        let vocals = lib.resolve(&entry.vocals_path);
        let accompaniment = lib.resolve(&entry.accomp_path);
        std::fs::create_dir_all(vocals.parent().expect("stem directory")).unwrap();
        std::fs::write(vocals, &wav).unwrap();
        std::fs::write(accompaniment, wav).unwrap();

        let loaded = super::load_playback_source(None, &connection, &lib, &song, 0, || true)
            .expect("remote stems should decode");

        assert!(!loaded.decoded_audio.samples.is_empty());
        assert!(matches!(
            loaded.stems,
            Some(crate::audio::playback::LoadedStems::TwoStem { .. })
        ));
    }
}
