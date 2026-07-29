use crate::audio::remote_source::HttpFetcher;
use crate::commands::error::{CommandError, CommandResult};
use crate::config::{RegisteredLibrary, RemoteLibraryProvider};
use crate::library::error::LibraryError;
use crate::remote::errors::{
    RemoteError, RemoteErrorKind, RemoteObjectMetadata, RemoteProviderCapabilities, RemoteResult,
};
use std::path::{Path, PathBuf};

pub(crate) enum ConditionalSource {
    #[allow(dead_code)]
    TempFile(PathBuf),
    Bytes(Vec<u8>),
}

impl ConditionalSource {
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

pub(crate) trait RemoteProvider {
    fn capabilities(&self) -> RemoteProviderCapabilities {
        RemoteProviderCapabilities::default()
    }

    fn get_revision(&self, relative_path: &str) -> CommandResult<Option<String>>;

    fn stat(&self, relative_path: &str) -> CommandResult<Option<RemoteObjectMetadata>> {
        let revision = self.get_revision(relative_path)?;
        let size = self.get_file_size(relative_path)?;
        if revision.is_none() && size.is_none() {
            // Distinguish "absent" from "present but no metadata". When both
            // are None, treat the object as absent. Providers with a real
            // `stat` override return None only on a true 404.
            Ok(None)
        } else {
            Ok(Some(RemoteObjectMetadata {
                size_bytes: size,
                revision,
            }))
        }
    }

    fn download_file(&self, relative_path: &str, destination: &Path) -> CommandResult<()>;

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

    fn initialize_or_sync(&self) -> CommandResult<Option<String>>;

    fn create_range_fetcher(
        &self,
        _relative_path: &str,
    ) -> CommandResult<Option<Box<dyn HttpFetcher>>> {
        Ok(None)
    }

    fn get_file_size(&self, _relative_path: &str) -> CommandResult<Option<u64>> {
        Ok(None)
    }

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
