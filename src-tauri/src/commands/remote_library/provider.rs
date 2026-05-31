use crate::commands::error::{CommandError, CommandResult};
use crate::config::RegisteredLibrary;
use crate::library::error::LibraryError;
use std::path::Path;

/// Unified interface for remote storage providers (Google Drive, Dropbox, WebDAV).
///
/// Each provider implementation holds its loaded secret internally and handles
/// token refresh transparently (for OAuth-based providers).
pub(crate) trait RemoteProvider {
    /// Get the revision/etag for a relative path, or `None` if it doesn't exist.
    fn get_revision(&self, relative_path: &str) -> CommandResult<Option<String>>;

    /// Download a remote file to a local destination path.
    fn download_file(&self, relative_path: &str, destination: &Path) -> CommandResult<()>;

    /// Upload a local file (from the library's working copy) to the remote.
    fn upload_file(&self, relative_path: &str) -> CommandResult<()>;

    /// Upload a local directory (from the library's working copy) to the remote.
    fn upload_directory(&self, relative_path: &str) -> CommandResult<()>;

    /// Delete a remote path.
    fn delete_path(&self, relative_path: &str) -> CommandResult<()>;

    /// Initialize or sync the library (ensure folders exist, download DB if needed).
    fn initialize_or_sync(&self) -> CommandResult<Option<String>>;
}

/// Create a `RemoteProvider` implementation for the given library.
///
/// Loads the provider's stored secret and constructs the appropriate provider struct.
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
