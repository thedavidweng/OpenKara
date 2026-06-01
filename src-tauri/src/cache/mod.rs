pub mod lyrics;
pub mod stems;

use crate::library::Song;
use anyhow::Context;
use rusqlite::{params, Connection, Row};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;

const DATABASE_FILENAME: &str = "openkara.sqlite3";
// Keep the SQL in the migrations directory so tests and runtime initialization
// execute the exact same schema definition.
const MIGRATIONS: [&str; 4] = [
    include_str!("../../migrations/001_init.sql"),
    include_str!("../../migrations/002_stems.sql"),
    include_str!("../../migrations/003_lyrics.sql"),
    include_str!("../../migrations/004_portable_paths.sql"),
];

fn database_path(base_dir: &Path) -> PathBuf {
    base_dir.join(DATABASE_FILENAME)
}

pub fn initialize_database(app_handle: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .context("failed to resolve application data directory")?;

    fs::create_dir_all(&app_data_dir).with_context(|| {
        format!(
            "failed to create application data directory at {}",
            app_data_dir.display()
        )
    })?;

    let database_path = database_path(&app_data_dir);
    let connection = open_database(&database_path)?;

    apply_migrations(&connection).context("failed to apply SQLite migrations")?;

    Ok(database_path)
}

pub fn open_database(database_path: &Path) -> anyhow::Result<Connection> {
    let conn = Connection::open(database_path).with_context(|| {
        format!(
            "failed to open SQLite database at {}",
            database_path.display()
        )
    })?;
    // Enable foreign key enforcement for all production connections so ON DELETE
    // CASCADE and other FK constraints are honored (they are off by default in
    // SQLite). Tests that need to verify FK behavior already enable it explicitly.
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .context("failed to enable foreign key enforcement")?;
    Ok(conn)
}

pub fn apply_migrations(connection: &Connection) -> rusqlite::Result<()> {
    for migration in MIGRATIONS {
        connection.execute_batch(migration)?;
    }

    // ALTER TABLE lacks IF NOT EXISTS in SQLite, so we check manually.
    if !column_exists(connection, "songs", "original_ext")? {
        connection.execute_batch("ALTER TABLE songs ADD COLUMN original_ext TEXT;")?;
    }
    if !column_exists(connection, "songs", "cdg_path")? {
        connection.execute_batch("ALTER TABLE songs ADD COLUMN cdg_path TEXT;")?;
    }
    if !column_exists(connection, "songs", "media_g_container")? {
        connection.execute_batch("ALTER TABLE songs ADD COLUMN media_g_container TEXT;")?;
    }
    if !column_exists(connection, "songs", "instrumental")? {
        connection.execute_batch(
            "ALTER TABLE songs ADD COLUMN instrumental INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    if !column_exists(connection, "songs", "language")? {
        connection.execute_batch("ALTER TABLE songs ADD COLUMN language TEXT;")?;
    }
    if !column_exists(connection, "songs", "audio_source_kind")? {
        connection.execute_batch(include_str!("../../migrations/005_audio_source_kind.sql"))?;
    }

    // 005_individual_stem_paths – add per-instrument columns to stems table.
    if !column_exists(connection, "stems", "drums_path")? {
        connection.execute_batch("ALTER TABLE stems ADD COLUMN drums_path TEXT;")?;
    }
    if !column_exists(connection, "stems", "bass_path")? {
        connection.execute_batch("ALTER TABLE stems ADD COLUMN bass_path TEXT;")?;
    }
    if !column_exists(connection, "stems", "other_path")? {
        connection.execute_batch("ALTER TABLE stems ADD COLUMN other_path TEXT;")?;
    }

    // 006_stem_model_variant – track which model produced each song's stems.
    if !column_exists(connection, "stems", "model_variant")? {
        connection
            .execute_batch("ALTER TABLE stems ADD COLUMN model_variant TEXT DEFAULT 'htdemucs';")?;
    }

    migrate_legacy_song_schema(connection)?;

    // 008_playlists – playlist management tables.
    connection.execute_batch(include_str!("../../migrations/008_playlists.sql"))?;
    // 009_singer_rotation – singer rotation state for turn-based queue workflows.
    connection.execute_batch(include_str!("../../migrations/009_singer_rotation.sql"))?;

    Ok(())
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let sql = format!("PRAGMA table_info({})", table);
    let mut stmt = connection.prepare(&sql)?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names.iter().any(|name| name == column))
}

fn column_is_not_null(
    connection: &Connection,
    table: &str,
    column: &str,
) -> rusqlite::Result<bool> {
    let sql = format!("PRAGMA table_info({})", table);
    let mut stmt = connection.prepare(&sql)?;
    let columns = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(3)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(columns
        .into_iter()
        .any(|(name, not_null)| name == column && not_null != 0))
}

fn migrate_legacy_song_schema(connection: &Connection) -> rusqlite::Result<()> {
    let file_path_is_not_null = column_is_not_null(connection, "songs", "file_path")?;
    let has_audio_source_kind = column_exists(connection, "songs", "audio_source_kind")?;

    if !file_path_is_not_null && has_audio_source_kind {
        return Ok(());
    }

    connection.execute_batch(
        "
        PRAGMA foreign_keys = OFF;
        BEGIN;
        DROP TABLE IF EXISTS songs_new;
        CREATE TABLE songs_new (
            hash               TEXT PRIMARY KEY,
            file_path          TEXT,
            title              TEXT,
            artist             TEXT,
            album              TEXT,
            duration_ms        INTEGER,
            cover_art          BLOB,
            imported_at        INTEGER NOT NULL,
            original_ext       TEXT,
            cdg_path           TEXT,
            media_g_container  TEXT,
            instrumental       INTEGER NOT NULL DEFAULT 0,
            audio_source_kind  TEXT NOT NULL DEFAULT 'original'
        );
        INSERT INTO songs_new (
            hash,
            file_path,
            title,
            artist,
            album,
            duration_ms,
            cover_art,
            imported_at,
            original_ext,
            cdg_path,
            media_g_container,
            instrumental,
            audio_source_kind
        )
        SELECT
            hash,
            file_path,
            title,
            artist,
            album,
            duration_ms,
            cover_art,
            imported_at,
            original_ext,
            cdg_path,
            media_g_container,
            instrumental,
            COALESCE(audio_source_kind, 'original')
        FROM songs;
        DROP TABLE songs;
        ALTER TABLE songs_new RENAME TO songs;
        COMMIT;
        PRAGMA foreign_keys = ON;
        ",
    )?;

    Ok(())
}

/// Initialize a database at an explicit path (for use inside a LibraryRoot).
pub fn initialize_library_database(database_path: &Path) -> anyhow::Result<()> {
    let connection = open_database(database_path)?;
    apply_migrations(&connection).context("failed to apply SQLite migrations")?;
    Ok(())
}

pub fn upsert_song(connection: &Connection, song: &Song) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO songs (
            hash,
            file_path,
            cdg_path,
            media_g_container,
            instrumental,
            language,
            audio_source_kind,
            title,
            artist,
            album,
            duration_ms,
            cover_art,
            imported_at,
            original_ext
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(hash) DO UPDATE SET
            file_path = excluded.file_path,
            cdg_path = excluded.cdg_path,
            media_g_container = excluded.media_g_container,
            instrumental = excluded.instrumental,
            language = excluded.language,
            audio_source_kind = excluded.audio_source_kind,
            title = excluded.title,
            artist = excluded.artist,
            album = excluded.album,
            duration_ms = excluded.duration_ms,
            cover_art = excluded.cover_art,
            imported_at = excluded.imported_at,
            original_ext = excluded.original_ext",
        params![
            song.hash,
            song.file_path,
            song.cdg_path,
            song.media_g_container,
            song.instrumental,
            song.language,
            song.audio_source_kind,
            song.title,
            song.artist,
            song.album,
            song.duration_ms,
            song.cover_art,
            song.imported_at,
            song.original_ext,
        ],
    )?;

    Ok(())
}

pub fn list_songs(connection: &Connection) -> rusqlite::Result<Vec<Song>> {
    let mut statement = connection.prepare(
        "SELECT
            hash,
            file_path,
            cdg_path,
            media_g_container,
            instrumental,
            language,
            audio_source_kind,
            title,
            artist,
            album,
            duration_ms,
            cover_art,
            imported_at,
            original_ext
        FROM songs
        ORDER BY imported_at DESC, title COLLATE NOCASE ASC, hash ASC",
    )?;

    let songs = statement
        .query_map([], map_song_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(songs)
}

pub fn search_songs(connection: &Connection, query: &str) -> rusqlite::Result<Vec<Song>> {
    let pattern = format!("%{}%", query.to_lowercase());
    let mut statement = connection.prepare(
        "SELECT
            hash,
            file_path,
            cdg_path,
            media_g_container,
            instrumental,
            language,
            audio_source_kind,
            title,
            artist,
            album,
            duration_ms,
            cover_art,
            imported_at,
            original_ext
        FROM songs
        WHERE lower(coalesce(title, '')) LIKE ?1
           OR lower(coalesce(artist, '')) LIKE ?1
           OR lower(coalesce(album, '')) LIKE ?1
           OR lower(coalesce(file_path, '')) LIKE ?1
        ORDER BY imported_at DESC, title COLLATE NOCASE ASC, hash ASC",
    )?;

    let songs = statement
        .query_map([pattern], map_song_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(songs)
}

pub fn get_song_by_hash(connection: &Connection, hash: &str) -> rusqlite::Result<Option<Song>> {
    let mut statement = connection.prepare(
        "SELECT
            hash,
            file_path,
            cdg_path,
            media_g_container,
            instrumental,
            language,
            audio_source_kind,
            title,
            artist,
            album,
            duration_ms,
            cover_art,
            imported_at,
            original_ext
        FROM songs
        WHERE hash = ?1
        LIMIT 1",
    )?;

    let mut rows = statement.query([hash])?;
    match rows.next()? {
        Some(row) => Ok(Some(map_song_row(row)?)),
        None => Ok(None),
    }
}

pub fn update_song_title_artist(
    connection: &Connection,
    hash: &str,
    title: Option<&str>,
    artist: Option<&str>,
) -> rusqlite::Result<()> {
    connection.execute(
        "UPDATE songs SET title = ?, artist = ? WHERE hash = ?",
        params![title, artist, hash],
    )?;
    Ok(())
}

pub fn update_song_cover_art(
    connection: &Connection,
    hash: &str,
    cover_art: Option<&[u8]>,
) -> rusqlite::Result<()> {
    connection.execute(
        "UPDATE songs SET cover_art = ? WHERE hash = ?",
        params![cover_art, hash],
    )?;
    Ok(())
}

pub fn update_song_instrumental(
    connection: &Connection,
    hash: &str,
    instrumental: bool,
) -> rusqlite::Result<usize> {
    connection.execute(
        "UPDATE songs SET instrumental = ?1 WHERE hash = ?2",
        params![instrumental, hash],
    )
}

pub fn update_song_language(
    connection: &Connection,
    hash: &str,
    language: Option<&str>,
) -> rusqlite::Result<usize> {
    connection.execute(
        "UPDATE songs SET language = ?1 WHERE hash = ?2",
        params![language, hash],
    )
}

fn map_song_row(row: &Row<'_>) -> rusqlite::Result<Song> {
    Ok(Song {
        hash: row.get(0)?,
        file_path: row.get(1)?,
        cdg_path: row.get(2)?,
        media_g_container: row.get(3)?,
        instrumental: row.get(4)?,
        language: row.get(5)?,
        audio_source_kind: row.get(6)?,
        title: row.get(7)?,
        artist: row.get(8)?,
        album: row.get(9)?,
        duration_ms: row.get(10)?,
        cover_art: row.get(11)?,
        imported_at: row.get(12)?,
        original_ext: row.get(13)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_migrations_and_creates_songs_table() {
        let connection = Connection::open_in_memory().expect("in-memory database should open");

        apply_migrations(&connection).expect("migrations should succeed");

        let songs_table_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'songs'",
                [],
                |row| row.get(0),
            )
            .expect("songs table lookup should succeed");

        assert_eq!(songs_table_count, 1);

        let stems_table_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'stems'",
                [],
                |row| row.get(0),
            )
            .expect("stems table lookup should succeed");

        assert_eq!(stems_table_count, 1);

        let instrumental_column_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('songs') WHERE name = 'instrumental'",
                [],
                |row| row.get(0),
            )
            .expect("instrumental column lookup should succeed");

        assert_eq!(instrumental_column_count, 1);

        let lyrics_table_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'lyrics'",
                [],
                |row| row.get(0),
            )
            .expect("lyrics table lookup should succeed");

        assert_eq!(lyrics_table_count, 1);
    }

    #[test]
    fn applies_migrations_idempotently() {
        let connection = Connection::open_in_memory().expect("in-memory database should open");

        apply_migrations(&connection).expect("first migration pass should succeed");
        apply_migrations(&connection).expect("second migration pass should also succeed");
    }

    #[test]
    fn migrates_legacy_song_schema_to_nullable_file_path_and_audio_source_kind() {
        let connection = Connection::open_in_memory().expect("in-memory database should open");

        connection
            .execute_batch(
                "
                CREATE TABLE songs (
                    hash        TEXT PRIMARY KEY,
                    file_path   TEXT NOT NULL,
                    title       TEXT,
                    artist      TEXT,
                    album       TEXT,
                    duration_ms INTEGER,
                    cover_art   BLOB,
                    imported_at INTEGER NOT NULL
                );
                INSERT INTO songs (
                    hash, file_path, title, artist, album, duration_ms, cover_art, imported_at
                ) VALUES (
                    'song-1',
                    'media/song-1.mp3',
                    'Song',
                    'Artist',
                    'Album',
                    1234,
                    X'',
                    1
                );
                ",
            )
            .expect("legacy schema should create");

        apply_migrations(&connection).expect("legacy schema migration should succeed");

        let file_path_nullable: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('songs') WHERE name = 'file_path' AND \"notnull\" = 0",
                [],
                |row| row.get(0),
            )
            .expect("file_path nullability lookup should succeed");
        assert_eq!(file_path_nullable, 1);

        let audio_source_kind_present: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('songs') WHERE name = 'audio_source_kind'",
                [],
                |row| row.get(0),
            )
            .expect("audio_source_kind lookup should succeed");
        assert_eq!(audio_source_kind_present, 1);

        let (file_path, audio_source_kind): (Option<String>, String) = connection
            .query_row(
                "SELECT file_path, audio_source_kind FROM songs WHERE hash = 'song-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("migrated song row should load");

        assert_eq!(file_path.as_deref(), Some("media/song-1.mp3"));
        assert_eq!(audio_source_kind, "original");
    }

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        apply_migrations(&conn).expect("migrations");
        conn
    }

    fn sample_song(hash: &str) -> Song {
        Song {
            hash: hash.to_owned(),
            file_path: Some(format!("media/{hash}.mp3")),
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: "original".to_owned(),
            title: Some("Test Song".to_owned()),
            artist: Some("Test Artist".to_owned()),
            album: Some("Test Album".to_owned()),
            duration_ms: 180_000,
            cover_art: None,
            imported_at: 1000,
            original_ext: Some("mp3".to_owned()),
        }
    }

    #[test]
    fn upsert_and_get_song_round_trip() {
        let conn = test_db();
        let song = sample_song("abc123");

        upsert_song(&conn, &song).expect("upsert should succeed");
        let retrieved = get_song_by_hash(&conn, "abc123")
            .expect("get should succeed")
            .expect("song should exist");

        assert_eq!(retrieved, song);
    }

    #[test]
    fn get_song_by_hash_returns_none_for_missing() {
        let conn = test_db();
        let result = get_song_by_hash(&conn, "nonexistent").expect("get should succeed");
        assert!(result.is_none());
    }

    #[test]
    fn upsert_song_updates_on_conflict() {
        let conn = test_db();
        let mut song = sample_song("abc123");
        upsert_song(&conn, &song).expect("first upsert");

        song.title = Some("Updated Title".to_owned());
        song.duration_ms = 240_000;
        upsert_song(&conn, &song).expect("second upsert");

        let retrieved = get_song_by_hash(&conn, "abc123")
            .expect("get should succeed")
            .expect("song should exist");

        assert_eq!(retrieved.title.as_deref(), Some("Updated Title"));
        assert_eq!(retrieved.duration_ms, 240_000);
    }

    #[test]
    fn list_songs_orders_by_imported_at_desc() {
        let conn = test_db();
        let mut s1 = sample_song("a");
        s1.imported_at = 100;
        s1.title = Some("Alpha".to_owned());
        let mut s2 = sample_song("b");
        s2.imported_at = 300;
        s2.title = Some("Charlie".to_owned());
        let mut s3 = sample_song("c");
        s3.imported_at = 200;
        s3.title = Some("Bravo".to_owned());

        upsert_song(&conn, &s1).unwrap();
        upsert_song(&conn, &s2).unwrap();
        upsert_song(&conn, &s3).unwrap();

        let songs = list_songs(&conn).expect("list should succeed");
        assert_eq!(songs.len(), 3);
        assert_eq!(songs[0].hash, "b"); // imported_at 300
        assert_eq!(songs[1].hash, "c"); // imported_at 200
        assert_eq!(songs[2].hash, "a"); // imported_at 100
    }

    #[test]
    fn list_songs_secondary_sorts_by_title_case_insensitive() {
        let conn = test_db();
        let mut s1 = sample_song("a");
        s1.imported_at = 100;
        s1.title = Some("banana".to_owned());
        let mut s2 = sample_song("b");
        s2.imported_at = 100;
        s2.title = Some("Apple".to_owned());

        upsert_song(&conn, &s1).unwrap();
        upsert_song(&conn, &s2).unwrap();

        let songs = list_songs(&conn).expect("list should succeed");
        assert_eq!(songs[0].title.as_deref(), Some("Apple"));
        assert_eq!(songs[1].title.as_deref(), Some("banana"));
    }

    #[test]
    fn search_songs_matches_title() {
        let conn = test_db();
        let mut s1 = sample_song("a");
        s1.title = Some("Bohemian Rhapsody".to_owned());
        let mut s2 = sample_song("b");
        s2.title = Some("Yesterday".to_owned());

        upsert_song(&conn, &s1).unwrap();
        upsert_song(&conn, &s2).unwrap();

        let results = search_songs(&conn, "rhapsody").expect("search should succeed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].hash, "a");
    }

    #[test]
    fn search_songs_matches_artist() {
        let conn = test_db();
        let mut s1 = sample_song("a");
        s1.artist = Some("The Beatles".to_owned());
        let mut s2 = sample_song("b");
        s2.artist = Some("Queen".to_owned());

        upsert_song(&conn, &s1).unwrap();
        upsert_song(&conn, &s2).unwrap();

        let results = search_songs(&conn, "beatles").expect("search should succeed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].hash, "a");
    }

    #[test]
    fn search_songs_matches_file_path() {
        let conn = test_db();
        let mut s1 = sample_song("a");
        s1.file_path = Some("media/special-track.mp3".to_owned());
        s1.title = Some("Generic".to_owned());
        s1.artist = None;
        s1.album = None;

        upsert_song(&conn, &s1).unwrap();

        let results = search_songs(&conn, "special").expect("search should succeed");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn update_song_title_and_artist() {
        let conn = test_db();
        upsert_song(&conn, &sample_song("abc")).unwrap();

        update_song_title_artist(&conn, "abc", Some("New Title"), Some("New Artist"))
            .expect("update should succeed");

        let song = get_song_by_hash(&conn, "abc").unwrap().unwrap();
        assert_eq!(song.title.as_deref(), Some("New Title"));
        assert_eq!(song.artist.as_deref(), Some("New Artist"));
    }

    #[test]
    fn update_song_cover_art_bytes() {
        let conn = test_db();
        upsert_song(&conn, &sample_song("abc")).unwrap();

        let art = vec![0xFF, 0xD8, 0xFF, 0xE0]; // JPEG magic bytes
        update_song_cover_art(&conn, "abc", Some(&art)).expect("update should succeed");

        let song = get_song_by_hash(&conn, "abc").unwrap().unwrap();
        assert_eq!(song.cover_art.as_deref(), Some(art.as_slice()));
    }

    #[test]
    fn update_song_instrumental_flag() {
        let conn = test_db();
        upsert_song(&conn, &sample_song("abc")).unwrap();

        let affected = update_song_instrumental(&conn, "abc", true).expect("update should succeed");
        assert_eq!(affected, 1);

        let song = get_song_by_hash(&conn, "abc").unwrap().unwrap();
        assert!(song.instrumental);
    }

    #[test]
    fn update_song_language_field() {
        let conn = test_db();
        upsert_song(&conn, &sample_song("abc")).unwrap();

        update_song_language(&conn, "abc", Some("ja")).expect("update should succeed");

        let song = get_song_by_hash(&conn, "abc").unwrap().unwrap();
        assert_eq!(song.language.as_deref(), Some("ja"));
    }

    #[test]
    fn upsert_song_with_null_optional_fields() {
        let conn = test_db();
        let song = Song {
            hash: "minimal".to_owned(),
            file_path: None,
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

        upsert_song(&conn, &song).expect("upsert should succeed");
        let retrieved = get_song_by_hash(&conn, "minimal").unwrap().unwrap();
        assert!(retrieved.file_path.is_none());
        assert!(retrieved.title.is_none());
        assert!(retrieved.artist.is_none());
        assert!(retrieved.album.is_none());
        assert!(retrieved.cover_art.is_none());
        assert!(retrieved.original_ext.is_none());
    }
}
