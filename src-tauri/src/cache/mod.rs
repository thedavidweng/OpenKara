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
const INITIAL_MIGRATION_SQL: &str = include_str!("../../migrations/001_init.sql");

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
    Connection::open(database_path).with_context(|| {
        format!(
            "failed to open SQLite database at {}",
            database_path.display()
        )
    })
}

pub fn apply_migrations(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(INITIAL_MIGRATION_SQL)
}

pub fn upsert_song(connection: &Connection, song: &Song) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO songs (
            hash,
            file_path,
            title,
            artist,
            album,
            duration_ms,
            cover_art,
            imported_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(hash) DO UPDATE SET
            file_path = excluded.file_path,
            title = excluded.title,
            artist = excluded.artist,
            album = excluded.album,
            duration_ms = excluded.duration_ms,
            cover_art = excluded.cover_art,
            imported_at = excluded.imported_at",
        params![
            song.hash,
            song.file_path,
            song.title,
            song.artist,
            song.album,
            song.duration_ms,
            song.cover_art,
            song.imported_at,
        ],
    )?;

    Ok(())
}

pub fn list_songs(connection: &Connection) -> rusqlite::Result<Vec<Song>> {
    let mut statement = connection.prepare(
        "SELECT
            hash,
            file_path,
            title,
            artist,
            album,
            duration_ms,
            cover_art,
            imported_at
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
            title,
            artist,
            album,
            duration_ms,
            cover_art,
            imported_at
        FROM songs
        WHERE lower(coalesce(title, '')) LIKE ?1
           OR lower(coalesce(artist, '')) LIKE ?1
           OR lower(coalesce(album, '')) LIKE ?1
           OR lower(file_path) LIKE ?1
        ORDER BY imported_at DESC, title COLLATE NOCASE ASC, hash ASC",
    )?;

    let songs = statement
        .query_map([pattern], map_song_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(songs)
}

fn map_song_row(row: &Row<'_>) -> rusqlite::Result<Song> {
    Ok(Song {
        hash: row.get(0)?,
        file_path: row.get(1)?,
        title: row.get(2)?,
        artist: row.get(3)?,
        album: row.get(4)?,
        duration_ms: row.get(5)?,
        cover_art: row.get(6)?,
        imported_at: row.get(7)?,
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
    }

    #[test]
    fn applies_migrations_idempotently() {
        let connection = Connection::open_in_memory().expect("in-memory database should open");

        apply_migrations(&connection).expect("first migration pass should succeed");
        apply_migrations(&connection).expect("second migration pass should also succeed");
    }
}
