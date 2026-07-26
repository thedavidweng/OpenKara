pub mod lyrics;
pub mod stems;
pub mod waveforms;

use crate::library::Song;
use anyhow::Context;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;

const DATABASE_FILENAME: &str = "openkara.sqlite3";
// Keep the SQL in the migrations directory so tests and runtime initialization
// execute the exact same schema definition.
const MIGRATIONS: [&str; 6] = [
    include_str!("../../migrations/001_init.sql"),
    include_str!("../../migrations/002_stems.sql"),
    include_str!("../../migrations/003_lyrics.sql"),
    include_str!("../../migrations/004_portable_paths.sql"),
    include_str!("../../migrations/010_fts5_songs.sql"),
    include_str!("../../migrations/011_waveforms.sql"),
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

    if !column_exists(connection, "stems", "drums_path")? {
        connection.execute_batch("ALTER TABLE stems ADD COLUMN drums_path TEXT;")?;
    }
    if !column_exists(connection, "stems", "bass_path")? {
        connection.execute_batch("ALTER TABLE stems ADD COLUMN bass_path TEXT;")?;
    }
    if !column_exists(connection, "stems", "other_path")? {
        connection.execute_batch("ALTER TABLE stems ADD COLUMN other_path TEXT;")?;
    }

    if !column_exists(connection, "stems", "model_variant")? {
        connection
            .execute_batch("ALTER TABLE stems ADD COLUMN model_variant TEXT DEFAULT 'htdemucs';")?;
    }

    migrate_legacy_song_schema(connection)?;

    // 012_artwork_derivatives: applied after migrate_legacy_song_schema because
    // that rebuild recreates the songs table and would discard columns added
    // earlier.
    if !column_exists(connection, "songs", "artwork_thumb_path")? {
        connection.execute_batch("ALTER TABLE songs ADD COLUMN artwork_thumb_path TEXT;")?;
    }
    if !column_exists(connection, "songs", "artwork_preview_path")? {
        connection.execute_batch("ALTER TABLE songs ADD COLUMN artwork_preview_path TEXT;")?;
    }

    connection.execute_batch(include_str!("../../migrations/008_playlists.sql"))?;
    connection.execute_batch(include_str!("../../migrations/009_singer_rotation.sql"))?;
    // Durable publish change-set for crash recovery across control-DB projection.
    connection.execute_batch(include_str!(
        "../../migrations/013_remote_publish_outbox.sql"
    ))?;

    Ok(())
}

pub(crate) fn column_exists(
    connection: &Connection,
    table: &str,
    column: &str,
) -> rusqlite::Result<bool> {
    if table.is_empty()
        || table.chars().next().is_some_and(|c| c.is_ascii_digit())
        || !table.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "invalid table identifier: {table}"
        )));
    }
    let sql = format!("PRAGMA table_info(\"{table}\")");
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
            language           TEXT,
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
            language,
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
            language,
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
            CASE WHEN cover_art IS NOT NULL THEN 1 ELSE 0 END,
            imported_at,
            original_ext,
            artwork_thumb_path
        FROM songs
        ORDER BY imported_at DESC, title COLLATE NOCASE ASC, hash ASC",
    )?;

    let songs = statement
        .query_map([], map_song_row_no_cover_art)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(songs)
}

pub fn search_songs(connection: &Connection, query: &str) -> rusqlite::Result<Vec<Song>> {
    // FTS5 prefix queries use unquoted terms with trailing * (e.g. "bohem*").
    // Quoted terms are treated as literal phrase searches.
    let fts_query = if let Some(prefix) = query.strip_suffix('*') {
        format!("{}*", prefix.replace('"', ""))
    } else {
        format!("\"{}\"", query.replace('"', "\"\""))
    };
    let mut statement = connection.prepare(
        "SELECT
            s.hash,
            s.file_path,
            s.cdg_path,
            s.media_g_container,
            s.instrumental,
            s.language,
            s.audio_source_kind,
            s.title,
            s.artist,
            s.album,
            s.duration_ms,
            CASE WHEN s.cover_art IS NOT NULL THEN 1 ELSE 0 END,
            s.imported_at,
            s.original_ext,
            s.artwork_thumb_path
        FROM songs s
        INNER JOIN songs_fts fts ON fts.rowid = s.rowid
        WHERE songs_fts MATCH ?1
        ORDER BY s.imported_at DESC, s.title COLLATE NOCASE ASC, s.hash ASC",
    )?;

    let songs = statement
        .query_map([fts_query], map_song_row_no_cover_art)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(songs)
}

pub fn get_cover_art(connection: &Connection, hash: &str) -> rusqlite::Result<Option<Vec<u8>>> {
    let mut statement =
        connection.prepare("SELECT cover_art FROM songs WHERE hash = ?1 LIMIT 1")?;

    let mut rows = statement.query([hash])?;
    match rows.next()? {
        Some(row) => Ok(row.get(0)?),
        None => Ok(None),
    }
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
            original_ext,
            artwork_thumb_path
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

/// Replace a song's cover art and derivative paths together. Returns the
/// previous derivative paths so the caller can clean up on-disk files whose
/// digest changed after the write commits. The original bytes and both
/// derivative paths are written by one atomic SQLite UPDATE, so a crash cannot
/// leave a row with new paths referencing unwritten files (or new bytes with
/// stale paths). The previous paths are read before that UPDATE so the caller
/// can delete old derivative files afterwards.
pub fn replace_cover_art_and_derivatives(
    connection: &Connection,
    hash: &str,
    cover_art: Option<&[u8]>,
    thumb_path: Option<&str>,
    preview_path: Option<&str>,
) -> rusqlite::Result<(Option<String>, Option<String>)> {
    let (old_thumb, old_preview) = connection
        .query_row(
            "SELECT artwork_thumb_path, artwork_preview_path FROM songs WHERE hash = ?1",
            [hash],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .unwrap_or((None, None));
    connection.execute(
        "UPDATE songs SET cover_art = ?, artwork_thumb_path = ?, artwork_preview_path = ? WHERE hash = ?",
        params![cover_art, thumb_path, preview_path, hash],
    )?;
    Ok((old_thumb, old_preview))
}

/// Internal record of cover art bytes and their derivative file paths.
/// Used by artwork generation/lazy-repair paths to avoid a full `Song` read
/// and to keep derivative paths out of Rust/TypeScript IPC.
#[derive(Debug, Clone, Default)]
pub struct ArtworkRecord {
    pub cover_art: Option<Vec<u8>>,
    pub artwork_thumb_path: Option<String>,
    pub artwork_preview_path: Option<String>,
}

pub fn get_artwork_record(
    connection: &Connection,
    hash: &str,
) -> rusqlite::Result<Option<ArtworkRecord>> {
    let mut statement = connection.prepare(
        "SELECT cover_art, artwork_thumb_path, artwork_preview_path
         FROM songs WHERE hash = ?1 LIMIT 1",
    )?;
    let mut rows = statement.query([hash])?;
    match rows.next()? {
        Some(row) => Ok(Some(ArtworkRecord {
            cover_art: row.get(0)?,
            artwork_thumb_path: row.get(1)?,
            artwork_preview_path: row.get(2)?,
        })),
        None => Ok(None),
    }
}

pub fn update_artwork_derivative_paths(
    connection: &Connection,
    hash: &str,
    thumb_path: Option<&str>,
    preview_path: Option<&str>,
) -> rusqlite::Result<()> {
    connection.execute(
        "UPDATE songs SET artwork_thumb_path = ?, artwork_preview_path = ? WHERE hash = ?",
        params![thumb_path, preview_path, hash],
    )?;
    Ok(())
}

/// Update derivative paths only if the song's `cover_art` BLOB still equals
/// `expected_cover_art` (the original bytes the derivatives were generated from).
/// Implemented as a single conditional UPDATE so it is atomic with respect to
/// concurrent writers without needing a mutable connection handle. Returns
/// true if the update was applied, false if the cover art changed (concurrent
/// cover-art replacement) or the row was deleted. This prevents a lazy repair
/// from overwriting a newer cover's derivative paths with stale ones generated
/// from older bytes. Comparing the raw BLOB avoids needing a separate digest
/// column and is a direct byte-for-byte identity check.
pub fn update_artwork_derivative_paths_if_cover_matches(
    connection: &Connection,
    hash: &str,
    thumb_path: Option<&str>,
    preview_path: Option<&str>,
    expected_cover_art: &[u8],
) -> rusqlite::Result<bool> {
    let changed = connection.execute(
        "UPDATE songs
         SET artwork_thumb_path = ?, artwork_preview_path = ?
         WHERE hash = ? AND cover_art = ?",
        params![thumb_path, preview_path, hash, expected_cover_art],
    )?;
    Ok(changed > 0)
}

/// Count how many song rows reference a given artwork derivative path.
/// Used by deletion to avoid removing a derivative file still referenced
/// by another song row (e.g. two songs sharing the same cover art bytes).
pub fn count_artwork_path_references(
    connection: &Connection,
    path: &str,
) -> rusqlite::Result<usize> {
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM songs
         WHERE artwork_thumb_path = ?1 OR artwork_preview_path = ?1",
        [path],
        |row| row.get(0),
    )?;
    Ok(count as usize)
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
    let cover_art: Option<Vec<u8>> = row.get(11)?;
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
        has_cover_art: cover_art.is_some(),
        artwork_thumb_path: row.get(14)?,
        cover_art,
        imported_at: row.get(12)?,
        original_ext: row.get(13)?,
    })
}

/// Map a song row for list/search queries where cover_art BLOB is excluded.
/// The query must select `CASE WHEN cover_art IS NOT NULL THEN 1 ELSE 0 END`
/// at column index 11 instead of the raw `cover_art` blob, and
/// `artwork_thumb_path` at column index 14.
fn map_song_row_no_cover_art(row: &Row<'_>) -> rusqlite::Result<Song> {
    let has_cover_art: bool = row.get(11)?;
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
        has_cover_art,
        artwork_thumb_path: row.get(14)?,
        cover_art: None,
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

        for column in ["artwork_thumb_path", "artwork_preview_path"] {
            let count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('songs') WHERE name = ?1",
                    [column],
                    |row| row.get(0),
                )
                .expect("artwork derivative column lookup should succeed");
            assert_eq!(count, 1, "missing {column}");
        }
    }

    #[test]
    fn applies_migrations_idempotently() {
        let connection = Connection::open_in_memory().expect("in-memory database should open");

        apply_migrations(&connection).expect("first migration pass should succeed");
        apply_migrations(&connection).expect("second migration pass should also succeed");
    }

    #[test]
    fn column_exists_quotes_valid_identifiers_and_rejects_invalid_ones() {
        let connection = Connection::open_in_memory().expect("in-memory database should open");
        apply_migrations(&connection).expect("migrations should succeed");

        assert!(column_exists(&connection, "songs", "hash").unwrap());
        assert!(!column_exists(&connection, "songs", "not_a_column").unwrap());
        for invalid in [
            "",
            "1songs",
            "songs; DROP TABLE songs",
            "songs\"",
            "song-name",
        ] {
            assert!(column_exists(&connection, invalid, "hash").is_err());
        }
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

    /// Regression for #219: the legacy-schema rebuild must carry `language`
    /// across the table recreation. A pre-1.0 database that already holds
    /// per-song language annotations must keep them after the rebuild, and the
    /// column-dependent `list_songs` query must still succeed.
    #[test]
    fn migrate_legacy_song_schema_preserves_language() {
        let connection = Connection::open_in_memory().expect("in-memory database should open");

        // Legacy 0.x shape that triggers the rebuild: file_path is NOT NULL and
        // audio_source_kind is absent, but the row already carries a populated
        // `language` column (added by an earlier inline ALTER in the wild).
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
                    imported_at INTEGER NOT NULL,
                    language    TEXT
                );
                INSERT INTO songs (
                    hash, file_path, title, artist, album, duration_ms, cover_art, imported_at, language
                ) VALUES (
                    'song-ja',
                    'media/song-ja.mp3',
                    'Song',
                    'Artist',
                    'Album',
                    1234,
                    X'',
                    1,
                    'ja'
                );
                ",
            )
            .expect("legacy schema with language data should create");

        apply_migrations(&connection).expect("legacy schema migration should succeed");

        // The rebuild must not silently drop the per-song language value.
        let language: Option<String> = connection
            .query_row(
                "SELECT language FROM songs WHERE hash = 'song-ja'",
                [],
                |row| row.get(0),
            )
            .expect("migrated song row should load");
        assert_eq!(language.as_deref(), Some("ja"));

        // list_songs selects the `language` column; before the fix the rebuilt
        // table lacked it and this query failed with "no such column: language".
        let songs = list_songs(&connection).expect("list_songs should succeed after rebuild");
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].language.as_deref(), Some("ja"));
    }

    /// Regression for #219: after the legacy rebuild the songs table must expose
    /// exactly the same set of columns as a freshly initialized database, so no
    /// column (language included) is dropped by the recreation.
    #[test]
    fn legacy_rebuild_matches_fresh_install_column_set() {
        fn song_columns(connection: &Connection) -> Vec<String> {
            let mut names: Vec<String> = connection
                .prepare("PRAGMA table_info(songs)")
                .expect("pragma prepare")
                .query_map([], |row| row.get::<_, String>(1))
                .expect("pragma query")
                .collect::<rusqlite::Result<Vec<_>>>()
                .expect("pragma rows");
            names.sort();
            names
        }

        // Fresh install: apply migrations against an empty database.
        let fresh = Connection::open_in_memory().expect("fresh in-memory database should open");
        apply_migrations(&fresh).expect("fresh migrations should succeed");
        let fresh_columns = song_columns(&fresh);

        // Legacy upgrade: a 0.x database that forces migrate_legacy_song_schema
        // to rebuild the songs table.
        let legacy = Connection::open_in_memory().expect("legacy in-memory database should open");
        legacy
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
                ",
            )
            .expect("legacy schema should create");
        apply_migrations(&legacy).expect("legacy migrations should succeed");
        let legacy_columns = song_columns(&legacy);

        assert_eq!(
            legacy_columns, fresh_columns,
            "rebuilt songs table columns must match a fresh install"
        );
        assert!(
            legacy_columns.iter().any(|name| name == "language"),
            "language column must be present after the legacy rebuild"
        );
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
            has_cover_art: false,
            artwork_thumb_path: None,
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

    /// search_songs should use FTS5 MATCH, not LIKE %q%.
    /// FTS5 MATCH requires an exact FTS5 table to exist.
    #[test]
    fn search_songs_uses_fts5_index() {
        let conn = test_db();

        let fts_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'songs_fts'",
                [],
                |row| row.get(0),
            )
            .expect("fts5 table lookup");
        assert_eq!(fts_count, 1, "songs_fts FTS5 virtual table should exist");
    }

    /// FTS5 search should match partial terms via prefix queries.
    #[test]
    fn search_songs_fts5_prefix_matching() {
        let conn = test_db();
        let mut s1 = sample_song("fts-test");
        s1.title = Some("Bohemian Rhapsody".to_owned());
        upsert_song(&conn, &s1).unwrap();

        let results = search_songs(&conn, "bohem*").expect("search should succeed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].hash, "fts-test");
    }

    /// FTS5 search should stay in sync after updates.
    #[test]
    fn search_songs_fts5_syncs_after_update() {
        let conn = test_db();
        let mut s1 = sample_song("fts-sync");
        s1.title = Some("Original Title".to_owned());
        upsert_song(&conn, &s1).unwrap();

        let results = search_songs(&conn, "original").expect("search should succeed");
        assert_eq!(results.len(), 1);

        update_song_title_artist(&conn, "fts-sync", Some("New Title"), None).unwrap();

        let results = search_songs(&conn, "original").expect("search should succeed");
        assert_eq!(results.len(), 0);

        let results = search_songs(&conn, "new title").expect("search should succeed");
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
            has_cover_art: false,
            artwork_thumb_path: None,
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

    /// list_songs should not include cover_art BLOB in IPC payload.
    /// Instead, has_cover_art indicates whether cover art exists.
    #[test]
    fn list_songs_excludes_cover_art_blob() {
        let conn = test_db();
        let mut song = sample_song("cover-test");
        song.cover_art = Some(vec![0xFF, 0xD8, 0xFF, 0xE0]); // JPEG magic bytes
        upsert_song(&conn, &song).expect("upsert with cover art");

        let songs = list_songs(&conn).expect("list should succeed");
        assert_eq!(songs.len(), 1);
        assert!(
            songs[0].cover_art.is_none(),
            "cover_art BLOB should not be in list results"
        );
        assert!(
            songs[0].has_cover_art,
            "has_cover_art should be true when cover art exists"
        );
    }

    /// list_songs with no cover art should report has_cover_art = false.
    #[test]
    fn list_songs_reports_no_cover_art() {
        let conn = test_db();
        let song = sample_song("no-cover");
        upsert_song(&conn, &song).expect("upsert without cover art");

        let songs = list_songs(&conn).expect("list should succeed");
        assert_eq!(songs.len(), 1);
        assert!(songs[0].cover_art.is_none());
        assert!(
            !songs[0].has_cover_art,
            "has_cover_art should be false when no cover art"
        );
    }

    /// search_songs should also exclude cover_art BLOB.
    #[test]
    fn search_songs_excludes_cover_art_blob() {
        let conn = test_db();
        let mut song = sample_song("search-cover");
        song.title = Some("Unique Searchable Title".to_owned());
        song.cover_art = Some(vec![0x89, 0x50, 0x4E, 0x47]); // PNG magic bytes
        upsert_song(&conn, &song).expect("upsert with cover art");

        let results = search_songs(&conn, "unique searchable").expect("search should succeed");
        assert_eq!(results.len(), 1);
        assert!(
            results[0].cover_art.is_none(),
            "search results should not include cover_art BLOB"
        );
        assert!(results[0].has_cover_art, "has_cover_art should be true");
    }

    /// get_cover_art returns the raw BLOB on demand.
    #[test]
    fn get_cover_art_returns_blob_on_demand() {
        let conn = test_db();
        let mut song = sample_song("art-demand");
        let art = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        song.cover_art = Some(art.clone());
        upsert_song(&conn, &song).expect("upsert with cover art");

        let result = get_cover_art(&conn, "art-demand").expect("query should succeed");
        assert_eq!(result.as_deref(), Some(art.as_slice()));
    }

    /// get_cover_art returns None for songs without cover art.
    #[test]
    fn get_cover_art_returns_none_for_missing() {
        let conn = test_db();
        upsert_song(&conn, &sample_song("no-art")).expect("upsert");

        let result = get_cover_art(&conn, "no-art").expect("query should succeed");
        assert!(result.is_none());
    }

    #[test]
    fn derivative_path_update_requires_the_original_cover_bytes() {
        let conn = test_db();
        let mut song = sample_song("conditional-artwork");
        let original = b"original cover".to_vec();
        song.cover_art = Some(original.clone());
        upsert_song(&conn, &song).expect("upsert");

        assert!(update_artwork_derivative_paths_if_cover_matches(
            &conn,
            &song.hash,
            Some("artwork/thumb_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa_80.webp"),
            Some("artwork/preview_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa_256.webp"),
            &original,
        )
        .expect("matching original should update"));

        update_song_cover_art(&conn, &song.hash, Some(b"replacement cover"))
            .expect("replace cover");
        assert!(!update_artwork_derivative_paths_if_cover_matches(
            &conn,
            &song.hash,
            Some("artwork/thumb_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb_80.webp"),
            Some("artwork/preview_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb_256.webp"),
            &original,
        )
        .expect("stale original should not update"));

        let record = get_artwork_record(&conn, &song.hash)
            .expect("read record")
            .expect("song exists");
        assert_eq!(
            record.artwork_thumb_path.as_deref(),
            Some("artwork/thumb_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa_80.webp")
        );
    }

    /// Item 1: get_song_by_hash still returns full cover_art.
    #[test]
    fn get_song_by_hash_still_returns_cover_art() {
        let conn = test_db();
        let mut song = sample_song("full-art");
        let art = vec![0xFF, 0xD8];
        song.cover_art = Some(art.clone());
        upsert_song(&conn, &song).expect("upsert");

        let retrieved = get_song_by_hash(&conn, "full-art").unwrap().unwrap();
        assert_eq!(retrieved.cover_art.as_deref(), Some(art.as_slice()));
        assert!(retrieved.has_cover_art);
    }
}
