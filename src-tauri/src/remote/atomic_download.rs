//! Atomic, verified downloads for remote repository assets.
//!
//! Every download lands in a sibling `.part.<operation_id>` temp file first.
//! Only after size/digest/integrity checks pass is the temp file `fsync`-ed
//! and atomically renamed over the final destination. This guarantees the
//! final path is never visible to readers in a truncated or corrupt state,
//! even if the process is killed mid-download or the network drops.
//!
//! ## Why a shared helper
//!
//! PR #1 inlined this pattern for stem sets. Media, stems, artwork, CDG, and
//! database pulls all need the same guarantees, so the logic lives here once.
//! Provider `download_file` internals are NOT modified here — resumable
//! transfers are PR #5's job. This helper always downloads the full file to a
//! temp path; resume-from-offset is out of scope.
//!
//! ## Database-specific integrity
//!
//! [`atomic_database_pull`] adds SQLite-specific checks on top of the generic
//! helper: `PRAGMA quick_check`, `PRAGMA foreign_key_check`, and a schema
//! compatibility check (the `songs` table must exist). A verified candidate
//! replaces the working `openkara.db` atomically, and the previous verified
//! copy is preserved as `openkara.db.lkg` (last-known-good) so a failed
//! activation never destroys the last working database.

use crate::{
    cache,
    commands::error::{internal_error, CommandError, CommandResult},
    hash,
    library_root::LibraryRoot,
    remote::control_db::{
        get_repository_state, upsert_repository_state, LocalState, RepositoryStateRow,
    },
    remote::provider::RemoteProvider,
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Sanitized error code string used when a downloaded candidate fails
/// integrity or schema compatibility checks. The full typed error enum is
/// PR #4's job; PR #3 persists/returns this stable string so recovery and UI
/// can branch on it without coupling to a not-yet-defined enum.
pub const REMOTE_INTEGRITY_FAILED: &str = "remote_integrity_failed";

/// Options describing a single atomic download + validation + rename.
pub(crate) struct AtomicDownloadOptions<'a> {
    /// Remote-relative path passed to `provider.download_file`.
    pub relative_path: &'a str,
    /// Final destination path. The temp file is created as a sibling
    /// `<destination>.part.<operation_id>` so the rename stays on the same
    /// filesystem (required for atomicity).
    pub destination: &'a Path,
    /// Expected byte length. When `Some`, the temp file is rejected if its
    /// length differs. When `None`, the size check is skipped (caller does not
    /// know the size yet — PR #4's manifest will supply it).
    pub expected_size: Option<u64>,
    /// Expected hex SHA-256 digest. When `Some`, the temp file is rejected if
    /// its SHA-256 differs. When `None`, the digest check is skipped.
    pub expected_digest: Option<&'a str>,
    /// Operation identifier used for temp file naming and log correlation.
    pub operation_id: &'a str,
}

/// Errors specific to atomic download validation. Network/IO failures are
/// wrapped from the provider's `CommandError` / `std::io::Error`.
#[derive(Debug)]
pub(crate) enum AtomicDownloadError {
    SizeMismatch { expected: u64, actual: u64 },
    DigestMismatch { expected: String, actual: String },
    DownloadFailed(CommandError),
    Io(std::io::Error),
}

impl std::fmt::Display for AtomicDownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AtomicDownloadError::SizeMismatch { expected, actual } => write!(
                f,
                "downloaded file size {actual} does not match expected {expected}"
            ),
            AtomicDownloadError::DigestMismatch { expected, actual } => write!(
                f,
                "downloaded file digest {actual} does not match expected {expected}"
            ),
            AtomicDownloadError::DownloadFailed(command_error) => {
                write!(f, "download failed: {:?}", command_error)
            }
            AtomicDownloadError::Io(error) => write!(f, "io error: {error}"),
        }
    }
}

impl std::error::Error for AtomicDownloadError {}

impl From<AtomicDownloadError> for CommandError {
    fn from(error: AtomicDownloadError) -> Self {
        match error {
            AtomicDownloadError::DownloadFailed(command_error) => command_error,
            AtomicDownloadError::Io(io_error) => {
                internal_error(format!("atomic download io error: {io_error}"))
            }
            AtomicDownloadError::SizeMismatch { expected, actual } => internal_error(format!(
                "atomic download size mismatch: expected {expected}, got {actual}"
            )),
            AtomicDownloadError::DigestMismatch { expected, actual } => internal_error(format!(
                "atomic download digest mismatch: expected {expected}, got {actual}"
            )),
        }
    }
}

/// Download `relative_path` from `provider` to a temp file, validate it, then
/// atomically rename it over `destination`.
///
/// The caller is responsible for removing an existing destination first when
/// a stale file must not be overwritten by rename semantics on the target OS.
/// On POSIX `fs::rename` over an existing file atomically replaces it; on
/// Windows `fs::rename` also replaces a same-volume destination. When the
/// destination must be preserved as a last-known-good (database pulls), use
/// [`atomic_database_pull`] instead — it renames the old destination aside
/// before the candidate rename.
///
/// On ANY failure after the temp file is created, the temp file is removed
/// (best-effort) so no partial file lingers at a final-adjacent path.
pub(crate) fn atomic_download(
    provider: &dyn RemoteProvider,
    opts: AtomicDownloadOptions,
) -> CommandResult<()> {
    let temp_path = part_path(opts.destination, opts.operation_id);

    // Ensure the parent directory exists (e.g. a fresh stem directory).
    if let Some(parent) = temp_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            internal_error(format!(
                "failed to create parent directory {}: {e}",
                parent.display()
            ))
        })?;
    }

    // Clean up a stale temp file from a previous attempt so the provider
    // writes into a fresh file.
    let _ = fs::remove_file(&temp_path);

    let result = run_atomic_download(provider, opts, &temp_path);

    // On failure, remove the temp file so no partial download lingers.
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    result
}

fn run_atomic_download(
    provider: &dyn RemoteProvider,
    opts: AtomicDownloadOptions,
    temp_path: &Path,
) -> CommandResult<()> {
    // 1. Download to the temp path. Provider internals are not modified.
    provider
        .download_file(opts.relative_path, temp_path)
        .map_err(AtomicDownloadError::DownloadFailed)?;

    // 2. Size check.
    if let Some(expected_size) = opts.expected_size {
        let actual = fs::metadata(temp_path).map(|m| m.len()).map_err(|e| {
            AtomicDownloadError::Io(std::io::Error::other(format!(
                "failed to stat temp file {}: {e}",
                temp_path.display()
            )))
        })?;
        if actual != expected_size {
            return Err(AtomicDownloadError::SizeMismatch {
                expected: expected_size,
                actual,
            }
            .into());
        }
    }

    // 3. Digest check.
    if let Some(expected_digest) = opts.expected_digest {
        let actual = sha256_file(temp_path)?;
        if !digests_equal(&actual, expected_digest) {
            return Err(AtomicDownloadError::DigestMismatch {
                expected: expected_digest.to_owned(),
                actual,
            }
            .into());
        }
    }

    // 4. fsync the temp file so its bytes are durable before the rename.
    fsync_file(temp_path)?;

    // 5. Atomically rename temp -> destination. On POSIX this replaces the
    //    destination atomically; on Windows same-volume rename also replaces.
    //    Callers that need to preserve the old destination (database pulls)
    //    must rename it aside first — see `atomic_database_pull`.
    fs::rename(temp_path, opts.destination).map_err(|e| {
        AtomicDownloadError::Io(std::io::Error::other(format!(
            "failed to atomically rename {} -> {}: {e}",
            temp_path.display(),
            opts.destination.display()
        )))
    })?;

    // 6. fsync the parent directory so the rename is durable on POSIX.
    //    On Windows directory fsync is not meaningful; guard with cfg.
    fsync_parent(opts.destination)?;

    Ok(())
}

/// Build the temp path `<destination>.part.<operation_id>`.
fn part_path(destination: &Path, operation_id: &str) -> PathBuf {
    let file_name = destination
        .file_name()
        .map(|name| format!("{}.part.{}", name.to_string_lossy(), operation_id))
        .unwrap_or_else(|| format!("openkara.part.{operation_id}"));
    destination.with_file_name(file_name)
}

/// Compute the SHA-256 hex digest of a file.
///
/// Factored here (rather than reusing `control_db::sha256_file`) so this
/// module does not depend on the control DB module for a pure crypto helper.
/// The implementation is identical.
pub(crate) fn sha256_file(path: &Path) -> CommandResult<String> {
    let bytes = fs::read(path).map_err(|e| {
        internal_error(format!("failed to read {} for digest: {e}", path.display()))
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hash::hex_lower(hasher.finalize()))
}

/// Compare two hex digests case-insensitively. Providers may return either
/// casing; normalize before comparing so a casing difference is not mistaken
/// for a digest mismatch.
fn digests_equal(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn fsync_file(path: &Path) -> CommandResult<()> {
    let file = fs::File::open(path)
        .map_err(|e| internal_error(format!("failed to open {} for fsync: {e}", path.display())))?;
    file.sync_all()
        .map_err(|e| internal_error(format!("failed to fsync {}: {e}", path.display())))?;
    Ok(())
}

fn fsync_parent(path: &Path) -> CommandResult<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    #[cfg(unix)]
    {
        let dir = fs::File::open(parent)
            .map_err(|e| internal_error(format!("failed to open parent dir for fsync: {e}")))?;
        dir.sync_all()
            .map_err(|e| internal_error(format!("failed to fsync parent directory: {e}")))?;
    }
    #[cfg(not(unix))]
    {
        // Windows does not support directory fsync; the file fsync above is
        // the best-effort durability guarantee. This is a no-op.
        let _ = parent;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Atomic database pull with integrity checks + last-known-good
// ---------------------------------------------------------------------------

/// Options for an atomic database pull. The candidate is downloaded to a temp
/// file, integrity-checked, then atomically renamed over the working
/// `openkara.db`. The previous verified database is preserved as
/// `openkara.db.lkg`.
pub(crate) struct DatabasePullOptions<'a> {
    /// Operation identifier for temp file naming and logging.
    pub operation_id: &'a str,
    /// Expected byte length of the candidate, when known. When `None` the size
    /// check is skipped but integrity checks still run.
    pub expected_size: Option<u64>,
    /// Expected hex SHA-256 of the candidate. For PR #3 this is computed and
    /// stored but comparison against the manifest is deferred to PR #4.
    pub expected_digest: Option<&'a str>,
    /// Library id, used to update the control DB repository state after
    /// activation succeeds.
    pub library_id: &'a str,
}

/// Result of an atomic database pull, returned so callers can persist the new
/// digest/revision and tests can inspect the outcome.
#[derive(Debug, Clone)]
pub(crate) struct DatabasePullResult {
    /// SHA-256 hex digest of the newly activated `openkara.db`.
    // used by PR#4: manifest generation comparison and tests
    #[allow(dead_code)]
    pub new_digest: String,
    /// Final size in bytes of the activated database.
    // used by PR#4: manifest size comparison and tests
    #[allow(dead_code)]
    pub new_size: u64,
}

/// Download, validate, and atomically install the remote `openkara.db` into
/// the working copy rooted at `library_root`.
///
/// Sequence (issue step 4, "For databases"):
/// 1. Download to `openkara.db.part.<operation-id>` (sibling temp file).
/// 2. Enforce expected byte length when known.
/// 3. Compute SHA-256 of the candidate. PR #3 stores it; PR #4 will compare
///    against the manifest's `databaseSha256`.
/// 4. Open the candidate read-only and run `PRAGMA quick_check` and
///    `PRAGMA foreign_key_check`. Reject on any failure.
/// 5. Verify schema compatibility: the `songs` table must exist.
/// 6. `fsync` the candidate.
/// 7. Preserve the current verified database: rename `openkara.db` to
///    `openkara.db.lkg` before replacing. Skip if no current DB exists.
/// 8. Atomically rename candidate -> `openkara.db`.
/// 9. `fsync` the parent directory.
/// 10. Update local repository state (control DB) only after activation
///     succeeds.
///
/// When the candidate is corrupt or incompatible, the working copy is left
/// untouched and the last-known-good remains usable. A
/// [`REMOTE_INTEGRITY_FAILED`] error code is returned.
pub(crate) fn atomic_database_pull(
    provider: &dyn RemoteProvider,
    control_db_conn: &Connection,
    library_root: &LibraryRoot,
    opts: DatabasePullOptions,
) -> CommandResult<DatabasePullResult> {
    let destination = library_root.database_path();
    let temp_path = part_path(&destination, opts.operation_id);

    // Ensure the working-copy root directory exists.
    fs::create_dir_all(library_root.root()).map_err(|e| {
        internal_error(format!(
            "failed to create library root {}: {e}",
            library_root.root().display()
        ))
    })?;

    // Clean up a stale temp file from a previous attempt.
    let _ = fs::remove_file(&temp_path);

    let result = run_database_pull(provider, control_db_conn, &destination, &temp_path, &opts);

    // On failure, remove the temp candidate so no partial file lingers.
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    result
}

fn run_database_pull(
    provider: &dyn RemoteProvider,
    control_db_conn: &Connection,
    destination: &Path,
    temp_path: &Path,
    opts: &DatabasePullOptions,
) -> CommandResult<DatabasePullResult> {
    // 1. Download the candidate to the temp path.
    provider
        .download_file("openkara.db", temp_path)
        .map_err(AtomicDownloadError::DownloadFailed)?;

    // 2. Size check (when known).
    let actual_size = fs::metadata(temp_path).map(|m| m.len()).map_err(|e| {
        AtomicDownloadError::Io(std::io::Error::other(format!(
            "failed to stat candidate {}: {e}",
            temp_path.display()
        )))
    })?;
    if let Some(expected_size) = opts.expected_size {
        if actual_size != expected_size {
            return Err(AtomicDownloadError::SizeMismatch {
                expected: expected_size,
                actual: actual_size,
            }
            .into());
        }
    }

    // 3. Compute SHA-256 of the candidate.
    // TODO(PR#4): compare against manifest databaseSha256.
    let candidate_digest = sha256_file(temp_path)?;
    if let Some(expected_digest) = opts.expected_digest {
        if !digests_equal(&candidate_digest, expected_digest) {
            return Err(AtomicDownloadError::DigestMismatch {
                expected: expected_digest.to_owned(),
                actual: candidate_digest,
            }
            .into());
        }
    }

    // 4. SQLite integrity checks: quick_check + foreign_key_check.
    verify_sqlite_integrity(temp_path)?;

    // 5. Schema compatibility: the `songs` table must exist.
    verify_schema_compatibility(temp_path)?;

    // 6. fsync the candidate.
    fsync_file(temp_path)?;

    // 7. Preserve the current verified database as last-known-good.
    let lkg_path = last_known_good_path(destination);
    if destination.exists() {
        // Rename the current verified DB aside before replacing it. If a
        // stale LKG exists, remove it first so the rename target is clear.
        let _ = fs::remove_file(&lkg_path);
        fs::rename(destination, &lkg_path).map_err(|e| {
            internal_error(format!(
                "failed to preserve last-known-good {}: {e}",
                lkg_path.display()
            ))
        })?;
    }

    // 8. Atomically rename candidate -> openkara.db.
    if let Err(e) = fs::rename(temp_path, destination) {
        // The rename failed after we moved the old DB aside. Restore the
        // last-known-good so the working copy is not left without a database.
        if lkg_path.exists() {
            let _ = fs::rename(&lkg_path, destination);
        }
        return Err(internal_error(format!(
            "failed to atomically install candidate database: {e}"
        )));
    }

    // 9. fsync the parent directory.
    fsync_parent(destination)?;

    // 10. Update local repository state only after activation succeeds.
    // TODO(PR#4): advance committed_generation from manifest.
    update_repository_state_after_pull(control_db_conn, opts.library_id, &candidate_digest)?;

    Ok(DatabasePullResult {
        new_digest: candidate_digest,
        new_size: actual_size,
    })
}

/// Path of the last-known-good database: `<db>.lkg`.
fn last_known_good_path(db_path: &Path) -> PathBuf {
    let file_name = db_path
        .file_name()
        .map(|name| format!("{}.lkg", name.to_string_lossy()))
        .unwrap_or_else(|| "openkara.db.lkg".to_owned());
    db_path.with_file_name(file_name)
}

/// Open the candidate SQLite file read-only and run `PRAGMA quick_check` and
/// `PRAGMA foreign_key_check`. Reject on any failure with a
/// `remote_integrity_failed` error.
// used by PR#4: executor integrity checks on the candidate DB
pub(crate) fn verify_sqlite_integrity_pub(candidate: &Path) -> CommandResult<()> {
    verify_sqlite_integrity(candidate)
}

/// Open the candidate SQLite file read-only and run `PRAGMA quick_check` and
/// `PRAGMA foreign_key_check`. Reject on any failure with a
/// `remote_integrity_failed` error.
fn verify_sqlite_integrity(candidate: &Path) -> CommandResult<()> {
    let conn = open_readonly(candidate)?;

    let quick_check: String = conn
        .query_row("PRAGMA quick_check;", [], |row| row.get(0))
        .map_err(|e| integrity_error(format!("quick_check failed: {e}")))?;
    if quick_check != "ok" {
        return Err(integrity_error(format!(
            "quick_check reported corruption: {quick_check}"
        )));
    }

    // foreign_key_check returns zero rows when all FK constraints hold.
    let fk_violations: Vec<(String, i64, String, String)> = conn
        .prepare("PRAGMA foreign_key_check;")
        .map_err(|e| integrity_error(format!("foreign_key_check prepare failed: {e}")))?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2).unwrap_or_default(),
                row.get::<_, String>(3).unwrap_or_default(),
            ))
        })
        .map_err(|e| integrity_error(format!("foreign_key_check query failed: {e}")))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| integrity_error(format!("foreign_key_check collect failed: {e}")))?;
    if !fk_violations.is_empty() {
        return Err(integrity_error(format!(
            "foreign_key_check reported {} violation(s)",
            fk_violations.len()
        )));
    }

    Ok(())
}

/// Open a SQLite database file read-only. Uses `OpenMode` read-only so the
/// candidate is never mutated during validation.
fn open_readonly(path: &Path) -> CommandResult<Connection> {
    let flags =
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    Connection::open_with_flags(path, flags)
        .map_err(|e| integrity_error(format!("failed to open candidate read-only: {e}")))
}

/// Verify schema compatibility: the `songs` table must exist. This mirrors
/// the `column_exists` helper in `cache::mod.rs` but operates on a read-only
/// candidate connection.
fn verify_schema_compatibility(candidate: &Path) -> CommandResult<()> {
    let conn = open_readonly(candidate)?;
    let table_exists: bool = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='songs' LIMIT 1;")
        .map_err(|e| integrity_error(format!("schema check prepare failed: {e}")))?
        .exists::<&[&dyn rusqlite::ToSql]>(&[])
        .map_err(|e| integrity_error(format!("schema check query failed: {e}")))?;
    if !table_exists {
        return Err(integrity_error(
            "candidate database is missing the required 'songs' table",
        ));
    }
    // Reuse the shared column-exists helper to confirm the schema is not a
    // bare stub. `column_exists` validates the identifier shape, so this is
    // safe even on an untrusted candidate.
    let has_hash_column = cache::column_exists(&conn, "songs", "hash")
        .map_err(|e| integrity_error(format!("schema check query failed: {e}")))?;
    if !has_hash_column {
        return Err(integrity_error(
            "candidate database 'songs' table is missing the 'hash' column",
        ));
    }
    Ok(())
}

/// Build a `remote_integrity_failed` error carrying the sanitized code string.
fn integrity_error(detail: impl std::fmt::Display) -> CommandError {
    // The full typed error enum is PR #4's job. PR #3 uses this stable code
    // string so recovery and UI can branch without coupling to a future enum.
    internal_error(format!("{REMOTE_INTEGRITY_FAILED}: {detail}"))
}

/// Update the control DB repository state after a successful database pull.
/// Sets `local_db_digest` to the new digest and `local_state = Clean`.
/// `committed_generation` is left as-is; PR #4 advances it from the manifest.
fn update_repository_state_after_pull(
    connection: &Connection,
    library_id: &str,
    new_digest: &str,
) -> CommandResult<()> {
    let now = crate::remote::types::current_unix_time_ms();
    let row = match get_repository_state(connection, library_id)? {
        Some(mut row) => {
            row.local_db_digest = Some(new_digest.to_owned());
            row.local_state = LocalState::Clean;
            row.last_success_at_ms = Some(now);
            row.last_error_code = None;
            row.updated_at_ms = now;
            row
        }
        None => RepositoryStateRow {
            library_id: library_id.to_owned(),
            committed_generation: 0,
            committed_manifest_revision: None,
            local_base_generation: 0,
            local_db_digest: Some(new_digest.to_owned()),
            local_state: LocalState::Clean,
            active_operation_id: None,
            last_success_at_ms: Some(now),
            last_error_code: None,
            updated_at_ms: now,
            repository_id: None,
            writer_id: None,
        },
    };
    upsert_repository_state(connection, &row)
}

/// Reconcile repository state after a restart that completed the rename but
/// not the local-state update.
///
/// If the active `openkara.db` digest differs from the recorded
/// `local_db_digest`, the rename happened but the control DB was not updated.
/// This updates `local_db_digest` to match the now-active database and marks
/// the repository `Clean` (the candidate already passed integrity checks
/// before the rename in the prior run).
///
/// Returns `Some(new_digest)` when reconciliation updated state, `None` when
/// the recorded digest already matched (or no state row / no active DB
/// exists).
pub(crate) fn reconcile_database_state_after_restart(
    control_db_conn: &Connection,
    library_root: &LibraryRoot,
    library_id: &str,
) -> CommandResult<Option<String>> {
    let Some(state) = get_repository_state(control_db_conn, library_id)? else {
        return Ok(None);
    };
    let db_path = library_root.database_path();
    if !db_path.exists() {
        return Ok(None);
    }
    let actual_digest = sha256_file(&db_path)?;
    if state.local_db_digest.as_deref() == Some(actual_digest.as_str()) {
        return Ok(None);
    }
    update_repository_state_after_pull(control_db_conn, library_id, &actual_digest)?;
    Ok(Some(actual_digest))
}

/// Remove stale `*.part.*` temp files from a working-copy directory tree.
///
/// Called during the startup recovery pass. In PR #3 there are no async
/// transfers, so every `*.part.*` file is stale. PR #5's running transfers
/// must be excluded before removal — see the TODO seam.
///
/// Scans recursively so part files in subdirectories (`media/`, `stems/`,
/// `artwork/`) are recovered too.
///
/// Returns the list of removed paths so callers/tests can observe the result.
pub(crate) fn remove_stale_part_files(working_copy_dir: &Path) -> CommandResult<Vec<PathBuf>> {
    let mut removed = Vec::new();
    remove_stale_part_files_recursive(working_copy_dir, &mut removed)?;
    Ok(removed)
}

fn remove_stale_part_files_recursive(dir: &Path, removed: &mut Vec<PathBuf>) -> CommandResult<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(internal_error(format!(
                "failed to read working copy dir {}: {e}",
                dir.display()
            )))
        }
    };
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if file_type.is_dir() {
            // Recurse into subdirectories (media, stems, artwork, etc.).
            // Skip the library marker and hidden directories.
            remove_stale_part_files_recursive(&path, removed)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if is_part_file(file_name) {
            // TODO(PR#5): exclude temp files belonging to currently-running
            // transfers. PR #3 has no async transfers, so all *.part.* files
            // are stale and safe to remove.
            if fs::remove_file(&path).is_ok() {
                removed.push(path);
            }
        }
    }
    Ok(())
}

/// A file is a stale partial-download temp file if its name contains the
/// `.part.` marker. This matches both `<name>.part.<op>` (asset downloads)
/// and `openkara.db.part.<op>` (database pulls).
fn is_part_file(file_name: &str) -> bool {
    file_name.contains(".part.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::error::CommandError;
    use crate::library::error::LibraryError;
    use crate::library_root::LibraryRoot;
    use crate::remote::control_db::{open_control_db, upsert_repository_state};
    use rusqlite::Connection;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    // ---- Test fake provider ----

    /// A scriptable fake provider. Each download invocation pops the next
    /// behavior from a queue so tests can simulate connection drops, short
    /// bodies, wrong sizes, corrupt pages, etc.
    struct FakeProvider {
        files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        /// Queue of behaviors per download attempt (by relative_path).
        behaviors: Arc<Mutex<Vec<DownloadBehavior>>>,
        sizes: Arc<Mutex<HashMap<String, u64>>>,
    }

    #[derive(Clone)]
    enum DownloadBehavior {
        /// Write the full stored bytes successfully.
        Success,
        /// Fail before writing anything (simulates connection drop before
        /// headers).
        FailBeforeWrite,
        /// Write only the first N bytes then fail (simulates mid-body drop).
        PartialThenFail(usize),
        /// Write a short body and succeed (simulates a server returning a
        /// truncated body with success status).
        ShortBody(usize),
        /// Write bytes that differ from the stored content (wrong digest).
        WrongDigest,
    }

    impl FakeProvider {
        fn new() -> Self {
            Self {
                files: Arc::new(Mutex::new(HashMap::new())),
                behaviors: Arc::new(Mutex::new(Vec::new())),
                sizes: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        fn store_file(&self, relative_path: &str, bytes: Vec<u8>) {
            self.sizes
                .lock()
                .unwrap()
                .insert(relative_path.to_owned(), bytes.len() as u64);
            self.files
                .lock()
                .unwrap()
                .insert(relative_path.to_owned(), bytes);
        }

        fn queue_behavior(&self, behavior: DownloadBehavior) {
            self.behaviors.lock().unwrap().push(behavior);
        }

        fn next_behavior(&self) -> DownloadBehavior {
            self.behaviors
                .lock()
                .unwrap()
                .pop()
                .unwrap_or(DownloadBehavior::Success)
        }
    }

    impl crate::remote::provider::RemoteProvider for FakeProvider {
        fn get_revision(&self, _relative_path: &str) -> CommandResult<Option<String>> {
            Ok(Some("rev-1".to_owned()))
        }

        fn download_file(&self, relative_path: &str, destination: &Path) -> CommandResult<()> {
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let behavior = self.next_behavior();
            match behavior {
                DownloadBehavior::Success => {
                    let data = self.files.lock().unwrap().get(relative_path).cloned();
                    let data = data.ok_or_else(|| {
                        CommandError::from(LibraryError::Internal(format!(
                            "fake provider: file {relative_path} not found"
                        )))
                    })?;
                    std::fs::write(destination, &data).map_err(|e| {
                        CommandError::from(LibraryError::Internal(format!(
                            "fake provider write failed: {e}"
                        )))
                    })?;
                    Ok(())
                }
                DownloadBehavior::FailBeforeWrite => {
                    Err(CommandError::from(LibraryError::Internal(
                        "fake provider: connection dropped before headers".to_owned(),
                    )))
                }
                DownloadBehavior::PartialThenFail(n) => {
                    let data = self.files.lock().unwrap().get(relative_path).cloned();
                    let data = data.ok_or_else(|| {
                        CommandError::from(LibraryError::Internal(format!(
                            "fake provider: file {relative_path} not found"
                        )))
                    })?;
                    let truncated = &data[..n.min(data.len())];
                    std::fs::write(destination, truncated).ok();
                    Err(CommandError::from(LibraryError::Internal(
                        "fake provider: connection dropped mid-body".to_owned(),
                    )))
                }
                DownloadBehavior::ShortBody(n) => {
                    let data = self.files.lock().unwrap().get(relative_path).cloned();
                    let data = data.ok_or_else(|| {
                        CommandError::from(LibraryError::Internal(format!(
                            "fake provider: file {relative_path} not found"
                        )))
                    })?;
                    let short = &data[..n.min(data.len())];
                    std::fs::write(destination, short).map_err(|e| {
                        CommandError::from(LibraryError::Internal(format!(
                            "fake provider write failed: {e}"
                        )))
                    })?;
                    Ok(())
                }
                DownloadBehavior::WrongDigest => {
                    let data = self.files.lock().unwrap().get(relative_path).cloned();
                    let data = data.ok_or_else(|| {
                        CommandError::from(LibraryError::Internal(format!(
                            "fake provider: file {relative_path} not found"
                        )))
                    })?;
                    // Flip the first byte so the digest differs.
                    let mut modified = data.clone();
                    if !modified.is_empty() {
                        modified[0] ^= 0xff;
                    }
                    std::fs::write(destination, &modified).map_err(|e| {
                        CommandError::from(LibraryError::Internal(format!(
                            "fake provider write failed: {e}"
                        )))
                    })?;
                    Ok(())
                }
            }
        }

        fn upload_file(&self, _relative_path: &str) -> CommandResult<()> {
            Ok(())
        }
        fn upload_directory(&self, _relative_path: &str) -> CommandResult<()> {
            Ok(())
        }
        fn delete_path(&self, _relative_path: &str) -> CommandResult<()> {
            Ok(())
        }
        fn initialize_or_sync(&self) -> CommandResult<Option<String>> {
            Ok(None)
        }
        fn get_file_size(&self, relative_path: &str) -> CommandResult<Option<u64>> {
            Ok(self
                .sizes
                .lock()
                .unwrap()
                .get(relative_path)
                .copied()
                .or_else(|| {
                    self.files
                        .lock()
                        .unwrap()
                        .get(relative_path)
                        .map(|d| d.len() as u64)
                }))
        }
        fn refresh_existing(&self) -> CommandResult<Option<String>> {
            Ok(None)
        }
    }

    fn fresh_library_root() -> (TempDir, LibraryRoot) {
        let dir = TempDir::new().expect("temp dir");
        let root = LibraryRoot::create(&dir.path().join("lib")).expect("create library root");
        (dir, root)
    }

    fn fresh_control_db() -> (TempDir, Connection) {
        let dir = TempDir::new().expect("temp dir");
        let conn = open_control_db(&dir.path().join("remote-state.db")).expect("open control db");
        (dir, conn)
    }

    fn make_valid_library_db(path: &Path) {
        crate::cache::initialize_library_database(path).expect("initialize library db");
    }

    // ---- atomic_download (generic asset) tests ----

    #[test]
    fn atomic_download_writes_valid_file_to_destination() {
        let dir = TempDir::new().expect("temp dir");
        let dest = dir.path().join("media/song.mp3");
        let provider = FakeProvider::new();
        provider.store_file("media/song.mp3", b"hello world".to_vec());

        atomic_download(
            &provider,
            AtomicDownloadOptions {
                relative_path: "media/song.mp3",
                destination: &dest,
                expected_size: Some(11),
                expected_digest: None,
                operation_id: "op-1",
            },
        )
        .expect("download succeeds");

        assert_eq!(std::fs::read(&dest).unwrap(), b"hello world");
        // No temp file lingers.
        assert!(!part_path(&dest, "op-1").exists());
    }

    #[test]
    fn atomic_download_connection_drop_before_headers_leaves_no_final_file() {
        let dir = TempDir::new().expect("temp dir");
        let dest = dir.path().join("media/song.mp3");
        let provider = FakeProvider::new();
        provider.store_file("media/song.mp3", b"hello world".to_vec());
        provider.queue_behavior(DownloadBehavior::FailBeforeWrite);

        let result = atomic_download(
            &provider,
            AtomicDownloadOptions {
                relative_path: "media/song.mp3",
                destination: &dest,
                expected_size: None,
                expected_digest: None,
                operation_id: "op-1",
            },
        );

        assert!(result.is_err());
        assert!(!dest.exists(), "no truncated final file");
        assert!(!part_path(&dest, "op-1").exists(), "temp cleaned up");
    }

    #[test]
    fn atomic_download_mid_body_drop_leaves_no_final_file() {
        let dir = TempDir::new().expect("temp dir");
        let dest = dir.path().join("media/song.mp3");
        let provider = FakeProvider::new();
        provider.store_file("media/song.mp3", b"hello world".to_vec());
        provider.queue_behavior(DownloadBehavior::PartialThenFail(5));

        let result = atomic_download(
            &provider,
            AtomicDownloadOptions {
                relative_path: "media/song.mp3",
                destination: &dest,
                expected_size: None,
                expected_digest: None,
                operation_id: "op-1",
            },
        );

        assert!(result.is_err());
        assert!(!dest.exists(), "no truncated final file");
        assert!(!part_path(&dest, "op-1").exists(), "temp cleaned up");
    }

    #[test]
    fn atomic_download_short_body_rejected_by_size_check() {
        let dir = TempDir::new().expect("temp dir");
        let dest = dir.path().join("media/song.mp3");
        let provider = FakeProvider::new();
        provider.store_file("media/song.mp3", b"hello world".to_vec());
        provider.queue_behavior(DownloadBehavior::ShortBody(5));

        let result = atomic_download(
            &provider,
            AtomicDownloadOptions {
                relative_path: "media/song.mp3",
                destination: &dest,
                expected_size: Some(11),
                expected_digest: None,
                operation_id: "op-1",
            },
        );

        assert!(result.is_err());
        assert!(!dest.exists(), "no truncated final file");
    }

    #[test]
    fn atomic_download_wrong_digest_rejected() {
        let dir = TempDir::new().expect("temp dir");
        let dest = dir.path().join("media/song.mp3");
        let provider = FakeProvider::new();
        provider.store_file("media/song.mp3", b"hello world".to_vec());
        let expected_digest = sha256_bytes(b"hello world");
        provider.queue_behavior(DownloadBehavior::WrongDigest);

        let result = atomic_download(
            &provider,
            AtomicDownloadOptions {
                relative_path: "media/song.mp3",
                destination: &dest,
                expected_size: None,
                expected_digest: Some(&expected_digest),
                operation_id: "op-1",
            },
        );

        assert!(result.is_err());
        assert!(!dest.exists(), "no final file on digest mismatch");
    }

    #[test]
    fn atomic_download_disk_full_during_write_leaves_no_final_file() {
        let dir = TempDir::new().expect("temp dir");
        let dest = dir.path().join("media/song.mp3");
        let provider = FakeProvider::new();
        provider.store_file("media/song.mp3", b"hello world".to_vec());
        // Simulate disk-full: write partial bytes then fail.
        provider.queue_behavior(DownloadBehavior::PartialThenFail(5));

        let result = atomic_download(
            &provider,
            AtomicDownloadOptions {
                relative_path: "media/song.mp3",
                destination: &dest,
                expected_size: None,
                expected_digest: None,
                operation_id: "op-1",
            },
        );

        assert!(result.is_err());
        assert!(!dest.exists(), "no truncated final file after disk-full");
    }

    // ---- atomic_database_pull tests ----

    #[test]
    fn database_pull_installs_valid_candidate_and_preserves_lkg() {
        let (lib_dir, root) = fresh_library_root();
        let (_db_dir, conn) = fresh_control_db();
        // Seed an existing working DB.
        make_valid_library_db(&root.database_path());
        let old_bytes = std::fs::read(root.database_path()).unwrap();

        // Build a new valid candidate DB.
        let candidate_path = lib_dir.path().join("candidate.db");
        make_valid_library_db(&candidate_path);
        // Modify it so the digest differs from the old one.
        {
            let c = Connection::open(&candidate_path).unwrap();
            c.execute_batch("CREATE TABLE IF NOT EXISTS pr3_marker (x INTEGER);")
                .unwrap();
            c.execute("INSERT INTO pr3_marker VALUES (1);", []).unwrap();
        }
        let candidate_bytes = std::fs::read(&candidate_path).unwrap();

        let provider = FakeProvider::new();
        provider.store_file("openkara.db", candidate_bytes.clone());

        let result = atomic_database_pull(
            &provider,
            &conn,
            &root,
            DatabasePullOptions {
                operation_id: "pull-1",
                expected_size: Some(candidate_bytes.len() as u64),
                expected_digest: None,
                library_id: "lib-1",
            },
        )
        .expect("pull succeeds");

        // Active DB is the new candidate.
        let active = std::fs::read(root.database_path()).unwrap();
        assert_eq!(active, candidate_bytes);
        // LKG is the old verified DB.
        let lkg = std::fs::read(last_known_good_path(&root.database_path())).unwrap();
        assert_eq!(lkg, old_bytes);
        // No temp lingers.
        assert!(!part_path(&root.database_path(), "pull-1").exists());
        // Control DB state updated.
        let state = crate::remote::control_db::get_repository_state(&conn, "lib-1")
            .unwrap()
            .unwrap();
        assert_eq!(state.local_state, LocalState::Clean);
        assert_eq!(
            state.local_db_digest.as_deref(),
            Some(result.new_digest.as_str())
        );
    }

    #[test]
    fn database_pull_corrupt_sqlite_rejected_and_lkg_remains_usable() {
        let (lib_dir, root) = fresh_library_root();
        let (_db_dir, conn) = fresh_control_db();
        make_valid_library_db(&root.database_path());
        let old_bytes = std::fs::read(root.database_path()).unwrap();

        // Build a corrupt candidate: valid header then garbage.
        let candidate_path = lib_dir.path().join("candidate.db");
        make_valid_library_db(&candidate_path);
        let mut corrupt = std::fs::read(&candidate_path).unwrap();
        // Overwrite a page in the middle to break integrity.
        let mid = corrupt.len() / 2;
        let end = (mid + 64).min(corrupt.len());
        for byte in &mut corrupt[mid..end] {
            *byte = 0xff;
        }

        let provider = FakeProvider::new();
        provider.store_file("openkara.db", corrupt);

        let result = atomic_database_pull(
            &provider,
            &conn,
            &root,
            DatabasePullOptions {
                operation_id: "pull-1",
                expected_size: None,
                expected_digest: None,
                library_id: "lib-1",
            },
        );

        assert!(result.is_err(), "corrupt candidate must be rejected");
        // Active DB is still the old verified version.
        let active = std::fs::read(root.database_path()).unwrap();
        assert_eq!(active, old_bytes, "active DB unchanged");
        // No LKG was created (nothing to preserve from the failed pull — the
        // old DB stayed in place).
        assert!(!last_known_good_path(&root.database_path()).exists());
        // No temp lingers.
        assert!(!part_path(&root.database_path(), "pull-1").exists());
    }

    #[test]
    fn database_pull_incompatible_schema_missing_songs_rejected() {
        let (lib_dir, root) = fresh_library_root();
        let (_db_dir, conn) = fresh_control_db();
        make_valid_library_db(&root.database_path());
        let old_bytes = std::fs::read(root.database_path()).unwrap();

        // Build a candidate with no `songs` table.
        let candidate_path = lib_dir.path().join("candidate.db");
        {
            let c = Connection::open(&candidate_path).unwrap();
            c.execute_batch("CREATE TABLE other_table (x INTEGER);")
                .unwrap();
        }
        let candidate_bytes = std::fs::read(&candidate_path).unwrap();

        let provider = FakeProvider::new();
        provider.store_file("openkara.db", candidate_bytes);

        let result = atomic_database_pull(
            &provider,
            &conn,
            &root,
            DatabasePullOptions {
                operation_id: "pull-1",
                expected_size: None,
                expected_digest: None,
                library_id: "lib-1",
            },
        );

        assert!(result.is_err(), "incompatible schema must be rejected");
        let active = std::fs::read(root.database_path()).unwrap();
        assert_eq!(active, old_bytes, "active DB unchanged");
    }

    #[test]
    fn database_pull_wrong_size_rejected() {
        let (lib_dir, root) = fresh_library_root();
        let (_db_dir, conn) = fresh_control_db();
        make_valid_library_db(&root.database_path());

        let candidate_path = lib_dir.path().join("candidate.db");
        make_valid_library_db(&candidate_path);
        let candidate_bytes = std::fs::read(&candidate_path).unwrap();
        let candidate_len = candidate_bytes.len() as u64;

        let provider = FakeProvider::new();
        provider.store_file("openkara.db", candidate_bytes);

        let result = atomic_database_pull(
            &provider,
            &conn,
            &root,
            DatabasePullOptions {
                operation_id: "pull-1",
                expected_size: Some(candidate_len + 100),
                expected_digest: None,
                library_id: "lib-1",
            },
        );

        assert!(result.is_err(), "wrong size must be rejected");
    }

    #[test]
    fn database_pull_mid_body_drop_leaves_old_db_active() {
        let (lib_dir, root) = fresh_library_root();
        let (_db_dir, conn) = fresh_control_db();
        make_valid_library_db(&root.database_path());
        let old_bytes = std::fs::read(root.database_path()).unwrap();

        let candidate_path = lib_dir.path().join("candidate.db");
        make_valid_library_db(&candidate_path);
        let candidate_bytes = std::fs::read(&candidate_path).unwrap();

        let provider = FakeProvider::new();
        provider.store_file("openkara.db", candidate_bytes);
        provider.queue_behavior(DownloadBehavior::PartialThenFail(10));

        let result = atomic_database_pull(
            &provider,
            &conn,
            &root,
            DatabasePullOptions {
                operation_id: "pull-1",
                expected_size: None,
                expected_digest: None,
                library_id: "lib-1",
            },
        );

        assert!(result.is_err());
        let active = std::fs::read(root.database_path()).unwrap();
        assert_eq!(active, old_bytes, "old DB remains active");
        assert!(
            !part_path(&root.database_path(), "pull-1").exists(),
            "temp cleaned"
        );
    }

    #[test]
    fn database_pull_first_pull_no_lkg_created() {
        let (_lib_dir, root) = fresh_library_root();
        let (_db_dir, conn) = fresh_control_db();
        // No existing working DB on first pull.
        assert!(!root.database_path().exists());

        let candidate_path = root.root().join("candidate.db");
        make_valid_library_db(&candidate_path);
        let candidate_bytes = std::fs::read(&candidate_path).unwrap();

        let provider = FakeProvider::new();
        provider.store_file("openkara.db", candidate_bytes.clone());

        atomic_database_pull(
            &provider,
            &conn,
            &root,
            DatabasePullOptions {
                operation_id: "pull-1",
                expected_size: None,
                expected_digest: None,
                library_id: "lib-1",
            },
        )
        .expect("first pull succeeds");

        assert!(root.database_path().exists());
        assert!(
            !last_known_good_path(&root.database_path()).exists(),
            "no LKG on first pull"
        );
    }

    // ---- Restart reconciliation tests ----

    #[test]
    fn reconcile_after_restart_updates_stale_digest() {
        let (_lib_dir, root) = fresh_library_root();
        let (_db_dir, conn) = fresh_control_db();
        make_valid_library_db(&root.database_path());
        let digest = sha256_file(&root.database_path()).unwrap();

        // Record a stale digest (simulates rename happened, state update did not).
        upsert_repository_state(
            &conn,
            &RepositoryStateRow {
                library_id: "lib-1".to_owned(),
                committed_generation: 0,
                committed_manifest_revision: None,
                local_base_generation: 0,
                local_db_digest: Some("stale-digest".to_owned()),
                local_state: LocalState::Dirty,
                active_operation_id: None,
                last_success_at_ms: None,
                last_error_code: None,
                updated_at_ms: 1000,
                repository_id: None,
                writer_id: None,
            },
        )
        .unwrap();

        let updated =
            reconcile_database_state_after_restart(&conn, &root, "lib-1").expect("reconcile");
        assert_eq!(updated.as_deref(), Some(digest.as_str()));

        let state = crate::remote::control_db::get_repository_state(&conn, "lib-1")
            .unwrap()
            .unwrap();
        assert_eq!(state.local_db_digest.as_deref(), Some(digest.as_str()));
        assert_eq!(state.local_state, LocalState::Clean);
    }

    #[test]
    fn reconcile_after_restart_noop_when_digest_matches() {
        let (_lib_dir, root) = fresh_library_root();
        let (_db_dir, conn) = fresh_control_db();
        make_valid_library_db(&root.database_path());
        let digest = sha256_file(&root.database_path()).unwrap();

        upsert_repository_state(
            &conn,
            &RepositoryStateRow {
                library_id: "lib-1".to_owned(),
                committed_generation: 0,
                committed_manifest_revision: None,
                local_base_generation: 0,
                local_db_digest: Some(digest.clone()),
                local_state: LocalState::Clean,
                active_operation_id: None,
                last_success_at_ms: None,
                last_error_code: None,
                updated_at_ms: 1000,
                repository_id: None,
                writer_id: None,
            },
        )
        .unwrap();

        let updated =
            reconcile_database_state_after_restart(&conn, &root, "lib-1").expect("reconcile");
        assert!(updated.is_none(), "no update when digest already matches");
    }

    // ---- Stale partial recovery tests ----

    #[test]
    fn remove_stale_part_files_deletes_part_files_only() {
        let dir = TempDir::new().expect("temp dir");
        let part1 = dir.path().join("openkara.db.part.pull-1");
        let part2 = dir.path().join("media/song.mp3.part.op-2");
        std::fs::write(&part1, b"partial").unwrap();
        std::fs::create_dir_all(part2.parent().unwrap()).unwrap();
        std::fs::write(&part2, b"partial").unwrap();
        let keep = dir.path().join("openkara.db");
        std::fs::write(&keep, b"real db").unwrap();
        let lkg = dir.path().join("openkara.db.lkg");
        std::fs::write(&lkg, b"lkg").unwrap();

        let removed = remove_stale_part_files(dir.path()).expect("recovery");

        assert!(!part1.exists());
        assert!(!part2.exists());
        assert!(keep.exists(), "real db preserved");
        assert!(lkg.exists(), "lkg preserved");
        assert_eq!(removed.len(), 2);
    }

    #[test]
    fn remove_stale_part_files_missing_dir_is_not_an_error() {
        let result = remove_stale_part_files(Path::new("/nonexistent/dir/xyz"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    // ---- Helper tests ----

    #[test]
    fn part_path_naming() {
        let dest = Path::new("/tmp/lib/openkara.db");
        assert_eq!(
            part_path(dest, "pull-1"),
            PathBuf::from("/tmp/lib/openkara.db.part.pull-1")
        );
    }

    #[test]
    fn is_part_file_detects_marker() {
        assert!(is_part_file("openkara.db.part.pull-1"));
        assert!(is_part_file("song.mp3.part.op-2"));
        assert!(!is_part_file("openkara.db"));
        assert!(!is_part_file("openkara.db.lkg"));
    }

    fn sha256_bytes(data: &[u8]) -> String {
        // Helper for computing expected digests in tests.
        let mut hasher = Sha256::new();
        hasher.update(data);
        hash::hex_lower(hasher.finalize())
    }
}
