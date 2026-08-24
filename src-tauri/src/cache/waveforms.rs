use rusqlite::{params, Connection};
use std::sync::Arc;

pub const MIN_BUCKETS: usize = 24;
pub const MAX_BUCKETS: usize = 1000;

/// Clamp a requested bucket count to the validated effective range used as
/// the cache/singleflight key. The command layer must call this before any
/// cache lookup so two callers asking for 10 and 5000 buckets do not split
/// the cache for the same logical waveform.
pub fn clamp_buckets(buckets: usize) -> usize {
    buckets.clamp(MIN_BUCKETS, MAX_BUCKETS)
}

/// Read a cached waveform BLOB as little-endian f32 peaks.
///
/// Returns `Ok(None)` when no row exists. A row is a cache miss and is
/// deleted best-effort when any validation fails:
/// - BLOB length differs from `buckets * size_of::<f32>()` (checked arithmetic);
/// - any decoded value is non-finite or outside `0.0..=1.0`;
/// - requested bucket count is outside the validated effective range.
///
/// Do not hold the database connection across decoding — read the BLOB into
/// a `Vec<u8>`, drop the statement, then decode.
pub fn get_cached_waveform(
    connection: &Connection,
    song_hash: &str,
    buckets: usize,
) -> rusqlite::Result<Option<Arc<[f32]>>> {
    if !(MIN_BUCKETS..=MAX_BUCKETS).contains(&buckets) {
        return Ok(None);
    }

    let blob: Option<Vec<u8>> = {
        let mut statement = connection
            .prepare("SELECT peaks FROM waveforms WHERE song_hash = ?1 AND buckets = ?2 LIMIT 1")?;
        let mut rows = statement.query(params![song_hash, buckets as i64])?;
        match rows.next()? {
            Some(row) => Some(row.get::<_, Vec<u8>>(0)?),
            None => None,
        }
    };

    let Some(blob) = blob else {
        return Ok(None);
    };

    match decode_peaks(&blob, buckets) {
        Some(peaks) => Ok(Some(Arc::from(peaks))),
        None => {
            // Best-effort delete of the invalid row; ignore failure since a
            // stale row will simply be re-validated and deleted on the next
            // miss too.
            let _ = connection.execute(
                "DELETE FROM waveforms WHERE song_hash = ?1 AND buckets = ?2",
                params![song_hash, buckets as i64],
            );
            Ok(None)
        }
    }
}

/// Persist a validated waveform as a little-endian f32 BLOB.
///
/// Requires `peaks.len() == buckets` and every value finite and in `0.0..=1.0`.
/// Uses `INSERT ... ON CONFLICT(song_hash, buckets) DO UPDATE` so a re-compute
/// for an existing key replaces the previous BLOB atomically.
pub fn save_waveform(
    connection: &Connection,
    song_hash: &str,
    buckets: usize,
    peaks: &[f32],
) -> rusqlite::Result<()> {
    debug_assert_eq!(peaks.len(), buckets, "caller must validate length");
    let blob = encode_peaks(peaks);
    connection.execute(
        "INSERT INTO waveforms (song_hash, buckets, peaks)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(song_hash, buckets) DO UPDATE SET peaks = excluded.peaks",
        params![song_hash, buckets as i64, blob],
    )?;
    Ok(())
}

fn encode_peaks(peaks: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(peaks));
    for peak in peaks {
        bytes.extend_from_slice(&peak.to_le_bytes());
    }
    bytes
}

fn decode_peaks(blob: &[u8], buckets: usize) -> Option<Vec<f32>> {
    // Checked arithmetic: reject any length mismatch rather than truncating.
    let expected = buckets.checked_mul(std::mem::size_of::<f32>())?;
    if blob.len() != expected {
        return None;
    }

    let mut peaks = Vec::with_capacity(buckets);
    for chunk in blob.as_chunks::<{ std::mem::size_of::<f32>() }>().0 {
        let value = f32::from_le_bytes(*chunk);
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return None;
        }
        peaks.push(value);
    }
    Some(peaks)
}

pub fn delete_waveforms_for_song(
    connection: &Connection,
    song_hash: &str,
) -> rusqlite::Result<usize> {
    connection.execute("DELETE FROM waveforms WHERE song_hash = ?1", [song_hash])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::apply_migrations;
    use crate::library::Song;
    use rusqlite::Connection;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        apply_migrations(&conn).expect("migrations");
        conn
    }

    fn insert_song(conn: &Connection, hash: &str) {
        let song = Song {
            hash: hash.to_owned(),
            file_path: Some(format!("media/{hash}.mp3")),
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: "original".to_owned(),
            title: None,
            artist: None,
            album: None,
            duration_ms: 0,
            cover_art: None,
            has_cover_art: false,
            artwork_thumb_path: None,
            imported_at: 0,
            original_ext: None,
        };
        super::super::upsert_song(conn, &song).expect("insert song");
    }

    fn sample_peaks(buckets: usize) -> Vec<f32> {
        (0..buckets)
            .map(|i| (i as f32 / buckets as f32) * 0.9 + 0.05)
            .collect()
    }

    #[test]
    fn clamp_buckets_enforces_valid_range() {
        assert_eq!(clamp_buckets(0), MIN_BUCKETS);
        assert_eq!(clamp_buckets(10), MIN_BUCKETS);
        assert_eq!(clamp_buckets(50), 50);
        assert_eq!(clamp_buckets(5000), MAX_BUCKETS);
    }

    #[test]
    fn save_and_get_round_trip() {
        let conn = test_db();
        insert_song(&conn, "hash-1");
        let peaks = sample_peaks(200);
        save_waveform(&conn, "hash-1", 200, &peaks).expect("save");
        let retrieved = get_cached_waveform(&conn, "hash-1", 200)
            .expect("get")
            .expect("entry exists");
        assert_eq!(retrieved.as_ref(), peaks.as_slice());
    }

    #[test]
    fn get_returns_none_for_missing() {
        let conn = test_db();
        insert_song(&conn, "hash-1");
        assert!(get_cached_waveform(&conn, "hash-1", 200)
            .expect("get")
            .is_none());
        assert!(get_cached_waveform(&conn, "missing", 200)
            .expect("get")
            .is_none());
    }

    #[test]
    fn composite_key_isolates_bucket_counts() {
        let conn = test_db();
        insert_song(&conn, "hash-1");
        let peaks_200 = sample_peaks(200);
        let peaks_400 = sample_peaks(400);
        save_waveform(&conn, "hash-1", 200, &peaks_200).expect("save 200");
        save_waveform(&conn, "hash-1", 400, &peaks_400).expect("save 400");

        let r200 = get_cached_waveform(&conn, "hash-1", 200)
            .expect("get 200")
            .expect("200 exists");
        let r400 = get_cached_waveform(&conn, "hash-1", 400)
            .expect("get 400")
            .expect("400 exists");
        assert_eq!(r200.len(), 200);
        assert_eq!(r400.len(), 400);
        assert_ne!(r200.as_ref(), r400.as_ref());
    }

    #[test]
    fn save_replaces_on_conflict() {
        let conn = test_db();
        insert_song(&conn, "hash-1");
        let first = sample_peaks(200);
        save_waveform(&conn, "hash-1", 200, &first).expect("first save");
        let second: Vec<f32> = vec![0.1; 200];
        save_waveform(&conn, "hash-1", 200, &second).expect("second save");
        let retrieved = get_cached_waveform(&conn, "hash-1", 200)
            .expect("get")
            .expect("entry");
        assert_eq!(retrieved.as_ref(), second.as_slice());
    }

    #[test]
    fn invalid_blob_length_is_deleted_and_misses() {
        let conn = test_db();
        insert_song(&conn, "hash-1");
        conn.execute(
            "INSERT INTO waveforms (song_hash, buckets, peaks) VALUES (?1, ?2, ?3)",
            params!["hash-1", 200_i64, vec![0u8; 100]],
        )
        .expect("insert bad row");

        let first = get_cached_waveform(&conn, "hash-1", 200).expect("get");
        assert!(first.is_none(), "invalid row should miss");

        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM waveforms WHERE song_hash = ?1 AND buckets = ?2",
                params!["hash-1", 200_i64],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(remaining, 0);
    }

    #[test]
    fn non_finite_values_in_blob_are_rejected() {
        let conn = test_db();
        insert_song(&conn, "hash-1");
        let mut blob = Vec::new();
        for _ in 0..199 {
            blob.extend_from_slice(&0.5f32.to_le_bytes());
        }
        blob.extend_from_slice(&f32::NAN.to_le_bytes());
        conn.execute(
            "INSERT INTO waveforms (song_hash, buckets, peaks) VALUES (?1, ?2, ?3)",
            params!["hash-1", 200_i64, blob],
        )
        .expect("insert nan row");

        let first = get_cached_waveform(&conn, "hash-1", 200).expect("get");
        assert!(first.is_none(), "non-finite row should miss");
    }

    #[test]
    fn out_of_range_values_in_blob_are_rejected() {
        let conn = test_db();
        insert_song(&conn, "hash-1");
        let mut blob = Vec::new();
        for _ in 0..199 {
            blob.extend_from_slice(&0.5f32.to_le_bytes());
        }
        blob.extend_from_slice(&1.5f32.to_le_bytes());
        conn.execute(
            "INSERT INTO waveforms (song_hash, buckets, peaks) VALUES (?1, ?2, ?3)",
            params!["hash-1", 200_i64, blob],
        )
        .expect("insert out-of-range row");

        let first = get_cached_waveform(&conn, "hash-1", 200).expect("get");
        assert!(first.is_none(), "out-of-range row should miss");
    }

    #[test]
    fn out_of_range_bucket_count_returns_none() {
        let conn = test_db();
        insert_song(&conn, "hash-1");
        assert!(get_cached_waveform(&conn, "hash-1", MIN_BUCKETS - 1)
            .expect("get")
            .is_none());
        assert!(get_cached_waveform(&conn, "hash-1", MAX_BUCKETS + 1)
            .expect("get")
            .is_none());
    }

    #[test]
    fn fk_cascade_deletes_waveforms_when_song_deleted() {
        let conn = test_db();
        // Foreign keys are off by default in SQLite; enable for this test.
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("fk on");
        insert_song(&conn, "hash-1");
        let peaks = sample_peaks(200);
        save_waveform(&conn, "hash-1", 200, &peaks).expect("save");
        assert!(get_cached_waveform(&conn, "hash-1", 200)
            .expect("get")
            .is_some());

        conn.execute("DELETE FROM songs WHERE hash = ?1", ["hash-1"])
            .expect("delete song");

        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM waveforms WHERE song_hash = ?1",
                ["hash-1"],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(remaining, 0, "FK cascade should delete waveforms");
    }

    #[test]
    fn migration_creates_waveforms_table() {
        let conn = test_db();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'waveforms'",
                [],
                |row| row.get(0),
            )
            .expect("table lookup");
        assert_eq!(count, 1);
    }

    #[test]
    fn migration_runs_idempotently_on_initialized_library() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        apply_migrations(&conn).expect("first migration pass");
        apply_migrations(&conn).expect("second migration pass");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'waveforms'",
                [],
                |row| row.get(0),
            )
            .expect("table lookup");
        assert_eq!(count, 1);
    }

    #[test]
    fn delete_waveforms_for_song_removes_all_bucket_counts() {
        let conn = test_db();
        insert_song(&conn, "hash-1");
        save_waveform(&conn, "hash-1", 200, &sample_peaks(200)).expect("save 200");
        save_waveform(&conn, "hash-1", 400, &sample_peaks(400)).expect("save 400");

        let deleted = delete_waveforms_for_song(&conn, "hash-1").expect("delete");
        assert_eq!(deleted, 2);
        assert!(get_cached_waveform(&conn, "hash-1", 200)
            .expect("get 200")
            .is_none());
        assert!(get_cached_waveform(&conn, "hash-1", 400)
            .expect("get 400")
            .is_none());
    }

    #[test]
    fn encode_decode_round_trip_all_edge_values() {
        let values = [0.0f32, 0.5, 1.0, 0.123_456_79];
        let blob = encode_peaks(&values);
        let decoded = decode_peaks(&blob, values.len()).expect("decode");
        assert_eq!(decoded, values.to_vec());
    }
}
