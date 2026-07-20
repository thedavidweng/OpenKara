//! Durable local control-plane database for remote repository state.
//!
//! This database (`<app-data>/remote-state.db`) is the authoritative local
//! record of remote operation/outbox state, repository cleanliness, resumable
//! transfer offsets, and the verified cache catalog. It stays outside every
//! portable library and is NEVER uploaded to a cloud provider.
//!
//! ## Why a separate database
//!
//! Library SQLite databases are portable: they travel inside the remote
//! repository working copy and are synced to cloud storage. Persisting
//! operation state, retry schedules, or cache metadata there would leak
//! machine-local concerns into the portable set and risk clobbering another
//! device's control state on sync. A dedicated local-only database keeps the
//! control plane private to this machine while the library DB remains the
//! shared content address.
//!
//! ## Storage safety
//!
//! Only sanitized machine-readable error codes are persisted. OAuth access
//! tokens, passwords, request URLs containing credentials, and raw provider
//! responses are never written here.
//!
//! ## Concurrency
//!
//! The control DB is opened with WAL journal mode so concurrent readers (e.g.
//! `get_all_upload_statuses`) do not block the single writer that transitions
//! operation state. All state transitions are committed transactionally.

use crate::commands::error::{database_error, internal_error, CommandError, CommandResult};
use crate::hash;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// SQL migrations for the remote control database, kept in a subdirectory so
/// they stay separate from portable library DB migrations.
///
/// Migration 001 uses `CREATE TABLE IF NOT EXISTS` (idempotent). Migration 002
/// uses `ALTER TABLE ADD COLUMN` which is NOT idempotent in SQLite, so it is
/// applied programmatically by [`apply_migration_002_manifest_columns`].
const REMOTE_STATE_MIGRATIONS: [&str; 1] =
    [include_str!("../../migrations/remote_state/001_init.sql")];

/// Filename of the control database inside the app data directory.
pub const CONTROL_DB_FILENAME: &str = "remote-state.db";

// ---------------------------------------------------------------------------
// Typed enums mirroring the CHECK constraints
// ---------------------------------------------------------------------------

/// Repository cleanliness state for a remote library.
///
/// Mirrors the `local_state` CHECK constraint on `remote_repository_state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalState {
    Clean,
    Dirty,
    Publishing,
    Conflicted,
    ReauthRequired,
}

impl LocalState {
    pub fn as_str(self) -> &'static str {
        match self {
            LocalState::Clean => "clean",
            LocalState::Dirty => "dirty",
            LocalState::Publishing => "publishing",
            LocalState::Conflicted => "conflicted",
            LocalState::ReauthRequired => "reauth_required",
        }
    }

    fn from_db(value: &str) -> Result<Self, CommandError> {
        match value {
            "clean" => Ok(LocalState::Clean),
            "dirty" => Ok(LocalState::Dirty),
            "publishing" => Ok(LocalState::Publishing),
            "conflicted" => Ok(LocalState::Conflicted),
            "reauth_required" => Ok(LocalState::ReauthRequired),
            other => Err(internal_error(format!(
                "unknown local_state value in control DB: {other}"
            ))),
        }
    }
}

/// Kind of remote operation recorded in `remote_operations`.
///
/// Mirrors the `operation_kind` CHECK constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Publish,
    Pull,
    DownloadAsset,
    DeleteAsset,
    Gc,
}

impl OperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            OperationKind::Publish => "publish",
            OperationKind::Pull => "pull",
            OperationKind::DownloadAsset => "download_asset",
            OperationKind::DeleteAsset => "delete_asset",
            OperationKind::Gc => "gc",
        }
    }

    fn from_db(value: &str) -> Result<Self, CommandError> {
        match value {
            "publish" => Ok(OperationKind::Publish),
            "pull" => Ok(OperationKind::Pull),
            "download_asset" => Ok(OperationKind::DownloadAsset),
            "delete_asset" => Ok(OperationKind::DeleteAsset),
            "gc" => Ok(OperationKind::Gc),
            other => Err(internal_error(format!(
                "unknown operation_kind value in control DB: {other}"
            ))),
        }
    }
}

/// Lifecycle state of a remote operation.
///
/// Mirrors the `state` CHECK constraint on `remote_operations`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Prepared,
    Pending,
    Running,
    RetryWait,
    Committing,
    Verifying,
    Completed,
    Failed,
    Conflicted,
    Cancelled,
}

impl OperationState {
    pub fn as_str(self) -> &'static str {
        match self {
            OperationState::Prepared => "prepared",
            OperationState::Pending => "pending",
            OperationState::Running => "running",
            OperationState::RetryWait => "retry_wait",
            OperationState::Committing => "committing",
            OperationState::Verifying => "verifying",
            OperationState::Completed => "completed",
            OperationState::Failed => "failed",
            OperationState::Conflicted => "conflicted",
            OperationState::Cancelled => "cancelled",
        }
    }

    fn from_db(value: &str) -> Result<Self, CommandError> {
        match value {
            "prepared" => Ok(OperationState::Prepared),
            "pending" => Ok(OperationState::Pending),
            "running" => Ok(OperationState::Running),
            "retry_wait" => Ok(OperationState::RetryWait),
            "committing" => Ok(OperationState::Committing),
            "verifying" => Ok(OperationState::Verifying),
            "completed" => Ok(OperationState::Completed),
            "failed" => Ok(OperationState::Failed),
            "conflicted" => Ok(OperationState::Conflicted),
            "cancelled" => Ok(OperationState::Cancelled),
            other => Err(internal_error(format!(
                "unknown operation state value in control DB: {other}"
            ))),
        }
    }
}

/// Transfer direction for a `remote_transfer_parts` row.
///
/// Mirrors the `direction` CHECK constraint.
// used by PR#5: resumable uploads/downloads
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirection {
    Upload,
    Download,
}

impl TransferDirection {
    // used by PR#5: resumable uploads/downloads
    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            TransferDirection::Upload => "upload",
            TransferDirection::Download => "download",
        }
    }

    // used by PR#5: resumable uploads/downloads
    #[allow(dead_code)]
    fn from_db(value: &str) -> Result<Self, CommandError> {
        match value {
            "upload" => Ok(TransferDirection::Upload),
            "download" => Ok(TransferDirection::Download),
            other => Err(internal_error(format!(
                "unknown transfer direction value in control DB: {other}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// payload_json contract
// ---------------------------------------------------------------------------

/// Stable JSON shape stored in `remote_operations.payload_json` for publish
/// operations. Later PRs (PR#4/#5) extend this with additional fields but must
/// keep `song_ids`, `percent`, and `detail` backward-compatible so recovery
/// and `get_all_upload_statuses` can read rows written by older versions.
///
/// ```json
/// {"song_ids": ["hash-a", "hash-b"], "percent": 42, "detail": "Uploading stems"}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationPayload {
    /// Song hashes affected by this operation. For publish operations this is
    /// the set of songs whose assets are uploaded.
    pub song_ids: Vec<String>,
    /// Completion percentage (0-100) for progress projection.
    pub percent: u8,
    /// Human-readable detail string for the upload status snapshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl OperationPayload {
    pub fn to_json(&self) -> CommandResult<String> {
        serde_json::to_string(self)
            .map_err(|e| internal_error(format!("failed to serialize operation payload: {e}")))
    }

    pub fn from_json(json: &str) -> CommandResult<Self> {
        serde_json::from_str(json)
            .map_err(|e| internal_error(format!("failed to deserialize operation payload: {e}")))
    }
}

// ---------------------------------------------------------------------------
// Row types
// ---------------------------------------------------------------------------

/// Row of `remote_repository_state`.
#[derive(Debug, Clone)]
pub struct RepositoryStateRow {
    pub library_id: String,
    pub committed_generation: i64,
    pub committed_manifest_revision: Option<String>,
    pub local_base_generation: i64,
    pub local_db_digest: Option<String>,
    pub local_state: LocalState,
    pub active_operation_id: Option<String>,
    pub last_success_at_ms: Option<i64>,
    pub last_error_code: Option<String>,
    pub updated_at_ms: i64,
    /// Stable repository UUID, set on first publication and never changed.
    /// Written into the manifest so all clients agree on repository identity.
    /// `None` for rows created before PR#4's manifest protocol.
    pub repository_id: Option<String>,
    /// Stable installation UUID of the writer. For diagnostics only, not a
    /// security principal. `None` for rows created before PR#4.
    pub writer_id: Option<String>,
}

/// Row of `remote_operations`.
#[derive(Debug, Clone)]
pub struct OperationRow {
    pub operation_id: String,
    pub library_id: String,
    pub operation_kind: OperationKind,
    pub state: OperationState,
    pub expected_generation: Option<i64>,
    pub target_generation: Option<i64>,
    pub source_db_digest: Option<String>,
    pub candidate_db_digest: Option<String>,
    pub payload_json: String,
    pub attempt_count: i64,
    pub next_attempt_at_ms: Option<i64>,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Row of `remote_transfer_parts`.
// used by PR#5: resumable uploads/downloads
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TransferPartRow {
    pub operation_id: String,
    pub relative_path: String,
    pub direction: TransferDirection,
    pub expected_size: Option<i64>,
    pub expected_digest: Option<String>,
    pub provider_revision: Option<String>,
    pub provider_session_id: Option<String>,
    pub transferred_bytes: i64,
    pub state: String,
    pub updated_at_ms: i64,
}

/// Row of `remote_cache_entries`.
// used by PR#6: persistent cache catalog
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CacheEntryRow {
    pub cache_key: String,
    pub library_id: String,
    pub relative_path: String,
    pub provider_revision: Option<String>,
    pub content_digest: Option<String>,
    pub expected_size: i64,
    pub downloaded_ranges_json: String,
    pub complete: bool,
    pub pinned_count: i64,
    pub last_access_at_ms: i64,
    pub verified_at_ms: Option<i64>,
    pub data_path: String,
}

// ---------------------------------------------------------------------------
// Connection management + migrations
// ---------------------------------------------------------------------------

/// Path of the control database inside the app data directory.
pub fn control_db_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(CONTROL_DB_FILENAME)
}

/// Open the control database, enable WAL journal mode, and apply migrations.
///
/// WAL mode lets concurrent readers (upload-status queries) proceed without
/// blocking the single writer that transitions operation state.
pub fn open_control_db(path: &Path) -> CommandResult<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            internal_error(format!(
                "failed to create control DB parent directory {}: {e}",
                path.display()
            ))
        })?;
    }
    let conn = Connection::open(path).map_err(|e| {
        database_error(format!(
            "failed to open control DB at {}: {e}",
            path.display()
        ))
    })?;

    // Enable WAL so readers never block the writer. This is a persistent
    // property of the database file, but setting it on every open is cheap
    // and makes the behavior explicit.
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| database_error(format!("failed to enable WAL on control DB: {e}")))?;

    apply_migrations(&conn)?;
    Ok(conn)
}

/// Apply all remote-state migrations. Idempotent: every statement uses
/// `CREATE TABLE IF NOT EXISTS` (migration 001) or checks for column existence
/// before adding (migration 002), so running this on an already-migrated
/// database is a no-op.
pub fn apply_migrations(connection: &Connection) -> CommandResult<()> {
    for migration in REMOTE_STATE_MIGRATIONS {
        connection
            .execute_batch(migration)
            .map_err(|e| database_error(format!("failed to apply control DB migration: {e}")))?;
    }
    // Migration 002: add repository_id and writer_id columns. ALTER TABLE ADD
    // COLUMN is not idempotent in SQLite, so we check for column existence
    // before adding.
    apply_migration_002_manifest_columns(connection)?;
    Ok(())
}

/// Add `repository_id` and `writer_id` columns to `remote_repository_state`
/// if they do not already exist. Idempotent.
fn apply_migration_002_manifest_columns(connection: &Connection) -> CommandResult<()> {
    let existing_columns: Vec<String> = connection
        .prepare("PRAGMA table_info(remote_repository_state);")
        .map_err(|e| database_error(format!("failed to inspect remote_repository_state: {e}")))?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| database_error(format!("failed to query column info: {e}")))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| database_error(format!("failed to collect column names: {e}")))?;

    if !existing_columns.iter().any(|c| c == "repository_id") {
        connection
            .execute(
                "ALTER TABLE remote_repository_state ADD COLUMN repository_id TEXT;",
                [],
            )
            .map_err(|e| database_error(format!("failed to add repository_id column: {e}")))?;
    }
    if !existing_columns.iter().any(|c| c == "writer_id") {
        connection
            .execute(
                "ALTER TABLE remote_repository_state ADD COLUMN writer_id TEXT;",
                [],
            )
            .map_err(|e| database_error(format!("failed to add writer_id column: {e}")))?;
    }
    Ok(())
}

/// Query the current journal mode. Used by tests to assert WAL is enabled.
// also used by PR#5 for transfer diagnostics
#[allow(dead_code)]
pub fn journal_mode(connection: &Connection) -> CommandResult<String> {
    connection
        .query_row("PRAGMA journal_mode;", [], |row| row.get::<_, String>(0))
        .map_err(|e| database_error(format!("failed to query journal mode: {e}")))
}

// ---------------------------------------------------------------------------
// remote_repository_state CRUD
// ---------------------------------------------------------------------------

/// Insert or replace a repository state row.
pub fn upsert_repository_state(
    connection: &Connection,
    row: &RepositoryStateRow,
) -> CommandResult<()> {
    connection
        .execute(
            "INSERT INTO remote_repository_state (
                library_id, committed_generation, committed_manifest_revision,
                local_base_generation, local_db_digest, local_state,
                active_operation_id, last_success_at_ms, last_error_code, updated_at_ms,
                repository_id, writer_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(library_id) DO UPDATE SET
                committed_generation = excluded.committed_generation,
                committed_manifest_revision = excluded.committed_manifest_revision,
                local_base_generation = excluded.local_base_generation,
                local_db_digest = excluded.local_db_digest,
                local_state = excluded.local_state,
                active_operation_id = excluded.active_operation_id,
                last_success_at_ms = excluded.last_success_at_ms,
                last_error_code = excluded.last_error_code,
                updated_at_ms = excluded.updated_at_ms,
                repository_id = COALESCE(excluded.repository_id, remote_repository_state.repository_id),
                writer_id = COALESCE(excluded.writer_id, remote_repository_state.writer_id)",
            params![
                row.library_id,
                row.committed_generation,
                row.committed_manifest_revision,
                row.local_base_generation,
                row.local_db_digest,
                row.local_state.as_str(),
                row.active_operation_id,
                row.last_success_at_ms,
                row.last_error_code,
                row.updated_at_ms,
                row.repository_id,
                row.writer_id,
            ],
        )
        .map_err(|e| database_error(format!("failed to upsert repository state: {e}")))?;
    Ok(())
}

/// Load a repository state row by library_id.
pub fn get_repository_state(
    connection: &Connection,
    library_id: &str,
) -> CommandResult<Option<RepositoryStateRow>> {
    let mut stmt = connection
        .prepare(
            "SELECT library_id, committed_generation, committed_manifest_revision,
                    local_base_generation, local_db_digest, local_state,
                    active_operation_id, last_success_at_ms, last_error_code, updated_at_ms,
                    repository_id, writer_id
             FROM remote_repository_state WHERE library_id = ?1",
        )
        .map_err(|e| database_error(format!("failed to prepare repository state query: {e}")))?;
    let row = stmt
        .query_row([library_id], map_repository_state_row)
        .optional()
        .map_err(|e| database_error(format!("failed to query repository state: {e}")))?;
    Ok(row)
}

/// Load all repository state rows.
// used by PR#4: recovery coordinator
#[allow(dead_code)]
pub fn list_repository_states(connection: &Connection) -> CommandResult<Vec<RepositoryStateRow>> {
    let mut stmt = connection
        .prepare(
            "SELECT library_id, committed_generation, committed_manifest_revision,
                    local_base_generation, local_db_digest, local_state,
                    active_operation_id, last_success_at_ms, last_error_code, updated_at_ms,
                    repository_id, writer_id
             FROM remote_repository_state",
        )
        .map_err(|e| database_error(format!("failed to prepare repository state list: {e}")))?;
    let rows = stmt
        .query_map([], map_repository_state_row)
        .map_err(|e| database_error(format!("failed to list repository states: {e}")))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| database_error(format!("failed to collect repository states: {e}")))?;
    Ok(rows)
}

fn map_repository_state_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepositoryStateRow> {
    let local_state_str: String = row.get(5)?;
    let local_state = LocalState::from_db(&local_state_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.message,
            )),
        )
    })?;
    Ok(RepositoryStateRow {
        library_id: row.get(0)?,
        committed_generation: row.get(1)?,
        committed_manifest_revision: row.get(2)?,
        local_base_generation: row.get(3)?,
        local_db_digest: row.get(4)?,
        local_state,
        active_operation_id: row.get(6)?,
        last_success_at_ms: row.get(7)?,
        last_error_code: row.get(8)?,
        updated_at_ms: row.get(9)?,
        repository_id: row.get(10)?,
        writer_id: row.get(11)?,
    })
}

// ---------------------------------------------------------------------------
// remote_operations CRUD
// ---------------------------------------------------------------------------

/// Insert or replace an operation row.
pub fn upsert_operation(connection: &Connection, row: &OperationRow) -> CommandResult<()> {
    connection
        .execute(
            "INSERT INTO remote_operations (
                operation_id, library_id, operation_kind, state,
                expected_generation, target_generation, source_db_digest, candidate_db_digest,
                payload_json, attempt_count, next_attempt_at_ms,
                error_code, error_detail, created_at_ms, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            ON CONFLICT(operation_id) DO UPDATE SET
                library_id = excluded.library_id,
                operation_kind = excluded.operation_kind,
                state = excluded.state,
                expected_generation = excluded.expected_generation,
                target_generation = excluded.target_generation,
                source_db_digest = excluded.source_db_digest,
                candidate_db_digest = excluded.candidate_db_digest,
                payload_json = excluded.payload_json,
                attempt_count = excluded.attempt_count,
                next_attempt_at_ms = excluded.next_attempt_at_ms,
                error_code = excluded.error_code,
                error_detail = excluded.error_detail,
                updated_at_ms = excluded.updated_at_ms",
            params![
                row.operation_id,
                row.library_id,
                row.operation_kind.as_str(),
                row.state.as_str(),
                row.expected_generation,
                row.target_generation,
                row.source_db_digest,
                row.candidate_db_digest,
                row.payload_json,
                row.attempt_count,
                row.next_attempt_at_ms,
                row.error_code,
                row.error_detail,
                row.created_at_ms,
                row.updated_at_ms,
            ],
        )
        .map_err(|e| database_error(format!("failed to upsert operation: {e}")))?;
    Ok(())
}

/// Load an operation row by operation_id.
pub fn get_operation(
    connection: &Connection,
    operation_id: &str,
) -> CommandResult<Option<OperationRow>> {
    let mut stmt = connection
        .prepare(
            "SELECT operation_id, library_id, operation_kind, state,
                    expected_generation, target_generation, source_db_digest, candidate_db_digest,
                    payload_json, attempt_count, next_attempt_at_ms,
                    error_code, error_detail, created_at_ms, updated_at_ms
             FROM remote_operations WHERE operation_id = ?1",
        )
        .map_err(|e| database_error(format!("failed to prepare operation query: {e}")))?;
    let row = stmt
        .query_row([operation_id], map_operation_row)
        .optional()
        .map_err(|e| database_error(format!("failed to query operation: {e}")))?;
    Ok(row)
}

/// Load all operation rows.
pub fn list_operations(connection: &Connection) -> CommandResult<Vec<OperationRow>> {
    let mut stmt = connection
        .prepare(
            "SELECT operation_id, library_id, operation_kind, state,
                    expected_generation, target_generation, source_db_digest, candidate_db_digest,
                    payload_json, attempt_count, next_attempt_at_ms,
                    error_code, error_detail, created_at_ms, updated_at_ms
             FROM remote_operations",
        )
        .map_err(|e| database_error(format!("failed to prepare operation list: {e}")))?;
    let rows = stmt
        .query_map([], map_operation_row)
        .map_err(|e| database_error(format!("failed to list operations: {e}")))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| database_error(format!("failed to collect operations: {e}")))?;
    Ok(rows)
}

/// Load all operation rows for a given library.
// used by PR#4: operation executor
#[allow(dead_code)]
pub fn list_operations_for_library(
    connection: &Connection,
    library_id: &str,
) -> CommandResult<Vec<OperationRow>> {
    let mut stmt = connection
        .prepare(
            "SELECT operation_id, library_id, operation_kind, state,
                    expected_generation, target_generation, source_db_digest, candidate_db_digest,
                    payload_json, attempt_count, next_attempt_at_ms,
                    error_code, error_detail, created_at_ms, updated_at_ms
             FROM remote_operations WHERE library_id = ?1",
        )
        .map_err(|e| database_error(format!("failed to prepare operation list: {e}")))?;
    let rows = stmt
        .query_map([library_id], map_operation_row)
        .map_err(|e| database_error(format!("failed to list operations: {e}")))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| database_error(format!("failed to collect operations: {e}")))?;
    Ok(rows)
}

/// Load all operation rows matching one of the given states.
pub fn list_operations_in_states(
    connection: &Connection,
    states: &[OperationState],
) -> CommandResult<Vec<OperationRow>> {
    if states.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (0..states.len()).map(|i| format!("?{}", i + 1)).collect();
    let sql = format!(
        "SELECT operation_id, library_id, operation_kind, state,
                expected_generation, target_generation, source_db_digest, candidate_db_digest,
                payload_json, attempt_count, next_attempt_at_ms,
                error_code, error_detail, created_at_ms, updated_at_ms
         FROM remote_operations WHERE state IN ({})",
        placeholders.join(", ")
    );
    let mut stmt = connection
        .prepare(&sql)
        .map_err(|e| database_error(format!("failed to prepare operation state query: {e}")))?;
    let state_strs: Vec<&str> = states.iter().map(|s| s.as_str()).collect();
    let params_iter = state_strs
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect::<Vec<_>>();
    let rows = stmt
        .query_map(params_iter.as_slice(), map_operation_row)
        .map_err(|e| database_error(format!("failed to query operations by state: {e}")))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| database_error(format!("failed to collect operations by state: {e}")))?;
    Ok(rows)
}

fn map_operation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OperationRow> {
    let kind_str: String = row.get(2)?;
    let state_str: String = row.get(3)?;
    let operation_kind = OperationKind::from_db(&kind_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.message,
            )),
        )
    })?;
    let state = OperationState::from_db(&state_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.message,
            )),
        )
    })?;
    Ok(OperationRow {
        operation_id: row.get(0)?,
        library_id: row.get(1)?,
        operation_kind,
        state,
        expected_generation: row.get(4)?,
        target_generation: row.get(5)?,
        source_db_digest: row.get(6)?,
        candidate_db_digest: row.get(7)?,
        payload_json: row.get(8)?,
        attempt_count: row.get(9)?,
        next_attempt_at_ms: row.get(10)?,
        error_code: row.get(11)?,
        error_detail: row.get(12)?,
        created_at_ms: row.get(13)?,
        updated_at_ms: row.get(14)?,
    })
}

// ---------------------------------------------------------------------------
// remote_transfer_parts CRUD
// ---------------------------------------------------------------------------

/// Insert or replace a transfer part row.
// used by PR#5: resumable uploads/downloads
#[allow(dead_code)]
pub fn upsert_transfer_part(connection: &Connection, row: &TransferPartRow) -> CommandResult<()> {
    connection
        .execute(
            "INSERT INTO remote_transfer_parts (
                operation_id, relative_path, direction,
                expected_size, expected_digest, provider_revision, provider_session_id,
                transferred_bytes, state, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(operation_id, relative_path, direction) DO UPDATE SET
                expected_size = excluded.expected_size,
                expected_digest = excluded.expected_digest,
                provider_revision = excluded.provider_revision,
                provider_session_id = excluded.provider_session_id,
                transferred_bytes = excluded.transferred_bytes,
                state = excluded.state,
                updated_at_ms = excluded.updated_at_ms",
            params![
                row.operation_id,
                row.relative_path,
                row.direction.as_str(),
                row.expected_size,
                row.expected_digest,
                row.provider_revision,
                row.provider_session_id,
                row.transferred_bytes,
                row.state,
                row.updated_at_ms,
            ],
        )
        .map_err(|e| database_error(format!("failed to upsert transfer part: {e}")))?;
    Ok(())
}

/// Load all transfer parts for an operation.
// used by PR#5: resumable uploads/downloads
#[allow(dead_code)]
pub fn list_transfer_parts(
    connection: &Connection,
    operation_id: &str,
) -> CommandResult<Vec<TransferPartRow>> {
    let mut stmt = connection
        .prepare(
            "SELECT operation_id, relative_path, direction,
                    expected_size, expected_digest, provider_revision, provider_session_id,
                    transferred_bytes, state, updated_at_ms
             FROM remote_transfer_parts WHERE operation_id = ?1",
        )
        .map_err(|e| database_error(format!("failed to prepare transfer part query: {e}")))?;
    let rows = stmt
        .query_map([operation_id], map_transfer_part_row)
        .map_err(|e| database_error(format!("failed to list transfer parts: {e}")))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| database_error(format!("failed to collect transfer parts: {e}")))?;
    Ok(rows)
}

// used by PR#5: resumable uploads/downloads
#[allow(dead_code)]
fn map_transfer_part_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TransferPartRow> {
    let direction_str: String = row.get(2)?;
    let direction = TransferDirection::from_db(&direction_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.message,
            )),
        )
    })?;
    Ok(TransferPartRow {
        operation_id: row.get(0)?,
        relative_path: row.get(1)?,
        direction,
        expected_size: row.get(3)?,
        expected_digest: row.get(4)?,
        provider_revision: row.get(5)?,
        provider_session_id: row.get(6)?,
        transferred_bytes: row.get(7)?,
        state: row.get(8)?,
        updated_at_ms: row.get(9)?,
    })
}

/// Delete all transfer parts for an operation. Called after a transfer
/// completes (or is cancelled) so stale offsets do not cause a future restart
/// to resume against a non-existent remote partial.
// used by PR#5: resumable uploads/downloads
#[allow(dead_code)]
pub fn delete_transfer_parts(connection: &Connection, operation_id: &str) -> CommandResult<()> {
    connection
        .execute(
            "DELETE FROM remote_transfer_parts WHERE operation_id = ?1",
            params![operation_id],
        )
        .map_err(|e| database_error(format!("failed to delete transfer parts: {e}")))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// remote_cache_entries CRUD
// ---------------------------------------------------------------------------

/// Insert or replace a cache entry row.
///
/// PR#6 will populate this table; PR#2 only provides the accessor so the
/// schema is in place.
// used by PR#6: persistent cache catalog
#[allow(dead_code)]
pub fn upsert_cache_entry(connection: &Connection, row: &CacheEntryRow) -> CommandResult<()> {
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
                row.pinned_count,
                row.last_access_at_ms,
                row.verified_at_ms,
                row.data_path,
            ],
        )
        .map_err(|e| database_error(format!("failed to upsert cache entry: {e}")))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Digest helper
// ---------------------------------------------------------------------------

/// Compute the SHA-256 hex digest of a file's contents.
///
/// Used to record the working DB digest before and after a local mutation so
/// recovery can detect whether a `prepared` operation's local edit committed.
pub fn sha256_file(path: &Path) -> CommandResult<String> {
    let bytes = std::fs::read(path).map_err(|e| {
        internal_error(format!("failed to read {} for digest: {e}", path.display()))
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hash::hex_lower(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh_db() -> (TempDir, Connection) {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("remote-state.db");
        let conn = open_control_db(&path).expect("open control DB");
        (dir, conn)
    }

    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    }

    #[test]
    fn wal_mode_is_enabled_on_open() {
        let (_dir, conn) = fresh_db();
        let mode = journal_mode(&conn).expect("journal mode");
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn migration_is_idempotent() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("remote-state.db");
        let conn = open_control_db(&path).expect("first open");
        // Re-running migrations on an already-migrated DB must not error.
        apply_migrations(&conn).expect("second migration is a no-op");
        // Re-opening also re-runs migrations and must succeed.
        drop(conn);
        let conn2 = open_control_db(&path).expect("reopen");
        // Tables still exist and are usable.
        let row = RepositoryStateRow {
            library_id: "lib-1".to_owned(),
            committed_generation: 0,
            committed_manifest_revision: None,
            local_base_generation: 0,
            local_db_digest: None,
            local_state: LocalState::Clean,
            active_operation_id: None,
            last_success_at_ms: None,
            last_error_code: None,
            updated_at_ms: now_ms(),
            repository_id: None,
            writer_id: None,
        };
        upsert_repository_state(&conn2, &row).expect("upsert after re-migration");
    }

    #[test]
    fn repository_state_round_trips() {
        let (_dir, conn) = fresh_db();
        let row = RepositoryStateRow {
            library_id: "lib-1".to_owned(),
            committed_generation: 3,
            committed_manifest_revision: Some("rev-abc".to_owned()),
            local_base_generation: 2,
            local_db_digest: Some("deadbeef".to_owned()),
            local_state: LocalState::Dirty,
            active_operation_id: Some("op-1".to_owned()),
            last_success_at_ms: Some(1000),
            last_error_code: None,
            updated_at_ms: now_ms(),
            repository_id: Some("repo-uuid".to_owned()),
            writer_id: Some("writer-uuid".to_owned()),
        };
        upsert_repository_state(&conn, &row).expect("upsert");
        let loaded = get_repository_state(&conn, "lib-1").expect("get").unwrap();
        assert_eq!(loaded.library_id, "lib-1");
        assert_eq!(loaded.committed_generation, 3);
        assert_eq!(loaded.local_state, LocalState::Dirty);
        assert_eq!(loaded.local_db_digest.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn operation_round_trips() {
        let (_dir, conn) = fresh_db();
        let payload = OperationPayload {
            song_ids: vec!["song-a".to_owned()],
            percent: 42,
            detail: Some("Uploading".to_owned()),
        };
        let row = OperationRow {
            operation_id: "op-1".to_owned(),
            library_id: "lib-1".to_owned(),
            operation_kind: OperationKind::Publish,
            state: OperationState::Pending,
            expected_generation: Some(3),
            target_generation: Some(4),
            source_db_digest: Some("abc".to_owned()),
            candidate_db_digest: None,
            payload_json: payload.to_json().unwrap(),
            attempt_count: 0,
            next_attempt_at_ms: None,
            error_code: None,
            error_detail: None,
            created_at_ms: now_ms(),
            updated_at_ms: now_ms(),
        };
        upsert_operation(&conn, &row).expect("upsert");
        let loaded = get_operation(&conn, "op-1").expect("get").unwrap();
        assert_eq!(loaded.operation_kind, OperationKind::Publish);
        assert_eq!(loaded.state, OperationState::Pending);
        let p = OperationPayload::from_json(&loaded.payload_json).unwrap();
        assert_eq!(p.song_ids, vec!["song-a".to_owned()]);
        assert_eq!(p.percent, 42);
    }

    #[test]
    fn list_operations_in_states_filters() {
        let (_dir, conn) = fresh_db();
        let now = now_ms();
        for (id, state) in [
            ("op-1", OperationState::Running),
            ("op-2", OperationState::Completed),
            ("op-3", OperationState::Committing),
        ] {
            upsert_operation(
                &conn,
                &OperationRow {
                    operation_id: id.to_owned(),
                    library_id: "lib-1".to_owned(),
                    operation_kind: OperationKind::Publish,
                    state,
                    expected_generation: None,
                    target_generation: None,
                    source_db_digest: None,
                    candidate_db_digest: None,
                    payload_json: r#"{"song_ids":[],"percent":0}"#.to_owned(),
                    attempt_count: 0,
                    next_attempt_at_ms: None,
                    error_code: None,
                    error_detail: None,
                    created_at_ms: now,
                    updated_at_ms: now,
                },
            )
            .expect("upsert");
        }
        let active = list_operations_in_states(
            &conn,
            &[OperationState::Running, OperationState::Committing],
        )
        .expect("list");
        assert_eq!(active.len(), 2);
        assert!(active.iter().all(|r| r.operation_id != "op-2"));
    }

    #[test]
    fn transfer_part_round_trips() {
        let (_dir, conn) = fresh_db();
        let row = TransferPartRow {
            operation_id: "op-1".to_owned(),
            relative_path: "media/song.mp3".to_owned(),
            direction: TransferDirection::Upload,
            expected_size: Some(1024),
            expected_digest: Some("hash".to_owned()),
            provider_revision: None,
            provider_session_id: None,
            transferred_bytes: 512,
            state: "in_progress".to_owned(),
            updated_at_ms: now_ms(),
        };
        upsert_transfer_part(&conn, &row).expect("upsert");
        let parts = list_transfer_parts(&conn, "op-1").expect("list");
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].direction, TransferDirection::Upload);
        assert_eq!(parts[0].transferred_bytes, 512);
    }

    #[test]
    fn sha256_file_matches_known_value() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("test.bin");
        std::fs::write(&path, b"hello").expect("write");
        let digest = sha256_file(&path).expect("digest");
        // SHA-256 of "hello"
        assert_eq!(
            digest,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn payload_json_round_trips_without_detail() {
        let payload = OperationPayload {
            song_ids: vec!["x".to_owned()],
            percent: 0,
            detail: None,
        };
        let json = payload.to_json().unwrap();
        let back = OperationPayload::from_json(&json).unwrap();
        assert_eq!(back.song_ids, vec!["x".to_owned()]);
        assert_eq!(back.percent, 0);
    }
}
