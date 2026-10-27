//! Refresh Repository reader: install the published database snapshot into the
//! Local Working Copy.
//!
//! The manifest is the only visibility switch. [`plan_refresh`] reads and
//! validates it; [`pull_database_snapshot`] downloads the snapshot it names
//! with range resume into a sibling `.part.<operation-id>` file, verifies
//! size, SHA-256 and SQLite integrity, then swaps it in atomically while
//! preserving the previous verified database as `<name>.lkg`. A failed or
//! interrupted pull leaves the working copy untouched; the received prefix
//! stays in the part file so the next attempt resumes where it stopped.
//!
//! The reader never touches application state. Whether the Local Working Copy
//! may be replaced (the desktop Clean/Dirty rule) arrives through the
//! [`RefreshPolicy`] hook.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::error::{kind_from_io_error, RemoteError, RemoteErrorKind, RemoteResult};
use crate::manifest::{RepositoryManifest, MANIFEST_PATH};
use crate::storage::RepositoryStorage;

/// Chunk size for resumable snapshot downloads, matching the desktop reader.
pub const SNAPSHOT_DOWNLOAD_CHUNK_SIZE: u64 = 8 * 1024 * 1024;

/// Database object of a repository that predates the manifest.
pub const LEGACY_DATABASE_PATH: &str = "openkara.db";

/// The application decides whether the Local Working Copy may be replaced;
/// the core does not own control-database state.
pub trait RefreshPolicy {
    fn may_replace_working_copy(&self) -> bool;
}

/// The database file of one Local Working Copy and its temp-file layout.
pub struct WorkingCopyDatabase {
    path: PathBuf,
}

impl WorkingCopyDatabase {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Sibling temp file a snapshot downloads into before activation.
    pub fn part_path(&self, operation_id: &str) -> PathBuf {
        let file_name = self
            .path
            .file_name()
            .map(|name| format!("{}.part.{}", name.to_string_lossy(), operation_id))
            .unwrap_or_else(|| format!("openkara.part.{operation_id}"));
        self.path.with_file_name(file_name)
    }

    fn last_known_good_path(&self) -> PathBuf {
        let file_name = self
            .path
            .file_name()
            .map(|name| format!("{}.lkg", name.to_string_lossy()))
            .unwrap_or_else(|| "openkara.db.lkg".to_owned());
        self.path.with_file_name(file_name)
    }
}

/// The remote object a refresh downloads, with the expectations the download
/// is verified against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotSource {
    pub remote_path: String,
    /// Manifest generation; 0 for a legacy repository without a manifest.
    pub generation: i64,
    pub expected_size: Option<u64>,
    pub expected_digest: Option<String>,
}

/// The result of inspecting the remote for a refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshPlan {
    /// Revision of the manifest, or of `openkara.db` for legacy repositories.
    pub remote_revision: Option<String>,
    /// `None` for legacy repositories without a manifest.
    pub manifest: Option<RepositoryManifest>,
    pub database: SnapshotSource,
}

/// A snapshot that was downloaded, verified and swapped in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshedSnapshot {
    pub generation: i64,
    pub remote_path: String,
    pub database_size: u64,
    pub database_sha256: String,
}

/// The result of a composed [`refresh_repository`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// The policy forbids replacing the Local Working Copy.
    SkippedByPolicy,
    /// The stored revision already matches the remote; nothing was downloaded.
    Unchanged { remote_revision: Option<String> },
    Refreshed {
        snapshot: RefreshedSnapshot,
        /// Manifest revision after the pull (pre-pull revision for legacy
        /// repositories), for the caller to store as the new baseline.
        remote_revision: Option<String>,
    },
}

/// The operation id used for a snapshot pull's part file, sanitized so an
/// ETag with quotes or slashes stays a safe file name.
pub fn snapshot_operation_id(remote_revision: Option<&str>) -> String {
    let sanitized: String = remote_revision
        .unwrap_or("unknown")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("pull-{sanitized}")
}

/// The remote revision a refresh compares against the stored one: the
/// manifest revision, or the `openkara.db` revision for legacy repositories.
pub fn probe_remote_revision(storage: &dyn RepositoryStorage) -> RemoteResult<Option<String>> {
    let manifest_revision = storage
        .stat(MANIFEST_PATH)?
        .and_then(|metadata| metadata.revision);
    if manifest_revision.is_some() {
        return Ok(manifest_revision);
    }
    Ok(storage
        .stat(LEGACY_DATABASE_PATH)?
        .and_then(|metadata| metadata.revision))
}

/// The remote revision is stale when it moved away from the stored one.
pub fn remote_revision_is_stale(
    stored_revision: Option<&str>,
    remote_revision: Option<&str>,
) -> bool {
    remote_revision.is_some_and(|revision| Some(revision) != stored_revision)
}

/// Inspect the remote and decide what a refresh would download.
///
/// Reads and validates the manifest when one exists and falls back to the
/// legacy `openkara.db` otherwise. A manifest that is absent or empty means
/// the repository predates the manifest format.
pub fn plan_refresh(storage: &dyn RepositoryStorage) -> RemoteResult<RefreshPlan> {
    let manifest_metadata = storage.stat(MANIFEST_PATH)?;
    let manifest_revision = manifest_metadata
        .as_ref()
        .and_then(|metadata| metadata.revision.clone());
    let manifest = match &manifest_metadata {
        Some(metadata) if metadata.size_bytes > 0 => {
            let mut bytes = Vec::new();
            storage.read(MANIFEST_PATH, &mut bytes)?;
            if bytes.is_empty() {
                None
            } else {
                let json = std::str::from_utf8(&bytes).map_err(|error| {
                    RemoteError::new(
                        RemoteErrorKind::RemoteIntegrityFailed,
                        format!("repository manifest is not valid UTF-8: {error}"),
                    )
                })?;
                Some(RepositoryManifest::from_json(json)?)
            }
        }
        _ => None,
    };

    match manifest {
        Some(manifest) => {
            // A HEAD without Content-Length reports 0; the manifest size is
            // the authoritative fallback, like the desktop's get_file_size.
            let database_size = storage
                .stat(&manifest.database_path)?
                .map(|metadata| metadata.size_bytes)
                .filter(|&size| size > 0)
                .or(Some(manifest.database_size_bytes));
            let remote_revision = match manifest_revision {
                Some(revision) => Some(revision),
                None => storage
                    .stat(LEGACY_DATABASE_PATH)?
                    .and_then(|metadata| metadata.revision),
            };
            let database = SnapshotSource {
                remote_path: manifest.database_path.clone(),
                generation: manifest.generation,
                expected_size: database_size,
                expected_digest: Some(manifest.database_sha256.clone()),
            };
            Ok(RefreshPlan {
                remote_revision,
                manifest: Some(manifest),
                database,
            })
        }
        None => {
            let legacy_metadata = storage.stat(LEGACY_DATABASE_PATH)?;
            let remote_revision = manifest_revision.or_else(|| {
                legacy_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.revision.clone())
            });
            Ok(RefreshPlan {
                remote_revision,
                manifest: None,
                database: SnapshotSource {
                    remote_path: LEGACY_DATABASE_PATH.to_owned(),
                    generation: 0,
                    expected_size: legacy_metadata
                        .map(|metadata| metadata.size_bytes)
                        .filter(|&size| size > 0),
                    expected_digest: None,
                },
            })
        }
    }
}

/// Download the planned snapshot into the working copy and swap it in.
///
/// Returns `Ok(None)` when the policy forbids replacing the working copy. On
/// a transfer error the received prefix stays in the part file and the next
/// call resumes from it; on a verification error the part file is deleted
/// because corrupt bytes can never resume into a valid snapshot. The working
/// copy database is only ever replaced by a fully verified candidate.
///
/// `on_progress` receives the durable byte count after every chunk and once
/// more before a transfer error is returned, so the caller can mirror
/// resumable-transfer bookkeeping.
pub fn pull_database_snapshot(
    storage: &dyn RepositoryStorage,
    plan: &RefreshPlan,
    working_copy: &WorkingCopyDatabase,
    operation_id: &str,
    policy: &dyn RefreshPolicy,
    on_progress: &mut dyn FnMut(u64),
) -> RemoteResult<Option<RefreshedSnapshot>> {
    if !policy.may_replace_working_copy() {
        return Ok(None);
    }
    let source = &plan.database;
    let part_path = working_copy.part_path(operation_id);
    download_snapshot(storage, source, &part_path, on_progress)?;
    let verified = match verify_snapshot(&part_path, source) {
        Ok(verified) => verified,
        Err(error) => {
            let _ = fs::remove_file(&part_path);
            return Err(error);
        }
    };
    activate_candidate(&part_path, working_copy)?;
    Ok(Some(RefreshedSnapshot {
        generation: source.generation,
        remote_path: source.remote_path.clone(),
        database_size: verified.size,
        database_sha256: verified.digest,
    }))
}

/// Refresh a Local Working Copy from the remote repository.
///
/// Composes [`plan_refresh`], the staleness check and
/// [`pull_database_snapshot`]. Callers that apply different error policies to
/// inspection and download failures (the desktop falls back to the local copy
/// when a pull fails) use the two phases directly.
pub fn refresh_repository(
    storage: &dyn RepositoryStorage,
    working_copy: &WorkingCopyDatabase,
    stored_revision: Option<&str>,
    policy: &dyn RefreshPolicy,
) -> RemoteResult<RefreshOutcome> {
    let remote_revision = probe_remote_revision(storage)?;
    if !remote_revision_is_stale(stored_revision, remote_revision.as_deref()) {
        return Ok(RefreshOutcome::Unchanged { remote_revision });
    }
    let plan = plan_refresh(storage)?;
    let operation_id = snapshot_operation_id(plan.remote_revision.as_deref());
    let mut ignore_progress = |_| {};
    let Some(snapshot) = pull_database_snapshot(
        storage,
        &plan,
        working_copy,
        &operation_id,
        policy,
        &mut ignore_progress,
    )?
    else {
        return Ok(RefreshOutcome::SkippedByPolicy);
    };
    let remote_revision = match &plan.manifest {
        Some(_) => storage
            .stat(MANIFEST_PATH)?
            .and_then(|metadata| metadata.revision),
        None => plan.remote_revision.clone(),
    };
    Ok(RefreshOutcome::Refreshed {
        snapshot,
        remote_revision,
    })
}

struct VerifiedCandidate {
    size: u64,
    digest: String,
}

fn download_snapshot(
    storage: &dyn RepositoryStorage,
    source: &SnapshotSource,
    part_path: &Path,
    on_progress: &mut dyn FnMut(u64),
) -> RemoteResult<()> {
    if let Some(parent) = part_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            io_error(
                format!("failed to create directory {}", parent.display()),
                error,
            )
        })?;
    }

    let resumable = storage.capabilities().range_read && source.expected_size.is_some();
    if !resumable {
        return download_snapshot_whole(storage, source, part_path, on_progress);
    }

    let expected_size = source.expected_size.unwrap_or(0);
    let mut offset = fs::metadata(part_path).map(|meta| meta.len()).unwrap_or(0);
    if offset > expected_size {
        let _ = fs::remove_file(part_path);
        offset = 0;
    }
    if offset == expected_size {
        return Ok(());
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(part_path)
        .map_err(|error| {
            io_error(
                format!("failed to open part file {}", part_path.display()),
                error,
            )
        })?;
    while offset < expected_size {
        let length = SNAPSHOT_DOWNLOAD_CHUNK_SIZE.min(expected_size - offset);
        match storage.read_range(&source.remote_path, offset, length, &mut file) {
            Ok(0) => {
                return Err(RemoteError::new(
                    RemoteErrorKind::RemoteIntegrityFailed,
                    format!(
                        "snapshot download of {} made no progress at offset {offset}",
                        source.remote_path
                    ),
                ));
            }
            Ok(written) => {
                offset += written;
                file.flush().map_err(|error| {
                    io_error(
                        format!("failed to flush part file {}", part_path.display()),
                        error,
                    )
                })?;
                on_progress(offset);
            }
            Err(error) => {
                let durable = fs::metadata(part_path)
                    .map(|meta| meta.len())
                    .unwrap_or(offset)
                    .min(expected_size);
                on_progress(durable);
                if durable == 0 {
                    let _ = fs::remove_file(part_path);
                }
                return Err(error);
            }
        }
    }
    Ok(())
}

fn download_snapshot_whole(
    storage: &dyn RepositoryStorage,
    source: &SnapshotSource,
    part_path: &Path,
    on_progress: &mut dyn FnMut(u64),
) -> RemoteResult<()> {
    let _ = fs::remove_file(part_path);
    let mut file = fs::File::create(part_path).map_err(|error| {
        io_error(
            format!("failed to create part file {}", part_path.display()),
            error,
        )
    })?;
    match storage.read(&source.remote_path, &mut file) {
        Ok(written) => {
            on_progress(written);
            Ok(())
        }
        Err(error) => {
            // Without range reads a partial file can never resume.
            let _ = fs::remove_file(part_path);
            Err(error)
        }
    }
}

fn verify_snapshot(part_path: &Path, source: &SnapshotSource) -> RemoteResult<VerifiedCandidate> {
    let size = fs::metadata(part_path)
        .map(|meta| meta.len())
        .map_err(|error| {
            io_error(
                format!("failed to stat candidate {}", part_path.display()),
                error,
            )
        })?;
    if let Some(expected_size) = source.expected_size {
        if size != expected_size {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!("downloaded database size {size} does not match expected {expected_size}"),
            ));
        }
    }
    let digest = sha256_file(part_path)?;
    if let Some(expected_digest) = &source.expected_digest {
        if !digest.eq_ignore_ascii_case(expected_digest) {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                format!(
                    "downloaded database digest {digest} does not match expected {expected_digest}"
                ),
            ));
        }
    }
    verify_sqlite_candidate(part_path)?;
    Ok(VerifiedCandidate { size, digest })
}

/// Swap a verified candidate into the working copy, preserving the current
/// database as last-known-good and restoring it when the install fails.
fn activate_candidate(candidate: &Path, working_copy: &WorkingCopyDatabase) -> RemoteResult<()> {
    let destination = working_copy.path();
    fsync_file(candidate)?;

    let lkg_path = working_copy.last_known_good_path();
    let mut preserved_lkg = false;
    if destination.exists() {
        let _ = fs::remove_file(&lkg_path);
        fs::rename(destination, &lkg_path).map_err(|error| {
            io_error(
                format!("failed to preserve last-known-good {}", lkg_path.display()),
                error,
            )
        })?;
        preserved_lkg = true;
    }

    if let Err(error) = fs::rename(candidate, destination) {
        if preserved_lkg && lkg_path.exists() {
            if let Err(restore_error) = fs::rename(&lkg_path, destination) {
                return Err(RemoteError::new(
                    kind_from_io_error(&restore_error),
                    format!(
                        "failed to install candidate database ({error}) and failed to \
                         restore last-known-good ({restore_error})"
                    ),
                ));
            }
        }
        return Err(io_error(
            format!(
                "failed to install candidate database as {}",
                destination.display()
            ),
            error,
        ));
    }

    fsync_parent(destination)?;
    Ok(())
}

pub(crate) fn verify_sqlite_candidate(candidate: &Path) -> RemoteResult<()> {
    let connection = open_readonly(candidate)?;

    let quick_check: String = connection
        .query_row("PRAGMA quick_check;", [], |row| row.get(0))
        .map_err(|error| integrity_error(format!("quick_check failed: {error}")))?;
    if quick_check != "ok" {
        return Err(integrity_error(format!(
            "quick_check reported corruption: {quick_check}"
        )));
    }

    let fk_violations: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_foreign_key_check;",
            [],
            |row| row.get(0),
        )
        .map_err(|error| integrity_error(format!("foreign_key_check failed: {error}")))?;
    if fk_violations > 0 {
        return Err(integrity_error(format!(
            "foreign_key_check reported {fk_violations} violation(s)"
        )));
    }

    let table_exists: bool = connection
        .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='songs' LIMIT 1;")
        .map_err(|error| integrity_error(format!("schema check prepare failed: {error}")))?
        .exists::<&[&dyn rusqlite::ToSql]>(&[])
        .map_err(|error| integrity_error(format!("schema check query failed: {error}")))?;
    if !table_exists {
        return Err(integrity_error(
            "candidate database is missing the required 'songs' table",
        ));
    }
    let mut statement = connection
        .prepare("PRAGMA table_info(\"songs\")")
        .map_err(|error| integrity_error(format!("schema check prepare failed: {error}")))?;
    let columns: Vec<String> = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| integrity_error(format!("schema check query failed: {error}")))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| integrity_error(format!("schema check query failed: {error}")))?;
    if !columns.iter().any(|column| column == "hash") {
        return Err(integrity_error(
            "candidate database 'songs' table is missing the 'hash' column",
        ));
    }
    Ok(())
}

pub(crate) fn open_readonly(path: &Path) -> RemoteResult<Connection> {
    let flags =
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    Connection::open_with_flags(path, flags)
        .map_err(|error| integrity_error(format!("failed to open candidate read-only: {error}")))
}

pub(crate) fn integrity_error(detail: impl Into<String>) -> RemoteError {
    RemoteError::new(RemoteErrorKind::RemoteIntegrityFailed, detail)
}

pub(crate) fn io_error(context: String, error: std::io::Error) -> RemoteError {
    RemoteError::new(kind_from_io_error(&error), format!("{context}: {error}"))
}

pub(crate) fn sha256_file(path: &Path) -> RemoteResult<String> {
    let mut file = fs::File::open(path).map_err(|error| {
        io_error(
            format!("failed to open {} for digest", path.display()),
            error,
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        use std::io::Read;
        let read = file.read(&mut buffer).map_err(|error| {
            io_error(
                format!("failed to read {} for digest", path.display()),
                error,
            )
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn fsync_file(path: &Path) -> RemoteResult<()> {
    let file = fs::File::open(path).map_err(|error| {
        io_error(
            format!("failed to open {} for fsync", path.display()),
            error,
        )
    })?;
    file.sync_all()
        .map_err(|error| io_error(format!("failed to fsync {}", path.display()), error))
}

fn fsync_parent(path: &Path) -> RemoteResult<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    #[cfg(unix)]
    {
        let dir = fs::File::open(parent).map_err(|error| {
            io_error(
                format!("failed to open {} for fsync", parent.display()),
                error,
            )
        })?;
        dir.sync_all().map_err(|error| {
            io_error(
                format!("failed to fsync directory {}", parent.display()),
                error,
            )
        })?;
    }
    #[cfg(not(unix))]
    {
        // Directory fsync is not meaningful on Windows.
        let _ = parent;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{
        database_path_for_generation, database_path_for_operation, CURRENT_SCHEMA_VERSION,
    };
    use crate::storage::{ObjectMetadata, StorageCapabilities};
    use crate::testing::{Fault, MemoryStorage, TestWebDavServer};
    use std::sync::{Arc, Mutex};

    struct AllowAll;
    impl RefreshPolicy for AllowAll {
        fn may_replace_working_copy(&self) -> bool {
            true
        }
    }

    struct DenyAll;
    impl RefreshPolicy for DenyAll {
        fn may_replace_working_copy(&self) -> bool {
            false
        }
    }

    fn snapshot_bytes(tag: &str) -> Vec<u8> {
        snapshot_bytes_with_padding(tag, 0)
    }

    fn snapshot_bytes_with_padding(tag: &str, padding: usize) -> Vec<u8> {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("snapshot.sqlite");
        {
            let connection = Connection::open(&db_path).expect("create snapshot database");
            connection
                .execute_batch(
                    "CREATE TABLE songs (\
                        hash TEXT PRIMARY KEY, \
                        title TEXT, \
                        imported_at INTEGER NOT NULL DEFAULT 0\
                    );",
                )
                .expect("create songs table");
            connection
                .execute(
                    "INSERT INTO songs (hash, title, imported_at) VALUES (?1, ?2, 1)",
                    rusqlite::params![format!("hash-{tag}"), format!("Song {tag}")],
                )
                .expect("insert song");
            if padding > 0 {
                connection
                    .execute_batch("CREATE TABLE padding (data BLOB);")
                    .expect("create padding table");
                // randomblob (not zeroblob): two padded snapshots must
                // differ in every chunk window, or a mid-download replacement
                // would splice indistinguishable tails.
                connection
                    .execute(
                        "INSERT INTO padding (data) VALUES (randomblob(?1))",
                        rusqlite::params![padding as i64],
                    )
                    .expect("pad snapshot");
            }
        }
        fs::read(&db_path).expect("read snapshot bytes")
    }

    fn digest_of(bytes: &[u8]) -> String {
        hex_lower(&Sha256::digest(bytes))
    }

    fn manifest_for(generation: i64, database_path: &str, snapshot: &[u8]) -> RepositoryManifest {
        RepositoryManifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            repository_id: "repo-1".to_owned(),
            generation,
            database_path: database_path.to_owned(),
            database_size_bytes: snapshot.len() as u64,
            database_sha256: digest_of(snapshot),
            committed_at_ms: 1_700_000_000_000,
            writer_id: "writer-1".to_owned(),
            operation_id: format!("op-{generation}"),
        }
    }

    /// Publish a new generation the way the writer does: immutable snapshot
    /// object first, then the manifest replace.
    fn publish(storage: &MemoryStorage, generation: i64, snapshot: &[u8]) -> RepositoryManifest {
        let manifest = manifest_for(
            generation,
            &database_path_for_generation(generation),
            snapshot,
        );
        publish_at(storage, manifest, snapshot)
    }

    fn publish_at(
        storage: &MemoryStorage,
        manifest: RepositoryManifest,
        snapshot: &[u8],
    ) -> RepositoryManifest {
        if storage
            .stat(&manifest.database_path)
            .expect("stat")
            .is_none()
        {
            storage
                .write_new(&manifest.database_path, snapshot)
                .expect("publish snapshot");
        }
        let manifest_json = manifest.to_json().expect("manifest json");
        match storage.stat(MANIFEST_PATH).expect("stat manifest") {
            Some(ObjectMetadata {
                revision: Some(revision),
                ..
            }) => {
                storage
                    .conditional_replace(MANIFEST_PATH, manifest_json.as_bytes(), &revision)
                    .expect("replace manifest");
            }
            _ => {
                storage
                    .write_new(MANIFEST_PATH, manifest_json.as_bytes())
                    .expect("write manifest");
            }
        }
        manifest
    }

    fn working_copy_of(dir: &tempfile::TempDir) -> WorkingCopyDatabase {
        WorkingCopyDatabase::new(dir.path().join("openkara.db"))
    }

    fn refreshed(outcome: RefreshOutcome) -> (RefreshedSnapshot, Option<String>) {
        match outcome {
            RefreshOutcome::Refreshed {
                snapshot,
                remote_revision,
            } => (snapshot, remote_revision),
            other => panic!("expected Refreshed, got {other:?}"),
        }
    }

    /// Records the windows every range read requests, to prove resume offsets.
    #[derive(Clone)]
    struct RecordingStorage {
        inner: MemoryStorage,
        ranges: Arc<Mutex<Vec<(u64, u64)>>>,
    }

    impl RecordingStorage {
        fn new(inner: MemoryStorage) -> Self {
            Self {
                inner,
                ranges: Arc::default(),
            }
        }

        fn recorded_ranges(&self) -> Vec<(u64, u64)> {
            self.ranges.lock().expect("ranges lock").clone()
        }

        fn script(&self, fault: Fault) {
            self.inner.script(fault);
        }
    }

    impl RepositoryStorage for RecordingStorage {
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
            self.ranges
                .lock()
                .expect("ranges lock")
                .push((offset, length));
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
            self.inner
                .conditional_replace(path, bytes, expected_revision)
        }

        fn list(&self, prefix: &str) -> RemoteResult<Vec<crate::storage::ListedObject>> {
            self.inner.list(prefix)
        }

        fn delete(&self, path: &str) -> RemoteResult<()> {
            self.inner.delete(path)
        }
    }

    /// Reports one object as size 0, the way a HEAD without a Content-Length
    /// header surfaces through the storage port.
    struct SizeHidingStorage {
        inner: MemoryStorage,
        hidden: String,
    }

    impl RepositoryStorage for SizeHidingStorage {
        fn capabilities(&self) -> StorageCapabilities {
            self.inner.capabilities()
        }

        fn stat(&self, path: &str) -> RemoteResult<Option<ObjectMetadata>> {
            Ok(self.inner.stat(path)?.map(|mut metadata| {
                if path == self.hidden {
                    metadata.size_bytes = 0;
                }
                metadata
            }))
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
            self.inner
                .conditional_replace(path, bytes, expected_revision)
        }

        fn list(&self, prefix: &str) -> RemoteResult<Vec<crate::storage::ListedObject>> {
            self.inner.list(prefix)
        }

        fn delete(&self, path: &str) -> RemoteResult<()> {
            self.inner.delete(path)
        }
    }

    #[test]
    fn the_manifest_size_is_the_fallback_when_the_database_stat_reports_no_size() {
        let storage = MemoryStorage::new();
        let snapshot = snapshot_bytes("one");
        let manifest = publish(&storage, 1, &snapshot);
        let hiding = SizeHidingStorage {
            inner: storage,
            hidden: manifest.database_path.clone(),
        };
        let dir = tempfile::tempdir().expect("temp dir");
        let working_copy = working_copy_of(&dir);

        let plan = plan_refresh(&hiding).expect("plan");
        assert_eq!(
            plan.database.expected_size,
            Some(manifest.database_size_bytes)
        );

        let (pulled, _) = refreshed(
            refresh_repository(&hiding, &working_copy, None, &AllowAll).expect("refresh"),
        );
        assert_eq!(pulled.database_sha256, digest_of(&snapshot));
        assert_eq!(
            fs::read(working_copy.path()).expect("working copy"),
            snapshot
        );
    }

    #[test]
    fn refresh_installs_the_published_snapshot() {
        let storage = MemoryStorage::new();
        let snapshot = snapshot_bytes("one");
        publish(&storage, 1, &snapshot);
        let dir = tempfile::tempdir().expect("temp dir");
        let working_copy = working_copy_of(&dir);

        let (pulled, revision) = refreshed(
            refresh_repository(&storage, &working_copy, None, &AllowAll).expect("refresh"),
        );

        assert_eq!(pulled.generation, 1);
        assert_eq!(pulled.database_size, snapshot.len() as u64);
        assert_eq!(pulled.database_sha256, digest_of(&snapshot));
        assert!(revision.is_some());
        assert_eq!(
            fs::read(working_copy.path()).expect("working copy"),
            snapshot,
            "the working copy holds the published snapshot byte for byte"
        );
    }

    #[test]
    fn refresh_is_unchanged_when_the_revision_matches() {
        let storage = MemoryStorage::new();
        let snapshot = snapshot_bytes("one");
        publish(&storage, 1, &snapshot);
        let dir = tempfile::tempdir().expect("temp dir");
        let working_copy = working_copy_of(&dir);
        let (_, revision) = refreshed(
            refresh_repository(&storage, &working_copy, None, &AllowAll).expect("refresh"),
        );

        let recording = RecordingStorage::new(storage);
        let outcome = refresh_repository(&recording, &working_copy, revision.as_deref(), &AllowAll)
            .expect("second refresh");

        assert_eq!(
            outcome,
            RefreshOutcome::Unchanged {
                remote_revision: revision
            }
        );
        assert!(
            recording.recorded_ranges().is_empty(),
            "an unchanged revision must not download"
        );
        assert!(
            fs::read_dir(dir.path())
                .expect("read dir")
                .all(|entry| !entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .contains(".part.")),
            "an unchanged revision must not leave part files"
        );
    }

    #[test]
    fn refresh_preserves_the_last_known_good_database() {
        let storage = MemoryStorage::new();
        let generation_one = snapshot_bytes("one");
        let generation_two = snapshot_bytes("two");
        publish(&storage, 1, &generation_one);
        let dir = tempfile::tempdir().expect("temp dir");
        let working_copy = working_copy_of(&dir);
        let (_, revision_one) = refreshed(
            refresh_repository(&storage, &working_copy, None, &AllowAll).expect("refresh"),
        );

        publish(&storage, 2, &generation_two);
        let (pulled, _) = refreshed(
            refresh_repository(&storage, &working_copy, revision_one.as_deref(), &AllowAll)
                .expect("second refresh"),
        );

        assert_eq!(pulled.generation, 2);
        assert_eq!(
            fs::read(working_copy.path()).expect("working copy"),
            generation_two
        );
        assert_eq!(
            fs::read(dir.path().join("openkara.db.lkg")).expect("last-known-good"),
            generation_one,
            "the previous verified database survives as last-known-good"
        );
    }

    #[test]
    fn refresh_refuses_to_replace_the_working_copy_when_the_policy_denies_it() {
        let storage = MemoryStorage::new();
        let snapshot = snapshot_bytes("one");
        publish(&storage, 1, &snapshot);
        let dir = tempfile::tempdir().expect("temp dir");
        let working_copy = working_copy_of(&dir);

        let outcome = refresh_repository(&storage, &working_copy, None, &DenyAll).expect("refresh");

        assert_eq!(outcome, RefreshOutcome::SkippedByPolicy);
        assert!(
            !working_copy.path().exists(),
            "a denied refresh must not touch the working copy"
        );
    }

    #[test]
    fn an_interrupted_download_resumes_from_the_received_prefix() {
        let storage = MemoryStorage::new();
        let snapshot = snapshot_bytes("one");
        let manifest = publish(&storage, 1, &snapshot);
        let recording = RecordingStorage::new(storage);
        let dir = tempfile::tempdir().expect("temp dir");
        let working_copy = working_copy_of(&dir);
        let plan = plan_refresh(&recording).expect("plan");
        let operation_id = snapshot_operation_id(plan.remote_revision.as_deref());
        let interrupt_at = snapshot.len() as u64 / 2;
        recording.script(Fault::InterruptTransfer {
            path: manifest.database_path.clone(),
            after_bytes: interrupt_at,
        });

        let error = pull_database_snapshot(
            &recording,
            &plan,
            &working_copy,
            &operation_id,
            &AllowAll,
            &mut |_| {},
        )
        .expect_err("the scripted interruption fails the first attempt");
        assert_eq!(error.kind, RemoteErrorKind::NetworkUnavailable);
        assert_eq!(error.code, "network_unavailable");
        assert!(error.retryable, "an interrupted transfer stays retryable");
        assert_eq!(
            fs::metadata(working_copy.part_path(&operation_id))
                .expect("part file")
                .len(),
            interrupt_at,
            "the received prefix stays on disk"
        );

        let plan = plan_refresh(&recording).expect("plan");
        let pulled = pull_database_snapshot(
            &recording,
            &plan,
            &working_copy,
            &operation_id,
            &AllowAll,
            &mut |_| {},
        )
        .expect("resume")
        .expect("the policy allows the pull");
        assert_eq!(pulled.database_sha256, digest_of(&snapshot));
        assert_eq!(
            fs::read(working_copy.path()).expect("working copy"),
            snapshot
        );

        let ranges = recording.recorded_ranges();
        assert_eq!(
            ranges[1],
            (interrupt_at, snapshot.len() as u64 - interrupt_at),
            "the retry resumes at the durable prefix instead of starting over"
        );
    }

    #[test]
    fn a_revision_moving_mid_download_fails_integrity_and_keeps_the_working_copy() {
        let storage = MemoryStorage::new();
        let snapshot = snapshot_bytes_with_padding("one", 9 * 1024 * 1024);
        assert!(snapshot.len() as u64 > SNAPSHOT_DOWNLOAD_CHUNK_SIZE);
        let replacement = snapshot_bytes_with_padding("two", 9 * 1024 * 1024);
        let manifest = publish(&storage, 1, &snapshot);
        let dir = tempfile::tempdir().expect("temp dir");
        let working_copy = working_copy_of(&dir);
        let plan = plan_refresh(&storage).expect("plan");
        let operation_id = snapshot_operation_id(plan.remote_revision.as_deref());
        storage.script(Fault::MoveRevision {
            path: manifest.database_path.clone(),
            bytes: replacement,
        });

        let error = pull_database_snapshot(
            &storage,
            &plan,
            &working_copy,
            &operation_id,
            &AllowAll,
            &mut |_| {},
        )
        .expect_err("bytes from two revisions can never verify");

        assert_eq!(error.kind, RemoteErrorKind::RemoteIntegrityFailed);
        assert!(!error.retryable);
        assert!(
            !working_copy.path().exists(),
            "a failed pull leaves the working copy untouched"
        );
        assert!(
            !working_copy.part_path(&operation_id).exists(),
            "corrupt bytes are deleted: they can never resume into a valid snapshot"
        );
    }

    #[test]
    fn a_digest_mismatch_is_an_integrity_failure() {
        let storage = MemoryStorage::new();
        let snapshot = snapshot_bytes("one");
        let mut manifest = manifest_for(1, &database_path_for_generation(1), &snapshot);
        manifest.database_sha256 = "00".repeat(32);
        publish_at(&storage, manifest, &snapshot);
        let dir = tempfile::tempdir().expect("temp dir");
        let working_copy = working_copy_of(&dir);

        let error = refresh_repository(&storage, &working_copy, None, &AllowAll)
            .expect_err("a wrong digest must fail");

        assert_eq!(error.kind, RemoteErrorKind::RemoteIntegrityFailed);
        assert_eq!(error.code, "remote_integrity_failed");
        assert!(!working_copy.path().exists());
    }

    #[test]
    fn a_size_mismatch_is_an_integrity_failure() {
        let storage = MemoryStorage::with_capabilities(StorageCapabilities {
            conditional_replace: true,
            range_read: false,
        });
        let snapshot = snapshot_bytes("one");
        publish(&storage, 1, &snapshot);
        let dir = tempfile::tempdir().expect("temp dir");
        let working_copy = working_copy_of(&dir);
        let mut plan = plan_refresh(&storage).expect("plan");
        plan.database.expected_size = Some(snapshot.len() as u64 + 128);

        let error = pull_database_snapshot(
            &storage,
            &plan,
            &working_copy,
            "pull-size",
            &AllowAll,
            &mut |_| {},
        )
        .expect_err("a truncated snapshot must fail");

        assert_eq!(error.kind, RemoteErrorKind::RemoteIntegrityFailed);
        assert!(!working_copy.path().exists());
    }

    #[test]
    fn a_corrupt_sqlite_snapshot_is_rejected() {
        let storage = MemoryStorage::new();
        let garbage = vec![0xABu8; 4096];
        publish_at(
            &storage,
            manifest_for(1, &database_path_for_generation(1), &garbage),
            &garbage,
        );
        let dir = tempfile::tempdir().expect("temp dir");
        let working_copy = working_copy_of(&dir);

        let error = refresh_repository(&storage, &working_copy, None, &AllowAll)
            .expect_err("garbage bytes are not a database");

        assert_eq!(error.kind, RemoteErrorKind::RemoteIntegrityFailed);
        assert!(!working_copy.path().exists());
    }

    #[test]
    fn a_snapshot_without_the_songs_table_is_rejected() {
        let storage = MemoryStorage::new();
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("no-songs.sqlite");
        {
            let connection = Connection::open(&db_path).expect("create database");
            connection
                .execute_batch("CREATE TABLE not_songs (id INTEGER PRIMARY KEY);")
                .expect("create table");
        }
        let snapshot = fs::read(&db_path).expect("read bytes");
        publish_at(
            &storage,
            manifest_for(1, &database_path_for_generation(1), &snapshot),
            &snapshot,
        );
        let working_copy = working_copy_of(&dir);

        let error = refresh_repository(&storage, &working_copy, None, &AllowAll)
            .expect_err("a database without the songs table is incompatible");

        assert_eq!(error.kind, RemoteErrorKind::RemoteIntegrityFailed);
        assert!(
            error
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("songs")),
            "the error names the missing table: {error:?}"
        );
        assert!(!working_copy.path().exists());
    }

    #[test]
    fn a_missing_snapshot_object_surfaces_object_not_found() {
        let storage = MemoryStorage::new();
        let snapshot = snapshot_bytes("one");
        let manifest = manifest_for(1, &database_path_for_generation(1), &snapshot);
        storage
            .write_new(MANIFEST_PATH, manifest.to_json().unwrap().as_bytes())
            .expect("write manifest without the snapshot");
        let dir = tempfile::tempdir().expect("temp dir");
        let working_copy = working_copy_of(&dir);

        let error = refresh_repository(&storage, &working_copy, None, &AllowAll)
            .expect_err("the snapshot object is absent");

        assert_eq!(error.kind, RemoteErrorKind::ObjectNotFound);
        assert_eq!(error.code, "remote_object_not_found");
    }

    #[test]
    fn a_legacy_repository_without_a_manifest_refreshes_from_openkara_db() {
        let storage = MemoryStorage::new();
        let snapshot = snapshot_bytes("legacy");
        storage
            .write_new(LEGACY_DATABASE_PATH, &snapshot)
            .expect("seed legacy database");
        let dir = tempfile::tempdir().expect("temp dir");
        let working_copy = working_copy_of(&dir);

        let (pulled, revision) = refreshed(
            refresh_repository(&storage, &working_copy, None, &AllowAll).expect("refresh"),
        );

        assert_eq!(
            pulled.generation, 0,
            "a legacy repository has no generation"
        );
        assert!(revision.is_some());
        assert_eq!(
            fs::read(working_copy.path()).expect("working copy"),
            snapshot
        );
    }

    #[test]
    fn storage_without_range_reads_downloads_the_whole_file_without_resume() {
        let storage = MemoryStorage::with_capabilities(StorageCapabilities {
            conditional_replace: true,
            range_read: false,
        });
        let snapshot = snapshot_bytes("one");
        publish(&storage, 1, &snapshot);
        let dir = tempfile::tempdir().expect("temp dir");
        let working_copy = working_copy_of(&dir);

        let (pulled, _) = refreshed(
            refresh_repository(&storage, &working_copy, None, &AllowAll).expect("refresh"),
        );
        assert_eq!(pulled.database_sha256, digest_of(&snapshot));

        // A whole-file download that is interrupted leaves nothing resumable.
        let dir2 = tempfile::tempdir().expect("temp dir");
        let working_copy2 = working_copy_of(&dir2);
        let generation_two = publish(&storage, 2, &snapshot_bytes("two"));
        storage.script(Fault::InterruptTransfer {
            path: generation_two.database_path.clone(),
            after_bytes: 10,
        });
        let plan = plan_refresh(&storage).expect("plan");
        let error = pull_database_snapshot(
            &storage,
            &plan,
            &working_copy2,
            "pull-whole",
            &AllowAll,
            &mut |_| {},
        )
        .expect_err("the whole-file read is interrupted");
        assert_eq!(error.kind, RemoteErrorKind::NetworkUnavailable);
        assert!(
            !working_copy2.part_path("pull-whole").exists(),
            "without range reads a partial file can never resume"
        );
    }

    /// A repository written by the current v1 writer: the manifest is the
    /// exact JSON the writer serializes and the snapshot a real SQLite
    /// database. The reader must refresh both the legacy
    /// `.openkara/databases/<generation>.sqlite` layout and the hardened
    /// `.openkara/databases/<generation>/<operation hash>.sqlite` layout.
    #[test]
    fn a_repository_written_by_todays_code_refreshes_identically() {
        for (generation, database_path) in [
            (1, database_path_for_generation(1)),
            (2, database_path_for_operation(2, "publish-op-2")),
        ] {
            let storage = MemoryStorage::new();
            let snapshot = snapshot_bytes("fixture");
            let manifest_json = format!(
                "{{\"schema_version\":1,\"repository_id\":\"repo-fixture\",\"generation\":{generation},\"database_path\":\"{database_path}\",\"database_size_bytes\":{},\"database_sha256\":\"{}\",\"committed_at_ms\":1700000000000,\"writer_id\":\"desktop-writer\",\"operation_id\":\"publish-op-{generation}\"}}",
                snapshot.len(),
                digest_of(&snapshot),
            );
            storage
                .write_new(&database_path, &snapshot)
                .expect("seed snapshot");
            storage
                .write_new(MANIFEST_PATH, manifest_json.as_bytes())
                .expect("seed manifest");
            let dir = tempfile::tempdir().expect("temp dir");
            let working_copy = working_copy_of(&dir);

            let (pulled, revision) = refreshed(
                refresh_repository(&storage, &working_copy, None, &AllowAll).expect("refresh"),
            );

            assert_eq!(pulled.generation, generation);
            assert_eq!(
                fs::read(working_copy.path()).expect("working copy"),
                snapshot,
                "generation {generation} installs byte for byte"
            );
            let connection = open_readonly(working_copy.path()).expect("open installed copy");
            let title: String = connection
                .query_row(
                    "SELECT title FROM songs WHERE hash = 'hash-fixture'",
                    [],
                    |row| row.get(0),
                )
                .expect("read song");
            assert_eq!(title, "Song fixture");

            let outcome =
                refresh_repository(&storage, &working_copy, revision.as_deref(), &AllowAll)
                    .expect("refresh again");
            assert!(
                matches!(outcome, RefreshOutcome::Unchanged { .. }),
                "the same repository refreshes to a no-op afterwards"
            );
            assert_eq!(
                fs::read(working_copy.path()).expect("working copy"),
                snapshot,
                "the working copy is identical before and after"
            );
        }
    }

    mod webdav {
        use super::*;

        fn publish_webdav(
            server: &TestWebDavServer,
            generation: i64,
            snapshot: &[u8],
        ) -> RepositoryManifest {
            let manifest = manifest_for(
                generation,
                &database_path_for_generation(generation),
                snapshot,
            );
            server
                .storage()
                .write_new(&manifest.database_path, snapshot)
                .expect("seed snapshot");
            server
                .storage()
                .write_new(MANIFEST_PATH, manifest.to_json().unwrap().as_bytes())
                .expect("seed manifest");
            manifest
        }

        #[test]
        fn refresh_installs_the_published_snapshot_over_webdav() {
            let server = TestWebDavServer::start();
            let snapshot = snapshot_bytes("webdav");
            let manifest = publish_webdav(&server, 1, &snapshot);
            let dir = tempfile::tempdir().expect("temp dir");
            let working_copy = working_copy_of(&dir);
            let storage = server.storage().clone();

            let (pulled, revision) = refreshed(
                refresh_repository(&storage, &working_copy, None, &AllowAll).expect("refresh"),
            );

            assert_eq!(pulled.generation, 1);
            assert_eq!(
                fs::read(working_copy.path()).expect("working copy"),
                snapshot
            );
            let manifest_revision = storage
                .stat(MANIFEST_PATH)
                .expect("stat")
                .and_then(|metadata| metadata.revision);
            assert_eq!(revision, manifest_revision);
            assert_eq!(manifest.database_path, pulled.remote_path);
        }

        #[test]
        fn refresh_over_webdav_downloads_nothing_when_the_revision_is_unchanged() {
            let server = TestWebDavServer::start();
            let snapshot = snapshot_bytes("webdav");
            publish_webdav(&server, 1, &snapshot);
            let dir = tempfile::tempdir().expect("temp dir");
            let working_copy = working_copy_of(&dir);
            let storage = server.storage().clone();
            let (_, revision) = refreshed(
                refresh_repository(&storage, &working_copy, None, &AllowAll).expect("refresh"),
            );
            let gets_before = server.get_request_count(&database_path_for_generation(1));

            let outcome =
                refresh_repository(&storage, &working_copy, revision.as_deref(), &AllowAll)
                    .expect("second refresh");

            assert!(matches!(outcome, RefreshOutcome::Unchanged { .. }));
            assert_eq!(
                server.get_request_count(&database_path_for_generation(1)),
                gets_before,
                "an unchanged revision must not re-download the snapshot"
            );
        }

        #[test]
        fn an_interrupted_webdav_download_resumes() {
            let server = TestWebDavServer::start();
            let snapshot = snapshot_bytes("webdav-resume");
            let manifest = publish_webdav(&server, 1, &snapshot);
            let dir = tempfile::tempdir().expect("temp dir");
            let working_copy = working_copy_of(&dir);
            let storage = server.storage().clone();
            let interrupt_at = snapshot.len() as u64 / 2;
            server.script(Fault::InterruptTransfer {
                path: manifest.database_path.clone(),
                after_bytes: interrupt_at,
            });

            let error = refresh_repository(&storage, &working_copy, None, &AllowAll)
                .expect_err("the scripted interruption fails the first refresh");
            assert_eq!(error.kind, RemoteErrorKind::NetworkUnavailable);
            assert!(error.retryable);

            let (pulled, _) = refreshed(
                refresh_repository(&storage, &working_copy, None, &AllowAll)
                    .expect("the retry resumes and completes"),
            );
            assert_eq!(pulled.database_sha256, digest_of(&snapshot));
            assert_eq!(
                fs::read(working_copy.path()).expect("working copy"),
                snapshot
            );
        }

        #[test]
        fn a_revision_moving_between_plan_and_pull_over_webdav_fails_integrity() {
            let server = TestWebDavServer::start();
            let snapshot = snapshot_bytes_with_padding("webdav-one", 9 * 1024 * 1024);
            let replacement = snapshot_bytes_with_padding("webdav-two", 9 * 1024 * 1024);
            let manifest = publish_webdav(&server, 1, &snapshot);
            let dir = tempfile::tempdir().expect("temp dir");
            let working_copy = working_copy_of(&dir);
            let storage = server.storage().clone();
            server.script(Fault::MoveRevision {
                path: manifest.database_path.clone(),
                bytes: replacement,
            });

            let error = refresh_repository(&storage, &working_copy, None, &AllowAll)
                .expect_err("bytes from two revisions can never verify");

            assert_eq!(error.kind, RemoteErrorKind::RemoteIntegrityFailed);
            assert!(!working_copy.path().exists());
        }
    }
}
