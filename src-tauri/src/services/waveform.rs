//! #90: Waveform computation service.
//!
//! Owns the blocking work performed by the singleflight computation task:
//! open a fresh DB connection, re-check the song exists and is local, return
//! a cached value when present, decode the audio, compute peaks, persist
//! them, and return the shared result. The command layer spawns this inside
//! `tauri::async_runtime::spawn_blocking` so the work never blocks the
//! async runtime or the WebView.

use crate::audio::waveform::compute_waveform_peaks;
use crate::cache::{self, waveforms};
use crate::library_root::LibraryRoot;
use crate::services::playback_source;
use crate::state::{WaveformKey, WaveformResult};

#[cfg(test)]
use std::sync::Arc;

/// Run the full blocking waveform computation pipeline for one
/// `(song_hash, buckets)` key. The caller has already verified the song
/// exists and is local; this function re-checks both facts under a fresh
/// connection so a song deleted between the command's initial lookup and
/// the computation task is observed and produces a sanitized error rather
/// than a stale cache write.
///
/// Steps:
/// 1. open a new connection from `LibraryRoot::database_path()`;
/// 2. re-read the song by hash and repeat the local-source guard;
/// 3. return a validated cached value when present;
/// 4. drop the connection;
/// 5. decode with `load_song_audio` and compute peaks;
/// 6. open a fresh connection, re-check that the same song still exists,
///    save the result, close it;
/// 7. return the `Arc<[f32]>`.
///
/// All errors are sanitized to a fixed message so no raw absolute paths
/// leak to IPC. The singleflight layer wraps this in a completion guard
/// that converts panics/JoinError into the same sanitized error.
pub fn compute_waveform_blocking(library_root: LibraryRoot, key: WaveformKey) -> WaveformResult {
    inner_compute(&library_root, &key)
}

fn inner_compute(library_root: &LibraryRoot, key: &WaveformKey) -> WaveformResult {
    // Step 1: open a fresh connection for the cache check.
    let connection = cache::open_database(&library_root.database_path())
        .map_err(|_| SANITIZED_WAVEFORM_ERROR.to_owned())?;

    // Step 2: re-read the song and repeat the local-source guard.
    let song = cache::get_song_by_hash(&connection, &key.song_hash)
        .map_err(|_| SANITIZED_WAVEFORM_ERROR.to_owned())?
        .ok_or_else(|| SANITIZED_WAVEFORM_ERROR.to_owned())?;
    if song.is_remote() {
        // The command layer already short-circuits remote sources, but a
        // song could in principle be flipped between the command's lookup
        // and this blocking task. Treat as a miss.
        return Err(SANITIZED_WAVEFORM_ERROR.to_owned());
    }

    // Step 3: return a validated cached value when present.
    if let Some(cached) = waveforms::get_cached_waveform(&connection, &key.song_hash, key.buckets)
        .map_err(|_| SANITIZED_WAVEFORM_ERROR.to_owned())?
    {
        return Ok(cached);
    }

    // Step 4: drop the connection before decoding.
    drop(connection);

    // Step 5: decode and compute peaks.
    let decoded = playback_source::load_song_audio(library_root, &song)
        .map_err(|_| SANITIZED_WAVEFORM_ERROR.to_owned())?;
    let peaks = compute_waveform_peaks(&decoded, key.buckets)
        .map_err(|_| SANITIZED_WAVEFORM_ERROR.to_owned())?;

    // Step 6: open a fresh connection, re-check the song still exists, save.
    let connection = cache::open_database(&library_root.database_path())
        .map_err(|_| SANITIZED_WAVEFORM_ERROR.to_owned())?;
    let still_exists = cache::get_song_by_hash(&connection, &key.song_hash)
        .map_err(|_| SANITIZED_WAVEFORM_ERROR.to_owned())?;
    if still_exists.is_some() {
        // Best-effort save: a failure to persist does not invalidate the
        // in-memory result the waiters will receive.
        let _ = waveforms::save_waveform(&connection, &key.song_hash, key.buckets, &peaks);
    }

    Ok(peaks)
}

/// Sanitized error returned to waiters when the computation task fails for
/// any reason (ordinary error, panic, cancellation, JoinError). The message
/// is intentionally generic — no raw absolute paths leak to IPC.
const SANITIZED_WAVEFORM_ERROR: &str = "waveform computation failed";

/// Test-only entry point that runs the same pipeline against an already-open
/// connection. Used by integration tests that want to inject a fixture DB
/// without a real library root on disk.
#[cfg(test)]
pub(crate) fn compute_waveform_with_connection(
    connection: &rusqlite::Connection,
    library_root: &LibraryRoot,
    key: &WaveformKey,
) -> WaveformResult {
    let song = cache::get_song_by_hash(connection, &key.song_hash)
        .map_err(|_| SANITIZED_WAVEFORM_ERROR.to_owned())?
        .ok_or_else(|| SANITIZED_WAVEFORM_ERROR.to_owned())?;
    if song.is_remote() {
        return Err(SANITIZED_WAVEFORM_ERROR.to_owned());
    }
    if let Some(cached) = waveforms::get_cached_waveform(connection, &key.song_hash, key.buckets)
        .map_err(|_| SANITIZED_WAVEFORM_ERROR.to_owned())?
    {
        return Ok(cached);
    }
    let decoded = playback_source::load_song_audio(library_root, &song)
        .map_err(|_| SANITIZED_WAVEFORM_ERROR.to_owned())?;
    let peaks = compute_waveform_peaks(&decoded, key.buckets)
        .map_err(|_| SANITIZED_WAVEFORM_ERROR.to_owned())?;
    let _ = waveforms::save_waveform(connection, &key.song_hash, key.buckets, &peaks);
    Ok(peaks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::decode::DecodedAudio;
    use crate::cache::{self, waveforms};
    use crate::library::Song;
    use rusqlite::Connection;
    use tempfile::tempdir;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        cache::apply_migrations(&conn).expect("migrations");
        conn
    }

    fn insert_song(conn: &Connection, hash: &str, audio_source_kind: &str) {
        let song = Song {
            hash: hash.to_owned(),
            file_path: Some(format!("media/{hash}.mp3")),
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: audio_source_kind.to_owned(),
            title: None,
            artist: None,
            album: None,
            duration_ms: 0,
            cover_art: None,
            has_cover_art: false,
            imported_at: 0,
            original_ext: Some("mp3".to_owned()),
        };
        cache::upsert_song(conn, &song).expect("insert song");
    }

    fn key(hash: &str, buckets: usize) -> WaveformKey {
        WaveformKey {
            song_hash: hash.to_owned(),
            buckets,
        }
    }

    #[test]
    fn remote_source_returns_error_without_decoding() {
        let conn = test_db();
        insert_song(&conn, "remote-1", "original_remote");
        let tmp = tempdir().expect("temp dir");
        let library_root = LibraryRoot::create(tmp.path().join("Lib").as_path()).expect("lib");
        let k = key("remote-1", 200);
        let result = compute_waveform_with_connection(&conn, &library_root, &k);
        assert!(result.is_err(), "remote source should not compute");
        // No waveform row should have been written.
        assert!(waveforms::get_cached_waveform(&conn, "remote-1", 200)
            .expect("get")
            .is_none());
    }

    #[test]
    fn missing_song_returns_error() {
        let conn = test_db();
        let tmp = tempdir().expect("temp dir");
        let library_root = LibraryRoot::create(tmp.path().join("Lib").as_path()).expect("lib");
        let k = key("missing", 200);
        let result = compute_waveform_with_connection(&conn, &library_root, &k);
        assert!(result.is_err());
    }

    #[test]
    fn cached_value_is_returned_without_decoding() {
        let conn = test_db();
        insert_song(&conn, "cached-1", "original");
        let peaks: Arc<[f32]> = Arc::from(vec![0.42; 200]);
        waveforms::save_waveform(&conn, "cached-1", 200, &peaks).expect("save");

        let tmp = tempdir().expect("temp dir");
        let library_root = LibraryRoot::create(tmp.path().join("Lib").as_path()).expect("lib");
        let k = key("cached-1", 200);
        let result = compute_waveform_with_connection(&conn, &library_root, &k);
        let returned = result.expect("cached ok");
        assert_eq!(returned.as_ref(), peaks.as_ref());
    }

    #[test]
    fn sanitized_error_message_is_generic() {
        let conn = test_db();
        let tmp = tempdir().expect("temp dir");
        let library_root = LibraryRoot::create(tmp.path().join("Lib").as_path()).expect("lib");
        let k = key("missing", 200);
        let err = compute_waveform_with_connection(&conn, &library_root, &k).expect_err("err");
        assert_eq!(err, SANITIZED_WAVEFORM_ERROR);
        // No raw path should leak.
        assert!(!err.contains('/') && !err.contains('\\'));
    }

    #[test]
    fn empty_audio_returns_buckets_zeros_via_compute() {
        // Verify the compute path handles empty audio by returning zeros
        // rather than erroring — the waveform module's contract.
        let audio = DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            duration_ms: 0,
            samples: vec![],
        };
        let peaks = compute_waveform_peaks(&audio, 100).expect("ok");
        assert_eq!(peaks.len(), 100);
        assert!(peaks.iter().all(|p| *p == 0.0));
    }

    #[test]
    fn media_g_local_song_is_supported() {
        // Media+G with audio_source_kind == "original" is local and supported
        // through services::playback_source::load_song_audio. The remote
        // guard checks audio_source_kind, not media_g_container.
        let conn = test_db();
        let song = Song {
            hash: "mediag-1".to_owned(),
            file_path: Some("media-g/mediag-1.zip".to_owned()),
            cdg_path: Some("media-g/mediag-1.cdg".to_owned()),
            media_g_container: Some("zip".to_owned()),
            instrumental: false,
            language: None,
            audio_source_kind: "original".to_owned(),
            title: None,
            artist: None,
            album: None,
            duration_ms: 0,
            cover_art: None,
            has_cover_art: false,
            imported_at: 0,
            original_ext: Some("zip".to_owned()),
        };
        cache::upsert_song(&conn, &song).expect("insert");
        // is_remote() returns false for audio_source_kind == "original".
        let retrieved = cache::get_song_by_hash(&conn, "mediag-1")
            .expect("get")
            .unwrap();
        assert!(!retrieved.is_remote(), "Media+G original is local");
    }
}
