use crate::audio::remote_source::HttpFetcher;
use crate::commands::error::{CommandError, CommandResult};
use crate::config::{RegisteredLibrary, RemoteLibraryProvider};
use crate::library::error::LibraryError;
use std::path::Path;

/// Unified interface for remote storage providers (Google Drive, Dropbox, WebDAV).
///
/// Each provider implementation holds its loaded secret internally and handles
/// token refresh transparently (for OAuth-based providers).
pub(crate) trait RemoteProvider {
    fn get_revision(&self, relative_path: &str) -> CommandResult<Option<String>>;

    fn download_file(&self, relative_path: &str, destination: &Path) -> CommandResult<()>;

    fn upload_file(&self, relative_path: &str) -> CommandResult<()>;

    fn upload_directory(&self, relative_path: &str) -> CommandResult<()>;

    fn delete_path(&self, relative_path: &str) -> CommandResult<()>;

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
