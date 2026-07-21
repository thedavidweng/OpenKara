//! Library-database publish outbox.
//!
//! The change set for a remote publish is written into the **library** SQLite
//! database in the same transaction as the local song mutation. Startup
//! recovery can then rebuild a missing control-DB operation from this outbox
//! when the process dies between the library commit and the remote-state.db
//! projection.

use crate::commands::error::{database_error, CommandResult};
use rusqlite::{params, Connection, OptionalExtension};

/// One durable publish intent stored next to the songs it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryPublishOutboxRow {
    pub operation_id: String,
    pub song_ids: Vec<String>,
    pub expected_generation: Option<i64>,
    pub source_db_digest: Option<String>,
    pub created_at_ms: i64,
    pub projected_at_ms: Option<i64>,
}

/// Insert or replace an outbox row. Caller must hold a library DB transaction
/// that also contains the song mutation.
pub fn upsert_library_publish_outbox(
    connection: &Connection,
    row: &LibraryPublishOutboxRow,
) -> CommandResult<()> {
    let song_ids_json = serde_json::to_string(&row.song_ids)
        .map_err(|e| database_error(format!("failed to serialize outbox song_ids: {e}")))?;
    connection
        .execute(
            "INSERT INTO remote_publish_outbox (
                operation_id, song_ids_json, expected_generation, source_db_digest,
                created_at_ms, projected_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(operation_id) DO UPDATE SET
                song_ids_json = excluded.song_ids_json,
                expected_generation = excluded.expected_generation,
                source_db_digest = excluded.source_db_digest,
                created_at_ms = excluded.created_at_ms,
                projected_at_ms = excluded.projected_at_ms",
            params![
                row.operation_id,
                song_ids_json,
                row.expected_generation,
                row.source_db_digest,
                row.created_at_ms,
                row.projected_at_ms,
            ],
        )
        .map_err(|e| database_error(format!("failed to upsert library publish outbox: {e}")))?;
    Ok(())
}

/// Mark an outbox row as projected into remote-state.db.
pub fn mark_library_outbox_projected(
    connection: &Connection,
    operation_id: &str,
    projected_at_ms: i64,
) -> CommandResult<()> {
    connection
        .execute(
            "UPDATE remote_publish_outbox SET projected_at_ms = ?1 WHERE operation_id = ?2",
            params![projected_at_ms, operation_id],
        )
        .map_err(|e| database_error(format!("failed to mark outbox projected: {e}")))?;
    Ok(())
}

/// Load a single outbox row.
#[allow(dead_code)]
pub fn get_library_publish_outbox(
    connection: &Connection,
    operation_id: &str,
) -> CommandResult<Option<LibraryPublishOutboxRow>> {
    connection
        .query_row(
            "SELECT operation_id, song_ids_json, expected_generation, source_db_digest,
                    created_at_ms, projected_at_ms
             FROM remote_publish_outbox WHERE operation_id = ?1",
            params![operation_id],
            map_outbox_row,
        )
        .optional()
        .map_err(|e| database_error(format!("failed to load library publish outbox: {e}")))
}

/// List outbox rows that have not yet been projected into the control DB.
pub fn list_unprojected_library_outbox(
    connection: &Connection,
) -> CommandResult<Vec<LibraryPublishOutboxRow>> {
    let mut stmt = connection
        .prepare(
            "SELECT operation_id, song_ids_json, expected_generation, source_db_digest,
                    created_at_ms, projected_at_ms
             FROM remote_publish_outbox
             WHERE projected_at_ms IS NULL
             ORDER BY created_at_ms ASC",
        )
        .map_err(|e| database_error(format!("failed to prepare unprojected outbox query: {e}")))?;
    let rows = stmt
        .query_map([], map_outbox_row)
        .map_err(|e| database_error(format!("failed to query unprojected outbox: {e}")))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| database_error(format!("failed to collect unprojected outbox: {e}")))?;
    Ok(rows)
}

fn map_outbox_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LibraryPublishOutboxRow> {
    let song_ids_json: String = row.get(1)?;
    let song_ids: Vec<String> = serde_json::from_str(&song_ids_json).unwrap_or_default();
    Ok(LibraryPublishOutboxRow {
        operation_id: row.get(0)?,
        song_ids,
        expected_generation: row.get(2)?,
        source_db_digest: row.get(3)?,
        created_at_ms: row.get(4)?,
        projected_at_ms: row.get(5)?,
    })
}
