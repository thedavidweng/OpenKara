use crate::error::{RemoteError, RemoteErrorKind, RemoteResult};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;

/// One row of the media cache catalog.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub cache_key: String,
    pub library_id: String,
    pub relative_path: String,
    pub provider_revision: Option<String>,
    pub content_digest: Option<String>,
    pub expected_size: i64,
    pub downloaded_ranges_json: String,
    pub complete: bool,
    /// Active playback leases. Persisted as the `pinned_count` column.
    pub lease_count: i64,
    pub last_access_at_ms: i64,
    pub verified_at_ms: Option<i64>,
    pub data_path: String,
}

// The desktop control database created `remote_cache_entries` in its first
// migration; this definition must stay column-compatible with it.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS remote_cache_entries (
  cache_key TEXT PRIMARY KEY,
  library_id TEXT NOT NULL,
  relative_path TEXT NOT NULL,
  provider_revision TEXT,
  content_digest TEXT,
  expected_size INTEGER NOT NULL,
  downloaded_ranges_json TEXT NOT NULL,
  complete INTEGER NOT NULL,
  pinned_count INTEGER NOT NULL DEFAULT 0,
  last_access_at_ms INTEGER NOT NULL,
  verified_at_ms INTEGER,
  data_path TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS remote_cache_pins (
  library_id TEXT NOT NULL,
  song_id TEXT NOT NULL,
  relative_path TEXT NOT NULL,
  pinned_at_ms INTEGER NOT NULL,
  PRIMARY KEY (library_id, song_id, relative_path)
);
";

const ENTRY_COLUMNS: &str =
    "cache_key, library_id, relative_path, provider_revision, content_digest,
    expected_size, downloaded_ranges_json, complete, pinned_count,
    last_access_at_ms, verified_at_ms, data_path";

pub(super) fn storage_error(context: &str, error: impl std::fmt::Display) -> RemoteError {
    RemoteError::new(
        RemoteErrorKind::LocalStorageFailed,
        format!("{context}: {error}"),
    )
}

pub(super) fn ensure_schema(connection: &Connection) -> RemoteResult<()> {
    connection
        .execute_batch(SCHEMA)
        .map_err(|e| storage_error("failed to prepare the media cache catalog", e))
}

pub(super) fn upsert_entry(connection: &Connection, row: &CacheEntry) -> RemoteResult<()> {
    connection
        .execute(
            "INSERT INTO remote_cache_entries (
                cache_key, library_id, relative_path, provider_revision, content_digest,
                expected_size, downloaded_ranges_json, complete, pinned_count,
                last_access_at_ms, verified_at_ms, data_path
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(cache_key) DO UPDATE SET
                library_id = excluded.library_id,
                relative_path = excluded.relative_path,
                provider_revision = excluded.provider_revision,
                content_digest = excluded.content_digest,
                expected_size = excluded.expected_size,
                downloaded_ranges_json = excluded.downloaded_ranges_json,
                complete = excluded.complete,
                pinned_count = excluded.pinned_count,
                last_access_at_ms = excluded.last_access_at_ms,
                verified_at_ms = excluded.verified_at_ms,
                data_path = excluded.data_path",
            params![
                row.cache_key,
                row.library_id,
                row.relative_path,
                row.provider_revision,
                row.content_digest,
                row.expected_size,
                row.downloaded_ranges_json,
                row.complete,
                row.lease_count,
                row.last_access_at_ms,
                row.verified_at_ms,
                row.data_path,
            ],
        )
        .map_err(|e| storage_error("failed to upsert cache entry", e))?;
    Ok(())
}

/// Look up one catalog row by its cache key.
pub fn read_entry(connection: &Connection, cache_key: &str) -> RemoteResult<Option<CacheEntry>> {
    connection
        .query_row(
            &format!("SELECT {ENTRY_COLUMNS} FROM remote_cache_entries WHERE cache_key = ?1"),
            [cache_key],
            map_entry,
        )
        .optional()
        .map_err(|e| storage_error("failed to query cache entry", e))
}

pub(super) fn list_entries(connection: &Connection) -> RemoteResult<Vec<CacheEntry>> {
    let mut stmt = connection
        .prepare(&format!("SELECT {ENTRY_COLUMNS} FROM remote_cache_entries"))
        .map_err(|e| storage_error("failed to prepare cache entry list", e))?;
    let rows = stmt
        .query_map([], map_entry)
        .map_err(|e| storage_error("failed to list cache entries", e))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| storage_error("failed to collect cache entries", e))?;
    Ok(rows)
}

pub(super) fn delete_entry(connection: &Connection, cache_key: &str) -> RemoteResult<()> {
    connection
        .execute(
            "DELETE FROM remote_cache_entries WHERE cache_key = ?1",
            params![cache_key],
        )
        .map_err(|e| storage_error("failed to delete cache entry", e))?;
    Ok(())
}

pub(super) fn update_entry_ranges(
    connection: &Connection,
    cache_key: &str,
    downloaded_ranges_json: &str,
    complete: bool,
    content_digest: Option<&str>,
    verified_at_ms: Option<i64>,
) -> RemoteResult<()> {
    connection
        .execute(
            "UPDATE remote_cache_entries SET
                downloaded_ranges_json = ?2,
                complete = ?3,
                content_digest = COALESCE(?4, content_digest),
                verified_at_ms = ?5
             WHERE cache_key = ?1",
            params![
                cache_key,
                downloaded_ranges_json,
                complete,
                content_digest,
                verified_at_ms,
            ],
        )
        .map_err(|e| storage_error("failed to update cache entry ranges", e))?;
    Ok(())
}

pub(super) fn touch_entry_access(
    connection: &Connection,
    cache_key: &str,
    last_access_at_ms: i64,
) -> RemoteResult<()> {
    connection
        .execute(
            "UPDATE remote_cache_entries SET last_access_at_ms = ?2 WHERE cache_key = ?1",
            params![cache_key, last_access_at_ms],
        )
        .map_err(|e| storage_error("failed to touch cache entry access", e))?;
    Ok(())
}

pub(super) fn add_lease(connection: &Connection, cache_key: &str) -> RemoteResult<()> {
    connection
        .execute(
            "UPDATE remote_cache_entries SET pinned_count = pinned_count + 1 WHERE cache_key = ?1",
            params![cache_key],
        )
        .map_err(|e| storage_error("failed to lease cache entry", e))?;
    Ok(())
}

pub(super) fn release_lease(connection: &Connection, cache_key: &str) -> RemoteResult<()> {
    connection
        .execute(
            "UPDATE remote_cache_entries SET pinned_count = MAX(0, pinned_count - 1)
             WHERE cache_key = ?1",
            params![cache_key],
        )
        .map_err(|e| storage_error("failed to release cache entry lease", e))?;
    Ok(())
}

pub(super) fn replace_song_pin(
    connection: &Connection,
    library_id: &str,
    song_id: &str,
    relative_paths: &[String],
    pinned_at_ms: i64,
) -> RemoteResult<()> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(|e| storage_error("failed to begin offline pin update", e))?;
    transaction
        .execute(
            "DELETE FROM remote_cache_pins WHERE library_id = ?1 AND song_id = ?2",
            params![library_id, song_id],
        )
        .map_err(|e| storage_error("failed to replace offline pin", e))?;
    for relative_path in relative_paths {
        transaction
            .execute(
                "INSERT OR IGNORE INTO remote_cache_pins
                    (library_id, song_id, relative_path, pinned_at_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                params![library_id, song_id, relative_path, pinned_at_ms],
            )
            .map_err(|e| storage_error("failed to record offline pin", e))?;
    }
    transaction
        .commit()
        .map_err(|e| storage_error("failed to commit offline pin", e))
}

pub(super) fn delete_song_pin(
    connection: &Connection,
    library_id: &str,
    song_id: &str,
) -> RemoteResult<()> {
    connection
        .execute(
            "DELETE FROM remote_cache_pins WHERE library_id = ?1 AND song_id = ?2",
            params![library_id, song_id],
        )
        .map_err(|e| storage_error("failed to remove offline pin", e))?;
    Ok(())
}

pub(super) fn list_pinned_songs(
    connection: &Connection,
    library_id: &str,
) -> RemoteResult<Vec<String>> {
    let mut stmt = connection
        .prepare(
            "SELECT DISTINCT song_id FROM remote_cache_pins WHERE library_id = ?1
             ORDER BY song_id",
        )
        .map_err(|e| storage_error("failed to prepare offline pin list", e))?;
    let songs = stmt
        .query_map([library_id], |row| row.get(0))
        .map_err(|e| storage_error("failed to list offline pins", e))?
        .collect::<rusqlite::Result<Vec<String>>>()
        .map_err(|e| storage_error("failed to collect offline pins", e))?;
    Ok(songs)
}

/// `(library_id, relative_path)` of every asset that belongs to a pinned song.
pub(super) fn list_pinned_assets(
    connection: &Connection,
) -> RemoteResult<HashSet<(String, String)>> {
    let mut stmt = connection
        .prepare("SELECT DISTINCT library_id, relative_path FROM remote_cache_pins")
        .map_err(|e| storage_error("failed to prepare pinned asset list", e))?;
    let assets = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| storage_error("failed to list pinned assets", e))?
        .collect::<rusqlite::Result<HashSet<(String, String)>>>()
        .map_err(|e| storage_error("failed to collect pinned assets", e))?;
    Ok(assets)
}

fn map_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<CacheEntry> {
    Ok(CacheEntry {
        cache_key: row.get(0)?,
        library_id: row.get(1)?,
        relative_path: row.get(2)?,
        provider_revision: row.get(3)?,
        content_digest: row.get(4)?,
        expected_size: row.get(5)?,
        downloaded_ranges_json: row.get(6)?,
        complete: row.get::<_, i64>(7)? != 0,
        lease_count: row.get(8)?,
        last_access_at_ms: row.get(9)?,
        verified_at_ms: row.get(10)?,
        data_path: row.get(11)?,
    })
}
