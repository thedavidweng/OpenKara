use crate::lyrics::fetch::LyricsSource;
use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricsCacheEntry {
    pub song_hash: String,
    pub lrc: String,
    pub source: LyricsSource,
    pub offset_ms: i64,
    pub fetched_at: i64,
}

pub fn upsert_lyrics_cache_entry(
    connection: &Connection,
    entry: &LyricsCacheEntry,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO lyrics (
            song_hash,
            lrc,
            source,
            offset_ms,
            fetched_at
        ) VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(song_hash) DO UPDATE SET
            lrc = excluded.lrc,
            source = excluded.source,
            offset_ms = excluded.offset_ms,
            fetched_at = excluded.fetched_at",
        params![
            entry.song_hash,
            entry.lrc,
            serialize_source(&entry.source),
            entry.offset_ms,
            entry.fetched_at,
        ],
    )?;

    Ok(())
}

pub fn get_lyrics_cache_entry(
    connection: &Connection,
    song_hash: &str,
) -> Result<Option<LyricsCacheEntry>> {
    let mut statement = connection.prepare(
        "SELECT song_hash, lrc, source, offset_ms, fetched_at
        FROM lyrics
        WHERE song_hash = ?1
        LIMIT 1",
    )?;

    let mut rows = statement.query([song_hash])?;
    match rows.next()? {
        Some(row) => Ok(Some(LyricsCacheEntry {
            song_hash: row.get(0)?,
            lrc: row.get(1)?,
            source: deserialize_source(row.get::<_, String>(2)?.as_str())?,
            offset_ms: row.get(3)?,
            fetched_at: row.get(4)?,
        })),
        None => Ok(None),
    }
}

pub fn set_lyrics_offset(
    connection: &Connection,
    song_hash: &str,
    offset_ms: i64,
) -> rusqlite::Result<()> {
    connection.execute(
        "UPDATE lyrics
        SET offset_ms = ?2
        WHERE song_hash = ?1",
        params![song_hash, offset_ms],
    )?;

    Ok(())
}

/// Delete all lyrics cache entries from the database.
/// Returns the number of deleted entries.
pub fn delete_all_lyrics_cache_entries(connection: &Connection) -> rusqlite::Result<usize> {
    connection.execute("DELETE FROM lyrics", [])
}

fn serialize_source(source: &LyricsSource) -> &'static str {
    match source {
        LyricsSource::LrcLib => "lrclib",
        LyricsSource::LrcApi => "lrc_api",
        LyricsSource::Embedded => "embedded",
        LyricsSource::Sidecar => "sidecar",
        LyricsSource::Manual => "manual",
    }
}

fn deserialize_source(source: &str) -> Result<LyricsSource> {
    match source {
        "lrclib" => Ok(LyricsSource::LrcLib),
        "lrc_api" => Ok(LyricsSource::LrcApi),
        "embedded" => Ok(LyricsSource::Embedded),
        "sidecar" => Ok(LyricsSource::Sidecar),
        "manual" => Ok(LyricsSource::Manual),
        other => Err(anyhow!("unknown lyrics source {other}"))
            .with_context(|| format!("failed to deserialize lyrics source {source}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    use crate::library::Song;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        super::super::apply_migrations(&conn).expect("migrations");
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
            imported_at: 0,
            original_ext: None,
        };
        super::super::upsert_song(conn, &song).expect("insert song");
    }

    fn sample_entry(song_hash: &str, source: LyricsSource) -> LyricsCacheEntry {
        LyricsCacheEntry {
            song_hash: song_hash.to_owned(),
            lrc: "[00:10.00]Hello world\n".to_owned(),
            source,
            offset_ms: 0,
            fetched_at: 1_000_000,
        }
    }

    #[test]
    fn upsert_and_get_round_trip() {
        let conn = test_db();
        insert_song(&conn, "hash-1");
        let entry = sample_entry("hash-1", LyricsSource::LrcLib);

        upsert_lyrics_cache_entry(&conn, &entry).expect("upsert should succeed");
        let retrieved = get_lyrics_cache_entry(&conn, "hash-1")
            .expect("get should succeed")
            .expect("entry should exist");

        assert_eq!(retrieved, entry);
    }

    #[test]
    fn get_returns_none_for_missing_hash() {
        let conn = test_db();
        let result = get_lyrics_cache_entry(&conn, "nonexistent").expect("get should succeed");
        assert!(result.is_none());
    }

    #[test]
    fn upsert_updates_on_conflict() {
        let conn = test_db();
        insert_song(&conn, "hash-1");
        let mut entry = sample_entry("hash-1", LyricsSource::LrcLib);
        upsert_lyrics_cache_entry(&conn, &entry).expect("first upsert");

        entry.lrc = "[00:20.00]Updated lyrics\n".to_owned();
        entry.source = LyricsSource::Manual;
        entry.offset_ms = 500;
        upsert_lyrics_cache_entry(&conn, &entry).expect("second upsert");

        let retrieved = get_lyrics_cache_entry(&conn, "hash-1")
            .expect("get should succeed")
            .expect("entry should exist");

        assert_eq!(retrieved.lrc, "[00:20.00]Updated lyrics\n");
        assert_eq!(retrieved.source, LyricsSource::Manual);
        assert_eq!(retrieved.offset_ms, 500);
    }

    #[test]
    fn set_lyrics_offset_updates_offset() {
        let conn = test_db();
        insert_song(&conn, "hash-1");
        let entry = sample_entry("hash-1", LyricsSource::LrcLib);
        upsert_lyrics_cache_entry(&conn, &entry).expect("upsert");

        set_lyrics_offset(&conn, "hash-1", -300).expect("set offset");

        let retrieved = get_lyrics_cache_entry(&conn, "hash-1")
            .expect("get should succeed")
            .expect("entry should exist");

        assert_eq!(retrieved.offset_ms, -300);
    }

    #[test]
    fn delete_all_lyrics_cache_entries_returns_count() {
        let conn = test_db();
        insert_song(&conn, "h1");
        insert_song(&conn, "h2");
        insert_song(&conn, "h3");
        upsert_lyrics_cache_entry(&conn, &sample_entry("h1", LyricsSource::LrcLib)).unwrap();
        upsert_lyrics_cache_entry(&conn, &sample_entry("h2", LyricsSource::Manual)).unwrap();
        upsert_lyrics_cache_entry(&conn, &sample_entry("h3", LyricsSource::Embedded)).unwrap();

        let deleted = delete_all_lyrics_cache_entries(&conn).expect("delete should succeed");
        assert_eq!(deleted, 3);

        assert!(get_lyrics_cache_entry(&conn, "h1").unwrap().is_none());
        assert!(get_lyrics_cache_entry(&conn, "h2").unwrap().is_none());
        assert!(get_lyrics_cache_entry(&conn, "h3").unwrap().is_none());
    }

    #[test]
    fn serialize_deserialize_round_trip_all_sources() {
        let sources = [
            LyricsSource::LrcLib,
            LyricsSource::LrcApi,
            LyricsSource::Embedded,
            LyricsSource::Sidecar,
            LyricsSource::Manual,
        ];

        for source in &sources {
            let serialized = serialize_source(source);
            let deserialized =
                deserialize_source(serialized).expect(&format!("deserialize {serialized}"));
            assert_eq!(&deserialized, source);
        }
    }

    #[test]
    fn deserialize_source_rejects_unknown() {
        let result = deserialize_source("unknown_source");
        assert!(result.is_err());
    }
}
