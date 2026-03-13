use anyhow::Context;
use rusqlite::Connection;
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
    let connection = Connection::open(&database_path).with_context(|| {
        format!(
            "failed to open SQLite database at {}",
            database_path.display()
        )
    })?;

    apply_migrations(&connection).context("failed to apply SQLite migrations")?;

    Ok(database_path)
}

pub fn apply_migrations(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(INITIAL_MIGRATION_SQL)
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
