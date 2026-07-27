use crate::audio::remote_source::HttpFetcher;
use crate::commands::error::{CommandError, CommandResult};
use crate::config::{RegisteredLibrary, RemoteLibraryProvider};
use crate::library::error::LibraryError;
use crate::remote::errors::{
    RemoteError, RemoteErrorKind, RemoteObjectMetadata, RemoteProviderCapabilities, RemoteResult,
};
use std::path::{Path, PathBuf};

/// Source bytes for a conditional replace operation. Providers read from the
/// temp file or in-memory bytes and upload them to the target path.
pub(crate) enum ConditionalSource {
    /// Upload from a temp file on disk.
    #[allow(dead_code)]
    TempFile(PathBuf),
    /// Upload from in-memory bytes (e.g. a small manifest JSON blob).
    Bytes(Vec<u8>),
}

impl ConditionalSource {
    /// Read the source bytes into a `Vec`.
    pub(crate) fn read_bytes(&self) -> CommandResult<Vec<u8>> {
        match self {
            ConditionalSource::TempFile(path) => std::fs::read(path).map_err(|e| {
                crate::commands::error::internal_error(format!(
                    "failed to read conditional source {}: {e}",
                    path.display()
                ))
            }),
            ConditionalSource::Bytes(bytes) => Ok(bytes.clone()),
        }
    }
}

/// Unified interface for remote storage providers (Google Drive, Dropbox, WebDAV).
///
/// Each provider implementation holds its loaded secret internally and handles
/// token refresh transparently (for OAuth-based providers).
pub(crate) trait RemoteProvider {
    /// Report the capabilities this provider supports. Providers that cannot
    /// enforce a capability return `false` for it; the operation executor
    /// fails closed rather than silently downgrading to last-writer-wins.
    fn capabilities(&self) -> RemoteProviderCapabilities {
        RemoteProviderCapabilities::default()
    }

    fn get_revision(&self, relative_path: &str) -> CommandResult<Option<String>>;

    /// Stat a remote object: return its size and provider revision (ETag /
    /// Dropbox rev / Google Drive headRevisionId) when available. Returns
    /// `Ok(None)` when the object does not exist. The default implementation
    /// falls back to `get_revision` + `get_file_size` so providers without a
    /// dedicated `stat` still work, but a dedicated override is preferred for
    /// atomicity (a single round-trip).
    fn stat(&self, relative_path: &str) -> CommandResult<Option<RemoteObjectMetadata>> {
        let revision = self.get_revision(relative_path)?;
        let size = self.get_file_size(relative_path)?;
        if revision.is_none() && size.is_none() {
            // Distinguish "absent" from "present but no metadata". When both
            // are None, treat the object as absent. Providers with a real
            // `stat` override return None only on a true 404.
            Ok(None)
        } else {
            Ok(Some(RemoteObjectMetadata { size, revision }))
        }
    }

    fn download_file(&self, relative_path: &str, destination: &Path) -> CommandResult<()>;

    /// Download a byte range `[offset, offset + length)` from a remote object
    /// and write it to the destination file at the given offset. Used by the
    /// resumable download path to resume from a verified offset after an
    /// interrupted transfer.
    ///
    /// Returns the actual number of verified bytes written. The caller
    /// advances its offset by this count, not by the requested `length`,
    /// so a short or full-body response is handled correctly.
    ///
    /// Implementations MUST:
    /// - Require `206 Partial Content` (or a deliberately supported full-body
    ///   fallback when `offset == 0`).
    /// - Validate the `Content-Range` header when present.
    /// - Validate the response body length matches the requested range (or
    ///   the full file size for a full-body fallback).
    /// - Reject short and oversized responses.
    ///
    /// The default implementation returns
    /// [`RemoteErrorKind::ProviderCapabilityUnavailable`] so providers opt in.
    /// A provider that does not support Range requests must NOT silently
    /// download the full file — the caller checks `range_download` first.
    fn download_range(
        &self,
        _relative_path: &str,
        _destination: &Path,
        _offset: u64,
        _length: u64,
    ) -> RemoteResult<u64> {
        Err(RemoteError::from_kind(
            RemoteErrorKind::ProviderCapabilityUnavailable,
        ))
    }

    fn upload_file(&self, relative_path: &str) -> CommandResult<()>;

    /// Upload `bytes` to `relative_path` using a resumable upload mechanism
    /// (provider-specific: Dropbox upload sessions, Google Drive resumable
    /// upload, WebDAV staged PUT + MOVE). `operation_id` and `control_db` are
    /// used to persist transfer progress so a restart can resume.
    ///
    /// The default implementation returns
    /// [`RemoteErrorKind::ProviderCapabilityUnavailable`] so providers opt in.
    /// The caller checks `resumable_upload` first and falls back to
    /// `upload_file` when unsupported.
    // used by PR#5: resumable uploads
    fn resumable_upload_bytes(
        &self,
        _relative_path: &str,
        _bytes: &[u8],
        _operation_id: &str,
        _control_db: &rusqlite::Connection,
    ) -> RemoteResult<()> {
        Err(RemoteError::from_kind(
            RemoteErrorKind::ProviderCapabilityUnavailable,
        ))
    }

    fn delete_path(&self, relative_path: &str) -> CommandResult<()>;

    /// Conditionally replace the object at `path` with `source`, enforcing a
    /// compare-and-swap precondition:
    /// - `expected_revision = Some(rev)`: the replace succeeds only if the
    ///   current remote revision matches `rev`. A mismatch yields
    ///   [`RemoteErrorKind::RemoteConflict`].
    /// - `expected_revision = None`: conditional-create semantics — the
    ///   replace succeeds only if the object does not already exist. A
    ///   pre-existing object yields `RemoteConflict`.
    ///
    /// Returns the metadata (size + new revision) of the committed object.
    ///
    /// The default implementation returns
    /// [`RemoteErrorKind::ProviderCapabilityUnavailable`] so providers opt in
    /// per-capability. A provider that cannot enforce CAS must NOT silently
    /// downgrade to an unconditional overwrite.
    fn conditional_replace(
        &self,
        _path: &str,
        _source: ConditionalSource,
        _expected_revision: Option<&str>,
    ) -> RemoteResult<RemoteObjectMetadata> {
        Err(RemoteError::from_kind(
            RemoteErrorKind::ProviderCapabilityUnavailable,
        ))
    }

    /// Register / first open: shared bootstrap `CreateOrOpen` mode (create
    /// layout/marker when empty, seed or pull `openkara.db`).
    fn initialize_or_sync(&self) -> CommandResult<Option<String>>;

    /// Create an `HttpFetcher` for streaming byte ranges from a remote file.
    ///
    /// Returns `Ok(Some(fetcher))` if the provider supports Range requests,
    /// `Ok(None)` if it doesn't (caller should fall back to full-file download).
    fn create_range_fetcher(
        &self,
        _relative_path: &str,
    ) -> CommandResult<Option<Box<dyn HttpFetcher>>> {
        Ok(None)
    }

    fn get_file_size(&self, _relative_path: &str) -> CommandResult<Option<u64>> {
        Ok(None)
    }

    /// Reauthorize open: shared bootstrap `RequireExisting` mode — marker + DB
    /// must already exist (never silently create a new empty remote library).
    /// Distinct from Refresh Repository (revision pull with existing credentials).
    fn refresh_existing(&self) -> CommandResult<Option<String>>;
}

pub(crate) fn create_provider<'a>(
    app_data_dir: &'a Path,
    library: &'a RegisteredLibrary,
) -> CommandResult<Box<dyn RemoteProvider + 'a>> {
    use super::dropbox;
    use super::google_drive;
    use super::webdav;
    use crate::config::RemoteLibraryProvider;

    match library.provider() {
        Some(RemoteLibraryProvider::WebDav) => {
            let secret = webdav::load_webdav_secret(app_data_dir, library)?;
            Ok(Box::new(webdav::WebDAVProvider::new(
                app_data_dir,
                secret,
                library,
            )))
        }
        Some(RemoteLibraryProvider::GoogleDrive) => {
            let secret = google_drive::load_google_drive_secret(app_data_dir, library)?;
            Ok(Box::new(google_drive::GoogleDriveProvider::new(
                app_data_dir,
                secret,
                library,
            )))
        }
        Some(RemoteLibraryProvider::Dropbox) => {
            let secret = dropbox::load_dropbox_secret(app_data_dir, library)?;
            Ok(Box::new(dropbox::DropboxProvider::new(
                app_data_dir,
                secret,
                library,
            )))
        }
        None => Err(CommandError::from(LibraryError::Internal(
            "the target library is not a remote library".to_owned(),
        ))),
    }
}

/// Shared helper used by both `RemoteProvider` implementations and session-based
/// code that doesn't yet have a provider instance.
pub(crate) fn compute_remote_path_display(
    provider: RemoteLibraryProvider,
    remote_root_locator: &str,
    display_name: &str,
) -> String {
    match provider {
        RemoteLibraryProvider::WebDav => {
            super::webdav::remote_path_display_from_url(remote_root_locator)
        }
        RemoteLibraryProvider::GoogleDrive => {
            super::google_drive::google_drive_root_display_name(display_name)
        }
        RemoteLibraryProvider::Dropbox => remote_root_locator.to_owned(),
    }
}
