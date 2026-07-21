//! Shared Remote Repository bootstrap protocol.
//!
//! Provider adapters implement [`RemoteBootstrapStorage`] (HTTP/path ops only).
//! The deep module here owns CreateOrOpen vs RequireExisting policy.
//!
//! ## Refresh Repository vs Reauthorize Repository
//!
//! - **Refresh Repository** (`sync_active_remote_library` / revision pull) reuses
//!   already-bound Repository Credentials and pulls the latest Remote Revision
//!   into the Local Working Copy. It does not re-run OAuth or rewrite secrets.
//! - **Reauthorize Repository** re-runs provider auth, rebinds Repository
//!   Credentials, then bootstraps with [`BootstrapMode::RequireExisting`] so a
//!   wrong folder cannot be silently initialized as a new empty library.
//! - **Register Repository** (first attach) uses [`BootstrapMode::CreateOrOpen`]
//!   to create the marker + layout directories and seed `openkara.db` when the
//!   remote root is empty.
//!
//! ## Legacy migration
//!
//! Repositories created before the manifest protocol store the database at
//! `openkara.db` in the remote root and have no `.openkara-repository.json`
//! manifest. Bootstrap probes the manifest first; only repositories without a
//! manifest use the legacy root database path. On first publication the
//! executor treats a missing manifest as generation 0 and publishes
//! generation 1. A deferred GC later removes the legacy root `openkara.db`
//! once the migration is safely committed.
//!
//! Empty repository creation (CreateOrOpen with no remote database) may still
//! seed a root `openkara.db`. That seed is a one-time bootstrap artifact, not
//! the ongoing publication path.

use crate::{
    cache,
    commands::error::{internal_error, CommandError, CommandResult},
    config::RegisteredLibrary,
    library::error::LibraryError,
    library_root::LibraryRoot,
};
use std::{fs, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootstrapMode {
    /// Register / first open: ensure layout dirs, create marker if missing,
    /// upload local DB when remote has none.
    CreateOrOpen,
    /// Reauthorize / strict open: remote marker + committed database must
    /// already exist (manifest generation DB or legacy openkara.db).
    RequireExisting,
}

/// Result of probing the committed remote database during bootstrap.
#[derive(Debug, Clone)]
pub(crate) struct CommittedDatabaseProbe {
    /// Staleness token for the visibility switch (manifest revision when
    /// present, otherwise the legacy openkara.db revision).
    pub revision: Option<String>,
    /// Relative path of the database object that [`RemoteBootstrapStorage::download_database`]
    /// must fetch.
    pub database_path: String,
    /// Manifest generation when a manifest was found; 0 for legacy root DB.
    pub generation: i64,
    /// Expected byte length from the manifest (or None for legacy).
    pub database_size: Option<u64>,
    /// Expected hex SHA-256 from the manifest (or None for legacy).
    pub database_sha256: Option<String>,
}

/// Provider-owned remote storage ops used by the shared bootstrap protocol.
///
/// Implementations should not open the local LibraryRoot or decide CreateOrOpen
/// vs RequireExisting — that policy lives in [`bootstrap_remote_library`].
pub(crate) trait RemoteBootstrapStorage {
    /// Noun phrase for RequireExisting errors, e.g. "Google Drive folder",
    /// "Dropbox folder", "WebDAV path".
    fn location_label(&self) -> &'static str;

    /// Ensure remote root layout directories exist (media, media-g, stems, and
    /// any provider-specific root folder materialization).
    fn ensure_layout(&mut self) -> CommandResult<()>;

    fn marker_exists(&mut self) -> CommandResult<bool>;

    fn upload_marker(&mut self, marker_bytes: &[u8]) -> CommandResult<()>;

    /// Probe the committed remote database.
    ///
    /// Prefer the repository manifest (`.openkara-repository.json`) and the
    /// generation-specific database it references. Fall back to the legacy
    /// root `openkara.db` only when no manifest exists.
    ///
    /// Return `Ok(None)` only when neither a manifest database nor a legacy
    /// root database is present. When a database is present, return
    /// `Ok(Some(probe))` even if the provider cannot supply a revision token
    /// (`probe.revision` may be `None`) — callers must not treat a missing
    /// etag as a missing file.
    fn probe_committed_database(&mut self) -> CommandResult<Option<CommittedDatabaseProbe>>;

    /// Download the database path discovered by the most recent successful
    /// [`probe_committed_database`] call into `destination`.
    fn download_database(&mut self, database_path: &str, destination: &Path) -> CommandResult<()>;

    /// Seed a new empty repository with a root `openkara.db`. Used only when
    /// CreateOrOpen finds no committed database. Ongoing publication never
    /// uses this path — the executor uploads generation-specific databases.
    fn upload_database(&mut self, source: &Path) -> CommandResult<Option<String>>;
}

pub(crate) fn bootstrap_remote_library(
    mode: BootstrapMode,
    library: &RegisteredLibrary,
    storage: &mut dyn RemoteBootstrapStorage,
) -> CommandResult<Option<String>> {
    let root = open_or_create_local_working_copy(library)?;

    match mode {
        BootstrapMode::CreateOrOpen => {
            storage.ensure_layout()?;
            if !storage.marker_exists()? {
                let marker_path = root.resolve(".openkara-library");
                let marker_bytes = b"openkara remote repository\n";
                fs::write(&marker_path, marker_bytes).map_err(|error| {
                    CommandError::from(LibraryError::Internal(format!(
                        "failed to write {}: {error}",
                        marker_path.display()
                    )))
                })?;
                storage.upload_marker(marker_bytes)?;
            }

            match storage.probe_committed_database()? {
                Some(probe) => {
                    activate_committed_database(storage, &root, &probe)?;
                    Ok(probe.revision)
                }
                None => {
                    // Empty repository seed: one-time root openkara.db upload.
                    // First publication migrates to the manifest protocol.
                    let uploaded = storage.upload_database(&root.database_path())?;
                    Ok(match uploaded {
                        Some(revision) => Some(revision),
                        None => storage
                            .probe_committed_database()?
                            .and_then(|probe| probe.revision),
                    })
                }
            }
        }
        BootstrapMode::RequireExisting => {
            // Do not create layout or marker on reauthorize: a missing marker
            // means the user pointed at the wrong remote folder.
            if !storage.marker_exists()? {
                return Err(CommandError::from(LibraryError::Internal(format!(
                    "The selected {} is not an OpenKara remote repository.",
                    storage.location_label()
                ))));
            }
            let probe = match storage.probe_committed_database()? {
                Some(probe) => probe,
                None => {
                    return Err(CommandError::from(LibraryError::Internal(format!(
                        "The selected {} is missing a committed database \
                         (.openkara-repository.json or openkara.db).",
                        storage.location_label()
                    ))));
                }
            };
            activate_committed_database(storage, &root, &probe)?;
            Ok(probe.revision)
        }
    }
}

/// Download the committed remote database to a temp path, verify size and
/// SHA-256 when known, run SQLite integrity checks, fsync, preserve LKG, and
/// atomically rename into the working path. Register/Reauthorize must not
/// write a corrupt or truncated generation DB directly to the final path.
fn activate_committed_database(
    storage: &mut dyn RemoteBootstrapStorage,
    root: &LibraryRoot,
    probe: &CommittedDatabaseProbe,
) -> CommandResult<()> {
    let destination = root.database_path();
    let temp_path = destination.with_extension(format!("db.part.bootstrap-{}", probe.generation));
    // Download to temp (never directly to the final working path).
    let _ = fs::remove_file(&temp_path);
    storage.download_database(&probe.database_path, &temp_path)?;

    let actual_size = fs::metadata(&temp_path)
        .map(|m| m.len())
        .map_err(|e| internal_error(format!("failed to stat bootstrap candidate: {e}")))?;
    if let Some(expected_size) = probe.database_size {
        if actual_size != expected_size {
            let _ = fs::remove_file(&temp_path);
            return Err(internal_error(format!(
                "bootstrap database size mismatch: expected {expected_size}, got {actual_size}"
            )));
        }
    }

    if let Some(ref expected_sha) = probe.database_sha256 {
        let actual = crate::remote::control_db::sha256_file(&temp_path)
            .map_err(|e| internal_error(e.message))?;
        if !actual.eq_ignore_ascii_case(expected_sha) {
            let _ = fs::remove_file(&temp_path);
            return Err(internal_error(format!(
                "bootstrap database digest mismatch: expected {expected_sha}, got {actual}"
            )));
        }
    }

    // SQLite integrity: quick_check + foreign_key_check via the shared helper.
    crate::remote::atomic_download::verify_sqlite_integrity_pub(&temp_path)?;

    if let Ok(file) = fs::File::open(&temp_path) {
        let _ = file.sync_all();
    }

    // Preserve last-known-good before replacing.
    if destination.exists() {
        let lkg = destination.with_extension("db.lkg");
        let _ = fs::remove_file(&lkg);
        let _ = fs::rename(&destination, &lkg);
    }
    fs::rename(&temp_path, &destination).map_err(|e| {
        internal_error(format!(
            "failed to activate bootstrap database at {}: {e}",
            destination.display()
        ))
    })?;
    if let Some(parent) = destination.parent() {
        if let Ok(dir) = fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

fn open_or_create_local_working_copy(library: &RegisteredLibrary) -> CommandResult<LibraryRoot> {
    let root_path = library.working_copy_root().ok_or_else(|| {
        CommandError::from(LibraryError::Internal(
            "remote repository is missing a cached working copy".to_string(),
        ))
    })?;
    let root = if root_path.join(".openkara-library").exists() {
        LibraryRoot::open(&root_path).map_err(internal_error)?
    } else {
        LibraryRoot::create(&root_path).map_err(internal_error)?
    };
    cache::initialize_library_database(&root.database_path())
        .map_err(|e| CommandError::from(LibraryError::DatabaseUnavailable(e.to_string())))?;
    Ok(root)
}
