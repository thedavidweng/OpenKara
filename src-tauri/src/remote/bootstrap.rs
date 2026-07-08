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

use crate::{
    cache,
    commands::error::{CommandError, CommandResult},
    config::RegisteredLibrary,
    library::error::LibraryError,
    library_root::LibraryRoot,
};
use std::{fs, path::Path};

/// Whether bootstrap may create a new remote library layout or must open one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootstrapMode {
    /// Register / first open: ensure layout dirs, create marker if missing,
    /// upload local DB when remote has none.
    CreateOrOpen,
    /// Reauthorize / strict open: remote marker + openkara.db must already exist.
    RequireExisting,
}

/// Provider-owned remote storage ops used by the shared bootstrap protocol.
///
/// Implementations should not open the local LibraryRoot or decide CreateOrOpen
/// vs RequireExisting — that policy lives in [`bootstrap_remote_library`].
pub(crate) trait RemoteBootstrapStorage {
    /// Noun phrase for RequireExisting errors, e.g. "Google Drive folder",
    /// "Dropbox folder", "WebDAV path" (preserves historical copy).
    fn location_label(&self) -> &'static str;

    /// Ensure remote root layout directories exist (media, media-g, stems, and
    /// any provider-specific root folder materialization).
    fn ensure_layout(&mut self) -> CommandResult<()>;

    /// Whether the remote `.openkara-library` marker exists.
    fn marker_exists(&mut self) -> CommandResult<bool>;

    /// Upload the library marker to the remote root.
    fn upload_marker(&mut self, marker_bytes: &[u8]) -> CommandResult<()>;

    /// Probe remote `openkara.db`.
    ///
    /// Return `Ok(None)` only when the file is absent. When present, return
    /// `Ok(Some(revision))` even if the provider cannot supply a revision
    /// token (`revision` may be `None`) — callers must not treat a missing
    /// etag as a missing file (that would overwrite a populated remote DB).
    fn probe_remote_database(&mut self) -> CommandResult<Option<Option<String>>>;

    /// Download remote `openkara.db` into `destination`.
    fn download_database(&mut self, destination: &Path) -> CommandResult<()>;

    /// Upload local `openkara.db` and return the new remote revision when known.
    fn upload_database(&mut self, source: &Path) -> CommandResult<Option<String>>;
}

/// One deep bootstrap protocol shared by all Remote Providers.
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

            match storage.probe_remote_database()? {
                Some(revision) => {
                    storage.download_database(&root.database_path())?;
                    Ok(revision)
                }
                None => {
                    let uploaded = storage.upload_database(&root.database_path())?;
                    Ok(match uploaded {
                        Some(revision) => Some(revision),
                        None => storage
                            .probe_remote_database()?
                            .and_then(|revision| revision),
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
            let revision = match storage.probe_remote_database()? {
                Some(revision) => revision,
                None => {
                    return Err(CommandError::from(LibraryError::Internal(format!(
                        "The selected {} is missing openkara.db.",
                        storage.location_label()
                    ))));
                }
            };
            storage.download_database(&root.database_path())?;
            Ok(revision)
        }
    }
}

fn open_or_create_local_working_copy(library: &RegisteredLibrary) -> CommandResult<LibraryRoot> {
    let root_path = library.working_copy_root().ok_or_else(|| {
        CommandError::from(LibraryError::Internal(
            "remote repository is missing a cached working copy".to_string(),
        ))
    })?;
    let root = if root_path.join(".openkara-library").exists() {
        LibraryRoot::open(&root_path)
            .map_err(|e| CommandError::from(LibraryError::Internal(e.to_string())))?
    } else {
        LibraryRoot::create(&root_path)
            .map_err(|e| CommandError::from(LibraryError::Internal(e.to_string())))?
    };
    cache::initialize_library_database(&root.database_path())
        .map_err(|e| CommandError::from(LibraryError::DatabaseUnavailable(e.to_string())))?;
    Ok(root)
}
