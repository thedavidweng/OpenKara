//! Publish Changes writer: commit the Local Working Copy to the remote
//! repository.
//!
//! Publication is three steps against the [`RepositoryStorage`] port:
//!
//! 1. Upload every asset the snapshot references but the remote lacks
//!    ([`ensure_referenced_assets`]). Asset objects are content-addressed and
//!    immutable, so an existing object of the right size is skipped and a torn
//!    leftover from an interrupted upload is deleted and rewritten
//!    (`write_new` never replaces).
//! 2. Upload a snapshot of the working database at its operation-scoped
//!    generation path (`.openkara/databases/<generation>/<hash>.sqlite`) and
//!    verify the stored bytes against the candidate digest.
//! 3. Replace the manifest conditionally on the revision observed when the
//!    publication started, so a concurrent writer is detected instead of
//!    overwritten.
//!
//! The manifest is the only commit point: a crash at any earlier step leaves
//! unreferenced objects next to the previous, still-consistent manifest, and
//! running the same publication again finishes the job. No journal state is
//! kept — the operation identity and expected generation arrive with every
//! call and idempotence does the rest. A manifest that already carries this
//! operation's id at the expected generation + 1 is recognized as an accepted
//! commit (the crash window between CAS and local completion) and reported as
//! [`PublishOutcome::AlreadyCommitted`].
//!
//! A stale expected generation or a failed manifest CAS surfaces as
//! `RemoteConflict` — the Pre-Publish Conflict the application turns into its
//! conflicted state. Song-scoped rebase is a later ticket; until then every
//! revision move conflicts.
//!
//! The writer never decides *whether* publishing is allowed; the application
//! owns that policy along with its operation bookkeeping.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::error::{RemoteError, RemoteErrorKind, RemoteResult};
use crate::manifest::{
    database_path_for_operation, RepositoryManifest, CURRENT_SCHEMA_VERSION, MANIFEST_PATH,
};
use crate::reader::{
    hex_lower, integrity_error, io_error, open_readonly, sha256_file, verify_sqlite_candidate,
};
use crate::storage::{validate_object_path, RepositoryStorage};

/// Managed top-level directories a database row may reference. Paths outside
/// them are rejected before they reach the storage.
const ASSET_TOP_LEVEL_DIRS: &[&str] = &["media", "media-g", "stems", "artwork"];

/// Everything the writer needs from the application for one publication.
///
/// `operation_id` scopes the snapshot object so two writers never upload to
/// the same path, and recognizes the publisher's own accepted commit after a
/// crash. `expected_generation` is the generation the working copy was based
/// on; the publication commits `expected_generation + 1` or conflicts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishRequest {
    pub repository_id: String,
    pub writer_id: String,
    pub operation_id: String,
    pub expected_generation: i64,
}

/// The manifest-visible identity of a committed generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedSnapshot {
    pub generation: i64,
    pub database_path: String,
    pub database_size_bytes: u64,
    pub database_sha256: String,
    /// Manifest revision after the commit, for the caller to store as its new
    /// Remote Revision baseline.
    pub manifest_revision: Option<String>,
}

/// The result of a [`publish_changes`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishOutcome {
    /// The manifest moved to the new generation during this call.
    Committed(CommittedSnapshot),
    /// The manifest already carried this operation's commit (a previous call
    /// crashed after the CAS but before the caller learned about it).
    AlreadyCommitted(CommittedSnapshot),
}

impl PublishOutcome {
    pub fn snapshot(&self) -> &CommittedSnapshot {
        match self {
            Self::Committed(snapshot) | Self::AlreadyCommitted(snapshot) => snapshot,
        }
    }
}

/// Coarse publication milestones, reported so the caller can mirror progress
/// into its own status projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishStep {
    ReadManifest,
    UploadAssets,
    PrepareSnapshot,
    UploadSnapshot,
    CommitManifest,
    VerifyManifest,
}

/// Publish the working copy database and the assets it references.
///
/// `working_database` is the library database inside `working_copy_root`;
/// asset paths referenced by its rows resolve against `working_copy_root`.
/// The database bytes are snapshotted before anything uploads, so a
/// publication never mixes pre- and post-mutation state.
///
/// Errors are split the way the recovery contract needs: retryable transfer
/// failures propagate unchanged, a stale generation or lost CAS race is
/// `RemoteConflict`, and corrupt local or remote bytes are
/// `RemoteIntegrityFailed`.
pub fn publish_changes(
    storage: &dyn RepositoryStorage,
    working_copy_root: &Path,
    working_database: &Path,
    request: &PublishRequest,
    on_step: &mut dyn FnMut(PublishStep),
) -> RemoteResult<PublishOutcome> {
    if !storage.capabilities().can_publish() {
        return Err(RemoteError::new(
            RemoteErrorKind::ProviderCapabilityUnavailable,
            "storage does not support conditional replace; publication is blocked",
        ));
    }

    on_step(PublishStep::ReadManifest);
    let prior = read_current_manifest(storage)?;
    let target_generation = request.expected_generation + 1;

    if let Some((manifest, revision)) = &prior {
        if manifest.generation == target_generation
            && !manifest.operation_id.is_empty()
            && manifest.operation_id == request.operation_id
        {
            return resolve_accepted_commit(
                working_copy_root,
                working_database,
                request,
                manifest,
                revision.clone(),
                on_step,
            );
        }
        if manifest.generation != request.expected_generation {
            return Err(pre_publish_conflict(
                request.expected_generation,
                manifest.generation,
            ));
        }
    } else if request.expected_generation != 0 {
        return Err(pre_publish_conflict(request.expected_generation, 0));
    }

    on_step(PublishStep::UploadAssets);
    ensure_referenced_assets(storage, working_copy_root, working_database)?;

    on_step(PublishStep::PrepareSnapshot);
    let candidate = prepare_candidate(working_copy_root, working_database, &request.operation_id)?;

    let result = commit_candidate(
        storage,
        request,
        prior.as_ref(),
        target_generation,
        &candidate,
        on_step,
    );
    if result.is_ok() {
        let _ = fs::remove_file(&candidate.path);
    }
    result
}

/// Ensure every asset the database references exists remotely.
///
/// Each referenced path resolves to a file under `working_copy_root`. An
/// object already remote at the right size is left untouched, a missing one
/// is uploaded, and a size-mismatched one is replaced only when its
/// content-addressed name proves the local bytes are the intended content;
/// anything else fails closed. A referenced asset that no longer exists
/// locally is accepted when the remote already has it (it was published by an
/// earlier run).
pub fn ensure_referenced_assets(
    storage: &dyn RepositoryStorage,
    working_copy_root: &Path,
    working_database: &Path,
) -> RemoteResult<()> {
    for path in collect_referenced_asset_paths(working_database)? {
        ensure_asset(storage, working_copy_root, &path)?;
    }
    Ok(())
}

/// The manifest plus the revision token a conditional replace needs, or
/// `None` for a repository that has no manifest yet (legacy or brand new).
fn read_current_manifest(
    storage: &dyn RepositoryStorage,
) -> RemoteResult<Option<(RepositoryManifest, Option<String>)>> {
    let Some(metadata) = storage.stat(MANIFEST_PATH)? else {
        return Ok(None);
    };
    if metadata.size_bytes == 0 {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    storage.read(MANIFEST_PATH, &mut bytes)?;
    let json = std::str::from_utf8(&bytes).map_err(|error| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!("repository manifest is not valid UTF-8: {error}"),
        )
    })?;
    let manifest = RepositoryManifest::from_json(json)?;
    // Fail closed: without a revision token the CAS would silently degrade
    // into an unconditional replace.
    if metadata.revision.is_none() {
        return Err(RemoteError::new(
            RemoteErrorKind::ProviderCapabilityUnavailable,
            "manifest exists but the storage reported no revision token; \
             cannot safely replace without a compare-and-swap guard",
        ));
    }
    Ok(Some((manifest, metadata.revision)))
}

/// The manifest already names this operation at the target generation: the
/// commit landed but the caller never learned. Accept it when the freshly
/// snapshotted working database still holds exactly the committed bytes;
/// otherwise the working copy moved on and this is an ordinary conflict.
fn resolve_accepted_commit(
    working_copy_root: &Path,
    working_database: &Path,
    request: &PublishRequest,
    manifest: &RepositoryManifest,
    manifest_revision: Option<String>,
    on_step: &mut dyn FnMut(PublishStep),
) -> RemoteResult<PublishOutcome> {
    on_step(PublishStep::PrepareSnapshot);
    let candidate = prepare_candidate(working_copy_root, working_database, &request.operation_id)?;
    let accepted = manifest.database_sha256 == candidate.digest
        && manifest.database_size_bytes == candidate.size;
    let _ = fs::remove_file(&candidate.path);
    let snapshot = CommittedSnapshot {
        generation: manifest.generation,
        database_path: manifest.database_path.clone(),
        database_size_bytes: manifest.database_size_bytes,
        database_sha256: manifest.database_sha256.clone(),
        manifest_revision,
    };
    if accepted {
        return Ok(PublishOutcome::AlreadyCommitted(snapshot));
    }
    Err(RemoteError::new(
        RemoteErrorKind::RemoteConflict,
        format!(
            "remote generation {} was committed by operation {} with different \
             database bytes; refresh the repository and publish again",
            manifest.generation, manifest.operation_id
        ),
    ))
}

/// Upload the snapshot and move the manifest. The manifest replace is the
/// only commit point, so every failure before it leaves the previous
/// generation consistent.
fn commit_candidate(
    storage: &dyn RepositoryStorage,
    request: &PublishRequest,
    prior: Option<&(RepositoryManifest, Option<String>)>,
    target_generation: i64,
    candidate: &Candidate,
    on_step: &mut dyn FnMut(PublishStep),
) -> RemoteResult<PublishOutcome> {
    let database_path = database_path_for_operation(target_generation, &request.operation_id);

    on_step(PublishStep::UploadSnapshot);
    ensure_snapshot_object(storage, &database_path, candidate)?;

    let manifest = RepositoryManifest {
        schema_version: CURRENT_SCHEMA_VERSION,
        repository_id: request.repository_id.clone(),
        generation: target_generation,
        database_path: database_path.clone(),
        database_size_bytes: candidate.size,
        database_sha256: candidate.digest.clone(),
        committed_at_ms: current_unix_time_ms(),
        writer_id: request.writer_id.clone(),
        operation_id: request.operation_id.clone(),
    };
    let manifest_json = manifest.to_json()?;

    on_step(PublishStep::CommitManifest);
    let committed_metadata = match prior {
        Some((_, Some(revision))) => {
            storage.conditional_replace(MANIFEST_PATH, manifest_json.as_bytes(), revision)?
        }
        Some((_, None)) => {
            return Err(RemoteError::new(
                RemoteErrorKind::ProviderCapabilityUnavailable,
                "manifest exists but the storage reported no revision token",
            ));
        }
        // The first manifest relies on write_new's no-replace guarantee.
        None => storage.write_new(MANIFEST_PATH, manifest_json.as_bytes())?,
    };

    on_step(PublishStep::VerifyManifest);
    verify_committed_manifest(storage, &manifest)?;

    Ok(PublishOutcome::Committed(CommittedSnapshot {
        generation: target_generation,
        database_path,
        database_size_bytes: candidate.size,
        database_sha256: candidate.digest.clone(),
        manifest_revision: committed_metadata.revision,
    }))
}

/// Ensure the operation-scoped snapshot object holds exactly the candidate
/// bytes. An existing object belongs to an earlier attempt of the same
/// operation (the path is operation-scoped), so a mismatch is a torn leftover
/// to delete and rewrite, never a conflict.
fn ensure_snapshot_object(
    storage: &dyn RepositoryStorage,
    remote_path: &str,
    candidate: &Candidate,
) -> RemoteResult<()> {
    if storage.stat(remote_path)?.is_some() {
        if remote_object_matches(storage, remote_path, candidate)? {
            return Ok(());
        }
        storage.delete(remote_path)?;
    }

    let bytes = fs::read(&candidate.path).map_err(|error| {
        io_error(
            format!("failed to read candidate {}", candidate.path.display()),
            error,
        )
    })?;
    match storage.write_new(remote_path, &bytes) {
        Ok(_) => {}
        Err(error) if error.kind == RemoteErrorKind::RemoteConflict => {
            // Another attempt of this very operation won the create race.
            if remote_object_matches(storage, remote_path, candidate)? {
                return Ok(());
            }
            return Err(integrity_error(format!(
                "snapshot object {remote_path} was created concurrently with different bytes"
            )));
        }
        Err(error) => return Err(error),
    }

    if !remote_object_matches(storage, remote_path, candidate)? {
        return Err(integrity_error(format!(
            "uploaded snapshot {remote_path} failed digest verification"
        )));
    }
    Ok(())
}

/// Stream the remote object through a hasher and compare size and digest
/// against the candidate. Size alone misses equal-size corruption.
fn remote_object_matches(
    storage: &dyn RepositoryStorage,
    remote_path: &str,
    candidate: &Candidate,
) -> RemoteResult<bool> {
    let Some(metadata) = storage.stat(remote_path)? else {
        return Ok(false);
    };
    if metadata.size_bytes != candidate.size {
        return Ok(false);
    }
    Ok(sha256_remote_object(storage, remote_path)? == candidate.digest)
}

fn sha256_remote_object(storage: &dyn RepositoryStorage, path: &str) -> RemoteResult<String> {
    struct DigestSink(Sha256);
    impl Write for DigestSink {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0.update(buffer);
            Ok(buffer.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut sink = DigestSink(Sha256::new());
    storage.read(path, &mut sink)?;
    Ok(hex_lower(&sink.0.finalize()))
}

/// The committed manifest must name exactly what this publication wrote.
fn verify_committed_manifest(
    storage: &dyn RepositoryStorage,
    manifest: &RepositoryManifest,
) -> RemoteResult<()> {
    let Some((committed, _)) = read_current_manifest(storage)? else {
        return Err(integrity_error("manifest was not found after commit"));
    };
    if committed.generation != manifest.generation
        || committed.database_path != manifest.database_path
        || committed.database_size_bytes != manifest.database_size_bytes
        || committed.database_sha256 != manifest.database_sha256
        || committed.operation_id != manifest.operation_id
    {
        return Err(integrity_error(
            "manifest verification failed: committed manifest does not match",
        ));
    }
    Ok(())
}

/// A snapshot of the working database with the machine-local publish outbox
/// stripped, verified and digested. The candidate is rebuilt on every call:
/// the only recovery journal is the outbox itself.
struct Candidate {
    path: PathBuf,
    digest: String,
    size: u64,
}

fn prepare_candidate(
    working_copy_root: &Path,
    working_database: &Path,
    operation_id: &str,
) -> RemoteResult<Candidate> {
    let candidate_path = working_copy_root
        .join(".openkara/candidates")
        .join(format!("{operation_id}.sqlite"));
    if let Some(parent) = candidate_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            io_error(
                format!("failed to create directory {}", parent.display()),
                error,
            )
        })?;
    }
    let _ = fs::remove_file(&candidate_path);
    fs::copy(working_database, &candidate_path).map_err(|error| {
        io_error(
            format!(
                "failed to copy the working database {} to the candidate {}",
                working_database.display(),
                candidate_path.display()
            ),
            error,
        )
    })?;
    let result = (|| {
        strip_machine_local_outbox(&candidate_path)?;
        verify_sqlite_candidate(&candidate_path)?;
        let digest = sha256_file(&candidate_path)?;
        let size = fs::metadata(&candidate_path)
            .map(|metadata| metadata.len())
            .map_err(|error| {
                io_error(
                    format!("failed to stat candidate {}", candidate_path.display()),
                    error,
                )
            })?;
        Ok(Candidate {
            path: candidate_path.clone(),
            digest,
            size,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&candidate_path);
    }
    result
}

/// The publish outbox is machine-local journal state and must never ship in
/// a generation snapshot. A missing table means an old candidate schema and
/// is fine; rows surviving the delete are not.
fn strip_machine_local_outbox(candidate: &Path) -> RemoteResult<()> {
    let connection = Connection::open(candidate).map_err(|error| {
        integrity_error(format!(
            "failed to open candidate for outbox sanitation: {error}"
        ))
    })?;
    let result = (|| {
        match connection.execute("DELETE FROM remote_publish_outbox", []) {
            Ok(_) => {}
            Err(error) if is_missing_table(&error) => {}
            Err(error) => {
                return Err(integrity_error(format!(
                    "failed to clear machine-local outbox from candidate: {error}"
                )));
            }
        }
        let remaining: i64 =
            match connection.query_row("SELECT COUNT(*) FROM remote_publish_outbox", [], |row| {
                row.get(0)
            }) {
                Ok(count) => count,
                Err(error) if is_missing_table(&error) => 0,
                Err(error) => {
                    return Err(integrity_error(format!(
                        "failed to verify outbox cleanup on candidate: {error}"
                    )));
                }
            };
        if remaining > 0 {
            return Err(integrity_error(format!(
                "candidate still has {remaining} remote_publish_outbox row(s) after cleanup"
            )));
        }
        Ok(())
    })();
    let close_result = connection.close();
    match (result, close_result) {
        (Err(error), _) => Err(error),
        (Ok(()), Err((_, error))) => Err(integrity_error(format!(
            "failed to close candidate after outbox sanitation: {error}"
        ))),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn is_missing_table(error: &rusqlite::Error) -> bool {
    error.to_string().contains("no such table")
}

/// Upload one referenced asset when the remote lacks it.
fn ensure_asset(
    storage: &dyn RepositoryStorage,
    working_copy_root: &Path,
    relative_path: &str,
) -> RemoteResult<()> {
    validate_referenced_asset_path(relative_path)?;
    let local_path = working_copy_root.join(relative_path);
    let local_size = fs::metadata(&local_path)
        .ok()
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len());
    let remote = storage.stat(relative_path)?;

    let Some(local_size) = local_size else {
        return match remote {
            Some(_) => Ok(()),
            None => Err(integrity_error(format!(
                "asset referenced by database is missing locally and remotely: {relative_path}"
            ))),
        };
    };

    match remote {
        Some(metadata) if metadata.size_bytes == local_size => Ok(()),
        Some(_) => {
            // A size mismatch is a torn leftover from an interrupted upload,
            // but only a content-addressed name proves the local bytes are the
            // content the path promises; anything else fails closed instead of
            // overwriting a foreign object.
            let digest = sha256_file(&local_path)?;
            if !path_claims_digest(relative_path, &digest) {
                return Err(integrity_error(format!(
                    "asset size mismatch for {relative_path}: local {local_size} bytes, \
                     remote {} bytes",
                    storage
                        .stat(relative_path)?
                        .map(|metadata| metadata.size_bytes)
                        .unwrap_or(0)
                )));
            }
            storage.delete(relative_path)?;
            write_asset(storage, &local_path, relative_path, local_size)
        }
        None => write_asset(storage, &local_path, relative_path, local_size),
    }
}

fn write_asset(
    storage: &dyn RepositoryStorage,
    local_path: &Path,
    relative_path: &str,
    local_size: u64,
) -> RemoteResult<()> {
    let bytes = fs::read(local_path).map_err(|error| {
        io_error(
            format!("failed to read asset {}", local_path.display()),
            error,
        )
    })?;
    match storage.write_new(relative_path, &bytes) {
        Ok(_) => {}
        Err(error) if error.kind == RemoteErrorKind::RemoteConflict => {
            // Another writer published the same content-addressed object first.
            return match storage.stat(relative_path)? {
                Some(metadata) if metadata.size_bytes == local_size => Ok(()),
                _ => Err(integrity_error(format!(
                    "asset {relative_path} was created concurrently with different content"
                ))),
            };
        }
        Err(error) => return Err(error),
    }
    match storage.stat(relative_path)? {
        Some(metadata) if metadata.size_bytes == local_size => Ok(()),
        other => Err(integrity_error(format!(
            "remote asset {relative_path} was not found at size {local_size} after upload \
             (stat reports {other:?})"
        ))),
    }
}

/// Whether `path` names its content by digest: the file stem is exactly the
/// 64-hex-character SHA-256 of the local bytes.
fn path_claims_digest(path: &str, digest: &str) -> bool {
    let Some(file_name) = path.rsplit('/').next() else {
        return false;
    };
    let stem = file_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(file_name);
    stem.len() == 64
        && stem.bytes().all(|byte| byte.is_ascii_hexdigit())
        && stem.eq_ignore_ascii_case(digest)
}

/// Mirror of the app's managed-path rule: relative, no traversal, under a
/// managed top-level directory, naming a file rather than a directory root.
fn validate_referenced_asset_path(path: &str) -> RemoteResult<()> {
    validate_object_path(path)
        .map_err(|_| integrity_error(format!("asset path {path:?} is not a safe relative path")))?;
    let mut segments = path.split('/');
    let top_level = segments.next().unwrap_or_default();
    if !ASSET_TOP_LEVEL_DIRS.contains(&top_level) {
        return Err(integrity_error(format!(
            "asset path {path:?} is outside the managed directories"
        )));
    }
    if segments.next().is_none() {
        return Err(integrity_error(format!(
            "asset path {path:?} refers to a managed root, not a file"
        )));
    }
    Ok(())
}

/// Every managed asset path referenced by the database's song and stem rows,
/// deduplicated and sorted for a deterministic upload order. Optional columns
/// (CDG, artwork derivatives) and the stems table are tolerated as absent so
/// older snapshots still publish.
fn collect_referenced_asset_paths(database: &Path) -> RemoteResult<Vec<String>> {
    let connection = open_readonly(database)?;

    let has_file_path = column_exists(&connection, "songs", "file_path")?;
    let has_cdg_path = column_exists(&connection, "songs", "cdg_path")?;
    let has_artwork_thumb = column_exists(&connection, "songs", "artwork_thumb_path")?;
    let has_artwork_preview = column_exists(&connection, "songs", "artwork_preview_path")?;
    let has_stems = column_exists(&connection, "stems", "song_hash")?;

    let file_col = if has_file_path { "s.file_path" } else { "NULL" };
    let cdg_col = if has_cdg_path { "s.cdg_path" } else { "NULL" };
    let artwork_thumb_col = if has_artwork_thumb {
        "s.artwork_thumb_path"
    } else {
        "NULL"
    };
    let artwork_preview_col = if has_artwork_preview {
        "s.artwork_preview_path"
    } else {
        "NULL"
    };
    let stems_join = if has_stems {
        "LEFT JOIN stems st ON st.song_hash = s.hash"
    } else {
        "LEFT JOIN (SELECT NULL AS song_hash, NULL AS vocals_path, NULL AS accomp_path, \
         NULL AS drums_path, NULL AS bass_path, NULL AS other_path) st ON 0=1"
    };
    let stems_cols = if has_stems {
        "st.vocals_path, st.accomp_path, st.drums_path, st.bass_path, st.other_path"
    } else {
        "NULL, NULL, NULL, NULL, NULL"
    };

    let sql = format!(
        "SELECT {file_col}, {cdg_col}, {artwork_thumb_col}, {artwork_preview_col}, {stems_cols}
         FROM songs s
         {stems_join}
         ORDER BY s.hash ASC"
    );

    let mut statement = connection.prepare(&sql).map_err(|error| {
        integrity_error(format!("failed to prepare referenced-asset query: {error}"))
    })?;
    let rows = statement
        .query_map([], |row| {
            Ok([
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
            ])
        })
        .map_err(|error| integrity_error(format!("referenced-asset query failed: {error}")))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| integrity_error(format!("referenced-asset query failed: {error}")))?;

    let mut paths = BTreeSet::new();
    for row in rows {
        for field in row.into_iter().flatten() {
            if !field.is_empty() {
                paths.insert(field);
            }
        }
    }
    Ok(paths.into_iter().collect())
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> RemoteResult<bool> {
    connection
        .prepare("SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2 LIMIT 1")
        .and_then(|mut statement| statement.exists([table, column]))
        .map_err(|error| integrity_error(format!("schema probe failed: {error}")))
}

fn pre_publish_conflict(expected_generation: i64, current_generation: i64) -> RemoteError {
    RemoteError::new(
        RemoteErrorKind::RemoteConflict,
        format!(
            "expected generation {expected_generation} but the remote repository is at \
             {current_generation}; another device published first"
        ),
    )
}

fn current_unix_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::MANIFEST_PATH;
    use crate::reader::{
        refresh_repository, RefreshOutcome, RefreshPolicy, WorkingCopyDatabase,
        LEGACY_DATABASE_PATH,
    };
    use crate::storage::{ListedObject, ObjectMetadata, StorageCapabilities};
    use crate::testing::{Fault, MemoryStorage};
    use rusqlite::params;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    struct AllowAll;
    impl RefreshPolicy for AllowAll {
        fn may_replace_working_copy(&self) -> bool {
            true
        }
    }

    #[derive(Default)]
    struct SongRow<'a> {
        hash: &'a str,
        file_path: Option<&'a str>,
        artwork_thumb_path: Option<&'a str>,
        stems: Option<[&'a str; 5]>,
    }

    /// A working copy directory with the schema subset the writer reads:
    /// songs, stems and the machine-local publish outbox.
    fn create_working_copy(songs: &[SongRow<'_>], outbox_operations: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("openkara.db");
        {
            let connection = Connection::open(&db_path).expect("create working database");
            connection
                .execute_batch(
                    "CREATE TABLE songs (
                        hash TEXT PRIMARY KEY,
                        title TEXT,
                        file_path TEXT,
                        cdg_path TEXT,
                        artwork_thumb_path TEXT,
                        artwork_preview_path TEXT,
                        audio_source_kind TEXT NOT NULL DEFAULT 'original',
                        imported_at INTEGER NOT NULL DEFAULT 0
                    );
                    CREATE TABLE stems (
                        song_hash TEXT PRIMARY KEY,
                        vocals_path TEXT,
                        accomp_path TEXT,
                        drums_path TEXT,
                        bass_path TEXT,
                        other_path TEXT
                    );
                    CREATE TABLE remote_publish_outbox (
                        operation_id TEXT PRIMARY KEY,
                        song_ids_json TEXT NOT NULL DEFAULT '[]',
                        whole_repository INTEGER NOT NULL DEFAULT 0,
                        expected_generation INTEGER,
                        source_db_digest TEXT,
                        created_at_ms INTEGER NOT NULL DEFAULT 0,
                        projected_at_ms INTEGER
                    );",
                )
                .expect("create schema");
            for song in songs {
                connection
                    .execute(
                        "INSERT INTO songs (hash, title, file_path, artwork_thumb_path) \
                         VALUES (?1, ?2, ?3, ?4)",
                        params![
                            song.hash,
                            format!("Song {}", song.hash),
                            song.file_path,
                            song.artwork_thumb_path
                        ],
                    )
                    .expect("insert song");
                if let Some(stems) = &song.stems {
                    connection
                        .execute(
                            "INSERT INTO stems (song_hash, vocals_path, accomp_path, \
                             drums_path, bass_path, other_path) \
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            params![song.hash, stems[0], stems[1], stems[2], stems[3], stems[4]],
                        )
                        .expect("insert stems");
                }
            }
            for operation_id in outbox_operations {
                connection
                    .execute(
                        "INSERT INTO remote_publish_outbox (operation_id, song_ids_json) \
                         VALUES (?1, '[]')",
                        params![operation_id],
                    )
                    .expect("insert outbox row");
            }
        }
        dir
    }

    fn add_asset(root: &Path, relative_path: &str, bytes: &[u8]) {
        let path = root.join(relative_path);
        fs::create_dir_all(path.parent().expect("asset parent")).expect("asset dirs");
        fs::write(path, bytes).expect("write asset");
    }

    fn content_addressed_path(top_level: &str, bytes: &[u8], extension: &str) -> String {
        format!("{top_level}/content/{}.{extension}", digest_of(bytes))
    }

    fn digest_of(bytes: &[u8]) -> String {
        hex_lower(&Sha256::digest(bytes))
    }

    fn request(expected_generation: i64, operation_id: &str) -> PublishRequest {
        PublishRequest {
            repository_id: "repo-1".to_owned(),
            writer_id: "writer-1".to_owned(),
            operation_id: operation_id.to_owned(),
            expected_generation,
        }
    }

    fn publish(
        storage: &dyn RepositoryStorage,
        working_copy: &tempfile::TempDir,
        request: &PublishRequest,
    ) -> RemoteResult<PublishOutcome> {
        publish_changes(
            storage,
            working_copy.path(),
            &working_copy.path().join("openkara.db"),
            request,
            &mut |_| {},
        )
    }

    fn committed(outcome: PublishOutcome) -> CommittedSnapshot {
        match outcome {
            PublishOutcome::Committed(snapshot) => snapshot,
            other => panic!("expected Committed, got {other:?}"),
        }
    }

    fn read_all(storage: &dyn RepositoryStorage, path: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        storage.read(path, &mut bytes).expect("read object");
        bytes
    }

    fn remote_manifest(storage: &dyn RepositoryStorage) -> RepositoryManifest {
        let bytes = read_all(storage, MANIFEST_PATH);
        RepositoryManifest::from_json(std::str::from_utf8(&bytes).expect("manifest utf8"))
            .expect("manifest parses")
    }

    fn listed_paths(storage: &dyn RepositoryStorage) -> Vec<String> {
        storage
            .list("")
            .expect("list")
            .into_iter()
            .map(|object| object.path)
            .collect()
    }

    /// Refresh a brand-new working copy from the remote, proving the remote
    /// state is a consistent, installable generation.
    fn refresh_into(storage: &dyn RepositoryStorage) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let working_copy = WorkingCopyDatabase::new(dir.path().join("openkara.db"));
        let outcome = refresh_repository(storage, &working_copy, None, &AllowAll).expect("refresh");
        assert!(
            matches!(outcome, RefreshOutcome::Refreshed { .. }),
            "expected a refresh, got {outcome:?}"
        );
        (dir, working_copy.path().to_path_buf())
    }

    fn song_hashes(database: &Path) -> Vec<String> {
        let connection = open_readonly(database).expect("open pulled database");
        let mut statement = connection
            .prepare("SELECT hash FROM songs ORDER BY hash")
            .expect("prepare");
        statement
            .query_map([], |row| row.get(0))
            .expect("query")
            .collect::<rusqlite::Result<Vec<String>>>()
            .expect("collect")
    }

    /// Records every `write_new` path so tests can prove idempotent uploads
    /// do not rewrite objects the remote already has.
    #[derive(Clone)]
    struct WriteCountingStorage {
        inner: MemoryStorage,
        writes: Arc<Mutex<Vec<String>>>,
    }

    impl WriteCountingStorage {
        fn new(inner: MemoryStorage) -> Self {
            Self {
                inner,
                writes: Arc::default(),
            }
        }

        fn written_paths(&self) -> Vec<String> {
            self.writes.lock().expect("writes lock").clone()
        }
    }

    impl RepositoryStorage for WriteCountingStorage {
        fn capabilities(&self) -> StorageCapabilities {
            self.inner.capabilities()
        }
        fn stat(&self, path: &str) -> RemoteResult<Option<ObjectMetadata>> {
            self.inner.stat(path)
        }
        fn read(&self, path: &str, sink: &mut dyn Write) -> RemoteResult<u64> {
            self.inner.read(path, sink)
        }
        fn read_range(
            &self,
            path: &str,
            offset: u64,
            length: u64,
            sink: &mut dyn Write,
        ) -> RemoteResult<u64> {
            self.inner.read_range(path, offset, length, sink)
        }
        fn write_new(&self, path: &str, bytes: &[u8]) -> RemoteResult<ObjectMetadata> {
            self.writes
                .lock()
                .expect("writes lock")
                .push(path.to_owned());
            self.inner.write_new(path, bytes)
        }
        fn conditional_replace(
            &self,
            path: &str,
            bytes: &[u8],
            expected_revision: &str,
        ) -> RemoteResult<ObjectMetadata> {
            self.inner
                .conditional_replace(path, bytes, expected_revision)
        }
        fn list(&self, prefix: &str) -> RemoteResult<Vec<ListedObject>> {
            self.inner.list(prefix)
        }
        fn delete(&self, path: &str) -> RemoteResult<()> {
            self.inner.delete(path)
        }
    }

    /// Another device commits its own manifest right between our probe and
    /// our conditional replace.
    struct ConcurrentCommitStorage {
        inner: MemoryStorage,
        intruder_manifest: Vec<u8>,
        fired: Arc<AtomicBool>,
    }

    impl RepositoryStorage for ConcurrentCommitStorage {
        fn capabilities(&self) -> StorageCapabilities {
            self.inner.capabilities()
        }
        fn stat(&self, path: &str) -> RemoteResult<Option<ObjectMetadata>> {
            self.inner.stat(path)
        }
        fn read(&self, path: &str, sink: &mut dyn Write) -> RemoteResult<u64> {
            self.inner.read(path, sink)
        }
        fn read_range(
            &self,
            path: &str,
            offset: u64,
            length: u64,
            sink: &mut dyn Write,
        ) -> RemoteResult<u64> {
            self.inner.read_range(path, offset, length, sink)
        }
        fn write_new(&self, path: &str, bytes: &[u8]) -> RemoteResult<ObjectMetadata> {
            self.inner.write_new(path, bytes)
        }
        fn conditional_replace(
            &self,
            path: &str,
            bytes: &[u8],
            expected_revision: &str,
        ) -> RemoteResult<ObjectMetadata> {
            if path == MANIFEST_PATH && !self.fired.swap(true, Ordering::SeqCst) {
                let current = self
                    .inner
                    .stat(MANIFEST_PATH)
                    .expect("stat")
                    .and_then(|metadata| metadata.revision)
                    .expect("manifest revision");
                self.inner
                    .conditional_replace(MANIFEST_PATH, &self.intruder_manifest, &current)
                    .expect("the intruder commits against the current revision");
            }
            self.inner
                .conditional_replace(path, bytes, expected_revision)
        }
        fn list(&self, prefix: &str) -> RemoteResult<Vec<ListedObject>> {
            self.inner.list(prefix)
        }
        fn delete(&self, path: &str) -> RemoteResult<()> {
            self.inner.delete(path)
        }
    }

    /// Corrupts every uploaded `.sqlite` object while reporting success, the
    /// way a broken proxy could.
    struct CorruptingStorage {
        inner: MemoryStorage,
    }

    impl RepositoryStorage for CorruptingStorage {
        fn capabilities(&self) -> StorageCapabilities {
            self.inner.capabilities()
        }
        fn stat(&self, path: &str) -> RemoteResult<Option<ObjectMetadata>> {
            self.inner.stat(path)
        }
        fn read(&self, path: &str, sink: &mut dyn Write) -> RemoteResult<u64> {
            self.inner.read(path, sink)
        }
        fn read_range(
            &self,
            path: &str,
            offset: u64,
            length: u64,
            sink: &mut dyn Write,
        ) -> RemoteResult<u64> {
            self.inner.read_range(path, offset, length, sink)
        }
        fn write_new(&self, path: &str, bytes: &[u8]) -> RemoteResult<ObjectMetadata> {
            let mut bytes = bytes.to_vec();
            if path.ends_with(".sqlite") && !bytes.is_empty() {
                bytes[0] ^= 0xFF;
            }
            self.inner.write_new(path, &bytes)
        }
        fn conditional_replace(
            &self,
            path: &str,
            bytes: &[u8],
            expected_revision: &str,
        ) -> RemoteResult<ObjectMetadata> {
            self.inner
                .conditional_replace(path, bytes, expected_revision)
        }
        fn list(&self, prefix: &str) -> RemoteResult<Vec<ListedObject>> {
            self.inner.list(prefix)
        }
        fn delete(&self, path: &str) -> RemoteResult<()> {
            self.inner.delete(path)
        }
    }

    fn intruder_manifest_json(generation: i64, operation_id: &str) -> Vec<u8> {
        RepositoryManifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            repository_id: "repo-1".to_owned(),
            generation,
            database_path: database_path_for_operation(generation, operation_id),
            database_size_bytes: 16,
            database_sha256: "00".repeat(32),
            committed_at_ms: 1_700_000_000_000,
            writer_id: "other-device".to_owned(),
            operation_id: operation_id.to_owned(),
        }
        .to_json()
        .expect("intruder manifest")
        .into_bytes()
    }

    #[test]
    fn publish_commits_the_first_generation_into_an_empty_repository() {
        let storage = MemoryStorage::new();
        let asset = b"audio bytes one".to_vec();
        let asset_path = content_addressed_path("media", &asset, "flac");
        let stems_bytes: Vec<Vec<u8>> = (0..5).map(|i| vec![i as u8; 32 + i]).collect();
        let stems_paths: Vec<String> = stems_bytes
            .iter()
            .map(|bytes| content_addressed_path("stems", bytes, "ogg"))
            .collect();
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                file_path: Some(&asset_path),
                stems: Some([
                    &stems_paths[0],
                    &stems_paths[1],
                    &stems_paths[2],
                    &stems_paths[3],
                    &stems_paths[4],
                ]),
                ..Default::default()
            }],
            &["op-1"],
        );
        add_asset(dir.path(), &asset_path, &asset);
        for (path, bytes) in stems_paths.iter().zip(&stems_bytes) {
            add_asset(dir.path(), path, bytes);
        }

        let snapshot = committed(publish(&storage, &dir, &request(0, "op-1")).expect("publish"));

        assert_eq!(snapshot.generation, 1);
        assert!(snapshot.manifest_revision.is_some());
        let manifest = remote_manifest(&storage);
        assert_eq!(manifest.generation, 1);
        assert_eq!(manifest.operation_id, "op-1");
        assert_eq!(manifest.writer_id, "writer-1");
        assert_eq!(manifest.repository_id, "repo-1");
        assert_eq!(
            manifest.database_path,
            database_path_for_operation(1, "op-1")
        );

        // Every referenced asset landed content-addressed.
        assert_eq!(read_all(&storage, &asset_path), asset);
        for (path, bytes) in stems_paths.iter().zip(&stems_bytes) {
            assert_eq!(read_all(&storage, path), *bytes);
        }

        // The snapshot is byte-identical to what a reader installs, carries
        // the song, and is stripped of the machine-local outbox.
        let snapshot_bytes = read_all(&storage, &manifest.database_path);
        assert_eq!(digest_of(&snapshot_bytes), manifest.database_sha256);
        assert_eq!(snapshot_bytes.len() as u64, manifest.database_size_bytes);
        let (_pulled_dir, pulled_db) = refresh_into(&storage);
        assert_eq!(fs::read(&pulled_db).expect("pulled"), snapshot_bytes);
        assert_eq!(song_hashes(&pulled_db), ["song-1"]);
        let connection = open_readonly(&pulled_db).expect("open pulled");
        let outbox_rows: i64 = connection
            .query_row("SELECT COUNT(*) FROM remote_publish_outbox", [], |row| {
                row.get(0)
            })
            .expect("outbox count");
        assert_eq!(outbox_rows, 0, "the machine-local outbox must not ship");

        // The working copy keeps its outbox row; only the candidate was stripped.
        let working = Connection::open(dir.path().join("openkara.db")).expect("open working");
        let working_outbox_rows: i64 = working
            .query_row("SELECT COUNT(*) FROM remote_publish_outbox", [], |row| {
                row.get(0)
            })
            .expect("outbox count");
        assert_eq!(working_outbox_rows, 1);
    }

    #[test]
    fn publishing_twice_advances_the_generation() {
        let storage = MemoryStorage::new();
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                ..Default::default()
            }],
            &[],
        );
        committed(publish(&storage, &dir, &request(0, "op-1")).expect("first publish"));

        let snapshot = committed(publish(&storage, &dir, &request(1, "op-2")).expect("second"));

        assert_eq!(snapshot.generation, 2);
        let manifest = remote_manifest(&storage);
        assert_eq!(manifest.generation, 2);
        assert_eq!(
            manifest.database_path,
            database_path_for_operation(2, "op-2")
        );
        // Both generations stay on the remote; cleanup is a separate concern.
        assert!(storage
            .stat(&database_path_for_operation(1, "op-1"))
            .expect("stat")
            .is_some());
        let (_pulled_dir, pulled_db) = refresh_into(&storage);
        assert_eq!(song_hashes(&pulled_db), ["song-1"]);
    }

    #[test]
    fn assets_the_remote_lacks_are_uploaded_and_present_ones_are_skipped() {
        let storage = MemoryStorage::new();
        let asset = b"shared audio".to_vec();
        let asset_path = content_addressed_path("media", &asset, "flac");
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                file_path: Some(&asset_path),
                ..Default::default()
            }],
            &[],
        );
        add_asset(dir.path(), &asset_path, &asset);
        committed(publish(&storage, &dir, &request(0, "op-1")).expect("first publish"));

        let counting = WriteCountingStorage::new(storage);
        committed(publish(&counting, &dir, &request(1, "op-2")).expect("second publish"));

        assert_eq!(
            counting.written_paths(),
            [database_path_for_operation(2, "op-2")],
            "only the new snapshot object is written; assets are not re-uploaded"
        );
    }

    #[test]
    fn a_referenced_asset_missing_everywhere_fails_the_publish() {
        let storage = MemoryStorage::new();
        let missing_path = format!("media/content/{}.flac", "ab".repeat(32));
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                file_path: Some(&missing_path),
                ..Default::default()
            }],
            &[],
        );

        let error = publish(&storage, &dir, &request(0, "op-1")).expect_err("must fail");

        assert_eq!(error.kind, RemoteErrorKind::RemoteIntegrityFailed);
        assert!(
            listed_paths(&storage).is_empty(),
            "nothing was uploaded or committed"
        );
    }

    #[test]
    fn a_referenced_asset_that_only_exists_remotely_is_accepted() {
        let storage = MemoryStorage::new();
        let asset = b"already published".to_vec();
        let asset_path = content_addressed_path("media", &asset, "flac");
        storage
            .write_new(&asset_path, &asset)
            .expect("seed remote asset");
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                file_path: Some(&asset_path),
                ..Default::default()
            }],
            &[],
        );

        committed(publish(&storage, &dir, &request(0, "op-1")).expect("publish"));
    }

    #[test]
    fn a_torn_remote_asset_is_deleted_and_rewritten() {
        let storage = MemoryStorage::new();
        let asset = b"the full asset bytes".to_vec();
        let asset_path = content_addressed_path("media", &asset, "flac");
        storage
            .write_new(&asset_path, &asset[..7])
            .expect("seed torn leftover");
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                file_path: Some(&asset_path),
                ..Default::default()
            }],
            &[],
        );
        add_asset(dir.path(), &asset_path, &asset);

        committed(publish(&storage, &dir, &request(0, "op-1")).expect("publish rewrites"));

        assert_eq!(read_all(&storage, &asset_path), asset);
    }

    #[test]
    fn a_size_mismatch_on_a_mutable_path_fails_closed() {
        let storage = MemoryStorage::new();
        let local_bytes = b"local version of the song".to_vec();
        storage
            .write_new("media/song-1.flac", b"different remote bytes")
            .expect("seed foreign object");
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                file_path: Some("media/song-1.flac"),
                ..Default::default()
            }],
            &[],
        );
        add_asset(dir.path(), "media/song-1.flac", &local_bytes);

        let error = publish(&storage, &dir, &request(0, "op-1")).expect_err("must fail closed");

        assert_eq!(error.kind, RemoteErrorKind::RemoteIntegrityFailed);
        assert_eq!(
            read_all(&storage, "media/song-1.flac"),
            b"different remote bytes",
            "a foreign object under a mutable name is never overwritten"
        );
        assert_eq!(storage.stat(MANIFEST_PATH).expect("stat"), None);
    }

    #[test]
    fn a_stale_expected_generation_is_a_pre_publish_conflict() {
        let storage = MemoryStorage::new();
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                ..Default::default()
            }],
            &[],
        );
        committed(publish(&storage, &dir, &request(0, "op-1")).expect("first publish"));
        let objects_before = listed_paths(&storage);

        let error = publish(&storage, &dir, &request(0, "op-2")).expect_err("stale base");

        assert_eq!(error.kind, RemoteErrorKind::RemoteConflict);
        assert_eq!(error.code, "remote_conflict");
        assert!(!error.retryable, "a conflict is not retried blindly");
        let manifest = remote_manifest(&storage);
        assert_eq!(manifest.generation, 1);
        assert_eq!(manifest.operation_id, "op-1");
        assert_eq!(
            listed_paths(&storage),
            objects_before,
            "a conflicting publish uploads nothing"
        );
    }

    #[test]
    fn a_concurrent_publishers_generation_conflicts() {
        let device_a = MemoryStorage::new();
        let device_b = device_a.clone();
        let dir_a = create_working_copy(
            &[SongRow {
                hash: "song-a",
                ..Default::default()
            }],
            &[],
        );
        let dir_b = create_working_copy(
            &[SongRow {
                hash: "song-b",
                ..Default::default()
            }],
            &[],
        );
        committed(publish(&device_a, &dir_a, &request(0, "op-a1")).expect("gen 1"));
        // Device B refreshed to gen 1 and published first.
        committed(publish(&device_b, &dir_b, &request(1, "op-b")).expect("gen 2"));

        let error =
            publish(&device_a, &dir_a, &request(1, "op-a2")).expect_err("device A is stale now");

        assert_eq!(error.kind, RemoteErrorKind::RemoteConflict);
        let manifest = remote_manifest(&device_a);
        assert_eq!(manifest.generation, 2);
        assert_eq!(manifest.operation_id, "op-b");
        let (_pulled_dir, pulled_db) = refresh_into(&device_a);
        assert_eq!(song_hashes(&pulled_db), ["song-b"]);
    }

    #[test]
    fn a_revision_moving_between_probe_and_commit_is_a_conflict() {
        let storage = MemoryStorage::new();
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                ..Default::default()
            }],
            &[],
        );
        committed(publish(&storage, &dir, &request(0, "op-1")).expect("gen 1"));
        let racing = ConcurrentCommitStorage {
            inner: storage.clone(),
            intruder_manifest: intruder_manifest_json(2, "op-intruder"),
            fired: Arc::default(),
        };

        let error = publish(&racing, &dir, &request(1, "op-2")).expect_err("lost the CAS race");

        assert_eq!(error.kind, RemoteErrorKind::RemoteConflict);
        let manifest = remote_manifest(&storage);
        assert_eq!(manifest.generation, 2);
        assert_eq!(manifest.operation_id, "op-intruder");
        // Our unreferenced snapshot remains; the remote is never torn.
        assert!(storage
            .stat(&database_path_for_operation(2, "op-2"))
            .expect("stat")
            .is_some());
    }

    #[test]
    fn a_revision_moving_before_the_probe_is_a_conflict() {
        let storage = MemoryStorage::new();
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                ..Default::default()
            }],
            &[],
        );
        committed(publish(&storage, &dir, &request(0, "op-1")).expect("gen 1"));
        storage.script(Fault::MoveRevision {
            path: MANIFEST_PATH.to_owned(),
            bytes: intruder_manifest_json(7, "op-intruder"),
        });

        let error = publish(&storage, &dir, &request(1, "op-2")).expect_err("moved revision");

        assert_eq!(error.kind, RemoteErrorKind::RemoteConflict);
        assert_eq!(remote_manifest(&storage).generation, 7);
    }

    #[test]
    fn an_interrupted_snapshot_upload_leaves_no_torn_state_and_recovers_on_republish() {
        let storage = MemoryStorage::new();
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                ..Default::default()
            }],
            &[],
        );
        committed(publish(&storage, &dir, &request(0, "op-1")).expect("gen 1"));
        let generation_one_snapshot = read_all(&storage, &database_path_for_operation(1, "op-1"));
        {
            let connection = Connection::open(dir.path().join("openkara.db")).expect("open");
            connection
                .execute(
                    "INSERT INTO songs (hash, title) VALUES ('song-2', 'Song song-2')",
                    [],
                )
                .expect("mutate working copy");
        }
        let snapshot_path = database_path_for_operation(2, "op-2");
        storage.script(Fault::PartialWrite {
            path: snapshot_path.clone(),
            stored_bytes: 100,
        });

        let error = publish(&storage, &dir, &request(1, "op-2")).expect_err("interrupted");
        assert_eq!(error.kind, RemoteErrorKind::NetworkUnavailable);
        assert!(error.retryable);

        // No torn state: the manifest still names generation 1 and the
        // generation-1 snapshot installs byte for byte.
        assert_eq!(remote_manifest(&storage).generation, 1);
        assert_eq!(
            read_all(&storage, &database_path_for_operation(1, "op-1")),
            generation_one_snapshot
        );
        let (_pulled_dir, pulled_db) = refresh_into(&storage);
        assert_eq!(
            fs::read(&pulled_db).expect("pulled"),
            generation_one_snapshot
        );
        let torn = storage
            .stat(&snapshot_path)
            .expect("stat")
            .expect("torn leftover");
        assert_eq!(torn.size_bytes, 100);

        let snapshot = committed(publish(&storage, &dir, &request(1, "op-2")).expect("republish"));
        assert_eq!(snapshot.generation, 2);
        let manifest = remote_manifest(&storage);
        let snapshot_bytes = read_all(&storage, &snapshot_path);
        assert_eq!(digest_of(&snapshot_bytes), manifest.database_sha256);
        let (_pulled_dir, pulled_db) = refresh_into(&storage);
        assert_eq!(song_hashes(&pulled_db), ["song-1", "song-2"]);
    }

    #[test]
    fn an_interrupted_asset_upload_recovers_on_republish() {
        let storage = MemoryStorage::new();
        let asset = b"asset bytes that get cut off".to_vec();
        let asset_path = content_addressed_path("media", &asset, "flac");
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                file_path: Some(&asset_path),
                ..Default::default()
            }],
            &[],
        );
        add_asset(dir.path(), &asset_path, &asset);
        storage.script(Fault::PartialWrite {
            path: asset_path.clone(),
            stored_bytes: 5,
        });

        let error = publish(&storage, &dir, &request(0, "op-1")).expect_err("interrupted");
        assert_eq!(error.kind, RemoteErrorKind::NetworkUnavailable);
        assert_eq!(
            storage.stat(MANIFEST_PATH).expect("stat"),
            None,
            "an interrupted asset upload never reaches the manifest"
        );
        assert_eq!(
            storage
                .stat(&asset_path)
                .expect("stat")
                .expect("torn asset")
                .size_bytes,
            5
        );

        committed(publish(&storage, &dir, &request(0, "op-1")).expect("republish"));
        assert_eq!(read_all(&storage, &asset_path), asset);
        assert_eq!(remote_manifest(&storage).generation, 1);
    }

    #[test]
    fn an_upload_that_corrupts_bytes_fails_integrity_before_the_manifest_moves() {
        let storage = MemoryStorage::new();
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                ..Default::default()
            }],
            &[],
        );
        committed(publish(&storage, &dir, &request(0, "op-1")).expect("gen 1"));
        let corrupting = CorruptingStorage {
            inner: storage.clone(),
        };

        let error = publish(&corrupting, &dir, &request(1, "op-2"))
            .expect_err("corrupted upload must be detected");

        assert_eq!(error.kind, RemoteErrorKind::RemoteIntegrityFailed);
        assert_eq!(
            remote_manifest(&storage).generation,
            1,
            "the manifest never moves past a corrupt snapshot"
        );
    }

    #[test]
    fn a_commit_lost_between_cas_and_completion_is_recognized_on_retry() {
        let storage = MemoryStorage::new();
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                ..Default::default()
            }],
            &[],
        );
        let first = committed(publish(&storage, &dir, &request(0, "op-1")).expect("publish"));
        let revision_after_commit = storage
            .stat(MANIFEST_PATH)
            .expect("stat")
            .and_then(|metadata| metadata.revision);

        let outcome = publish(&storage, &dir, &request(0, "op-1")).expect("retry");

        let PublishOutcome::AlreadyCommitted(snapshot) = outcome else {
            panic!("expected AlreadyCommitted, got {outcome:?}");
        };
        assert_eq!(snapshot.generation, 1);
        assert_eq!(snapshot.database_sha256, first.database_sha256);
        assert_eq!(
            storage
                .stat(MANIFEST_PATH)
                .expect("stat")
                .and_then(|metadata| metadata.revision),
            revision_after_commit,
            "a recognized commit does not replace the manifest again"
        );
    }

    #[test]
    fn a_recognized_commit_still_conflicts_when_the_working_copy_moved_on() {
        let storage = MemoryStorage::new();
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                ..Default::default()
            }],
            &[],
        );
        committed(publish(&storage, &dir, &request(0, "op-1")).expect("publish"));
        {
            let connection = Connection::open(dir.path().join("openkara.db")).expect("open");
            connection
                .execute(
                    "INSERT INTO songs (hash, title) VALUES ('song-2', 'Song song-2')",
                    [],
                )
                .expect("mutate working copy");
        }

        let error = publish(&storage, &dir, &request(0, "op-1")).expect_err("moved on");

        assert_eq!(error.kind, RemoteErrorKind::RemoteConflict);
        assert_eq!(remote_manifest(&storage).generation, 1);
    }

    #[test]
    fn a_storage_without_conditional_replace_cannot_publish() {
        let storage = MemoryStorage::with_capabilities(StorageCapabilities {
            conditional_replace: false,
            range_read: true,
        });
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                ..Default::default()
            }],
            &[],
        );

        let error = publish(&storage, &dir, &request(0, "op-1")).expect_err("cannot publish");

        assert_eq!(error.kind, RemoteErrorKind::ProviderCapabilityUnavailable);
        assert!(listed_paths(&storage).is_empty());
    }

    #[test]
    fn publish_into_a_legacy_repository_creates_the_first_manifest() {
        let storage = MemoryStorage::new();
        storage
            .write_new(LEGACY_DATABASE_PATH, b"legacy database bytes")
            .expect("seed legacy database");
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                ..Default::default()
            }],
            &[],
        );

        let snapshot = committed(publish(&storage, &dir, &request(0, "op-1")).expect("publish"));

        assert_eq!(snapshot.generation, 1);
        assert_eq!(
            read_all(&storage, LEGACY_DATABASE_PATH),
            b"legacy database bytes",
            "the legacy object is left untouched for old readers"
        );
        let (_pulled_dir, pulled_db) = refresh_into(&storage);
        assert_eq!(song_hashes(&pulled_db), ["song-1"]);
    }

    #[test]
    fn an_interrupted_verification_read_recovers_without_reuploading() {
        let storage = MemoryStorage::new();
        let dir = create_working_copy(
            &[SongRow {
                hash: "song-1",
                ..Default::default()
            }],
            &[],
        );
        committed(publish(&storage, &dir, &request(0, "op-1")).expect("gen 1"));
        {
            let connection = Connection::open(dir.path().join("openkara.db")).expect("open");
            connection
                .execute(
                    "INSERT INTO songs (hash, title) VALUES ('song-2', 'Song song-2')",
                    [],
                )
                .expect("mutate working copy");
        }
        let snapshot_path = database_path_for_operation(2, "op-2");
        // The upload lands but the verifying re-read is cut off.
        storage.script(Fault::InterruptTransfer {
            path: snapshot_path.clone(),
            after_bytes: 64,
        });

        let error = publish(&storage, &dir, &request(1, "op-2")).expect_err("interrupted");
        assert_eq!(error.kind, RemoteErrorKind::NetworkUnavailable);
        assert_eq!(remote_manifest(&storage).generation, 1);

        let counting = WriteCountingStorage::new(storage.clone());
        let snapshot = committed(publish(&counting, &dir, &request(1, "op-2")).expect("republish"));
        assert_eq!(snapshot.generation, 2);
        assert!(
            counting.written_paths().is_empty(),
            "the completed upload is verified and skipped, not re-uploaded: {:?}",
            counting.written_paths()
        );
    }

    #[test]
    fn a_database_without_optional_columns_publishes() {
        let storage = MemoryStorage::new();
        let dir = tempfile::tempdir().expect("temp dir");
        {
            let connection =
                Connection::open(dir.path().join("openkara.db")).expect("create database");
            connection
                .execute_batch(
                    "CREATE TABLE songs (\
                        hash TEXT PRIMARY KEY, \
                        title TEXT, \
                        imported_at INTEGER NOT NULL DEFAULT 0\
                    );",
                )
                .expect("create minimal schema");
            connection
                .execute(
                    "INSERT INTO songs (hash, title) VALUES ('song-1', 'Song song-1')",
                    [],
                )
                .expect("insert song");
        }

        committed(publish(&storage, &dir, &request(0, "op-1")).expect("publish"));

        let (_pulled_dir, pulled_db) = refresh_into(&storage);
        assert_eq!(song_hashes(&pulled_db), ["song-1"]);
    }

    mod webdav {
        use super::*;
        use crate::testing::TestWebDavServer;

        #[test]
        fn publish_commits_the_first_generation_over_webdav() {
            let server = TestWebDavServer::start();
            let storage = server.storage().clone();
            let asset = b"webdav audio bytes".to_vec();
            let asset_path = content_addressed_path("media", &asset, "flac");
            let dir = create_working_copy(
                &[SongRow {
                    hash: "song-1",
                    file_path: Some(&asset_path),
                    ..Default::default()
                }],
                &["op-1"],
            );
            add_asset(dir.path(), &asset_path, &asset);

            let snapshot =
                committed(publish(&storage, &dir, &request(0, "op-1")).expect("publish"));

            assert_eq!(snapshot.generation, 1);
            assert!(snapshot.manifest_revision.is_some());
            assert_eq!(server.file(&asset_path), Some(asset));
            let manifest_bytes = server.file(MANIFEST_PATH).expect("manifest on server");
            let manifest =
                RepositoryManifest::from_json(std::str::from_utf8(&manifest_bytes).expect("utf8"))
                    .expect("manifest parses");
            assert_eq!(manifest.operation_id, "op-1");
            let snapshot_bytes = server.file(&manifest.database_path).expect("snapshot");
            assert_eq!(digest_of(&snapshot_bytes), manifest.database_sha256);

            let (_pulled_dir, pulled_db) = refresh_into(&storage);
            assert_eq!(fs::read(&pulled_db).expect("pulled"), snapshot_bytes);
            assert_eq!(song_hashes(&pulled_db), ["song-1"]);
        }

        #[test]
        fn an_interrupted_snapshot_upload_over_webdav_recovers_on_republish() {
            let server = TestWebDavServer::start();
            let storage = server.storage().clone();
            let dir = create_working_copy(
                &[SongRow {
                    hash: "song-1",
                    ..Default::default()
                }],
                &[],
            );
            committed(publish(&storage, &dir, &request(0, "op-1")).expect("gen 1"));
            {
                let connection = Connection::open(dir.path().join("openkara.db")).expect("open");
                connection
                    .execute(
                        "INSERT INTO songs (hash, title) VALUES ('song-2', 'Song song-2')",
                        [],
                    )
                    .expect("mutate working copy");
            }
            let snapshot_path = database_path_for_operation(2, "op-2");
            server.script(Fault::PartialWrite {
                path: snapshot_path.clone(),
                stored_bytes: 100,
            });

            let error = publish(&storage, &dir, &request(1, "op-2")).expect_err("interrupted");
            assert!(
                error.retryable,
                "an interrupted upload is retryable: {error}"
            );

            // The manifest still names generation 1 and it installs cleanly.
            assert_eq!(remote_manifest(&storage).generation, 1);
            let (_pulled_dir, pulled_db) = refresh_into(&storage);
            assert_eq!(song_hashes(&pulled_db), ["song-1"]);

            let snapshot =
                committed(publish(&storage, &dir, &request(1, "op-2")).expect("republish"));
            assert_eq!(snapshot.generation, 2);
            let manifest = remote_manifest(&storage);
            let snapshot_bytes = server.file(&snapshot_path).expect("snapshot on server");
            assert_eq!(digest_of(&snapshot_bytes), manifest.database_sha256);
            let (_pulled_dir, pulled_db) = refresh_into(&storage);
            assert_eq!(song_hashes(&pulled_db), ["song-1", "song-2"]);
        }

        #[test]
        fn a_revision_moving_between_steps_over_webdav_is_a_conflict() {
            let server = TestWebDavServer::start();
            let storage = server.storage().clone();
            let dir = create_working_copy(
                &[SongRow {
                    hash: "song-1",
                    ..Default::default()
                }],
                &[],
            );
            committed(publish(&storage, &dir, &request(0, "op-1")).expect("gen 1"));
            server.script(Fault::MoveRevision {
                path: MANIFEST_PATH.to_owned(),
                bytes: intruder_manifest_json(9, "op-intruder"),
            });

            let error = publish(&storage, &dir, &request(1, "op-2")).expect_err("conflict");

            assert_eq!(error.kind, RemoteErrorKind::RemoteConflict);
            assert_eq!(remote_manifest(&storage).generation, 9);
        }
    }
}
