use super::{
    errors::{
        RemoteError, RemoteErrorKind, RemoteObjectMetadata, RemoteProviderCapabilities,
        RemoteResult,
    },
    provider::{
        ConditionalSource, RemoteMediaSource, RemoteMediaSourceCapabilities, RepositoryStorage,
    },
};
use crate::commands::error::{internal_error, CommandResult};
use std::{
    collections::HashMap,
    fs,
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub(crate) fn assert_provider_capabilities(
    storage: &dyn RepositoryStorage,
    expected_storage: RemoteProviderCapabilities,
    expected_media: RemoteMediaSourceCapabilities,
) {
    assert_eq!(storage.capabilities(), expected_storage);
    assert_eq!(storage.media_source().capabilities(), expected_media);
}

pub(crate) const SCRIPTED_REVISION: &str = "scripted-revision";

/// In-memory Remote Provider adapter for tests. Handles obtained with
/// [`ScriptedProvider::handle`] share the same repository contents and Remote
/// Revision, so a test can seed the remote side once and hand fresh boxed
/// adapters to production code as often as it asks for storage.
pub(crate) struct ScriptedProvider {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    revision: Arc<Mutex<String>>,
    root: PathBuf,
}

impl ScriptedProvider {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            files: Arc::new(Mutex::new(HashMap::new())),
            revision: Arc::new(Mutex::new(SCRIPTED_REVISION.to_owned())),
            root,
        }
    }

    pub(crate) fn handle(&self) -> Self {
        Self {
            files: Arc::clone(&self.files),
            revision: Arc::clone(&self.revision),
            root: self.root.clone(),
        }
    }

    pub(crate) fn put_file(&self, path: &str, bytes: Vec<u8>) {
        self.files
            .lock()
            .expect("scripted provider lock")
            .insert(path.to_owned(), bytes);
    }

    pub(crate) fn set_revision(&self, revision: &str) {
        *self.revision.lock().expect("scripted provider lock") = revision.to_owned();
    }

    fn revision(&self) -> String {
        self.revision
            .lock()
            .expect("scripted provider lock")
            .clone()
    }
}

impl RepositoryStorage for ScriptedProvider {
    fn media_source(&self) -> &dyn RemoteMediaSource {
        self
    }

    fn capabilities(&self) -> RemoteProviderCapabilities {
        RemoteProviderCapabilities {
            conditional_replace: true,
            resumable_upload: false,
            revision_metadata: true,
            server_side_move: false,
        }
    }

    fn get_revision(&self, path: &str) -> CommandResult<Option<String>> {
        Ok(self
            .files
            .lock()
            .expect("scripted provider lock")
            .contains_key(path)
            .then(|| self.revision()))
    }

    fn stat(&self, path: &str) -> CommandResult<Option<RemoteObjectMetadata>> {
        let files = self.files.lock().expect("scripted provider lock");
        Ok(files.get(path).map(|bytes| RemoteObjectMetadata {
            size_bytes: Some(bytes.len() as u64),
            revision: Some(self.revision()),
        }))
    }

    fn download_file(&self, path: &str, destination: &Path) -> CommandResult<()> {
        let bytes = self
            .files
            .lock()
            .expect("scripted provider lock")
            .get(path)
            .cloned()
            .ok_or_else(|| internal_error(format!("missing scripted file {path}")))?;
        fs::write(destination, bytes).map_err(|error| internal_error(error.to_string()))
    }

    fn upload_file(&self, path: &str) -> CommandResult<()> {
        let source = self.root.join(path);
        let bytes = fs::read(source).map_err(|error| internal_error(error.to_string()))?;
        self.files
            .lock()
            .expect("scripted provider lock")
            .insert(path.to_owned(), bytes);
        Ok(())
    }

    fn conditional_replace(
        &self,
        path: &str,
        source: ConditionalSource,
        _expected_revision: Option<&str>,
    ) -> RemoteResult<RemoteObjectMetadata> {
        let bytes = source.read_bytes().map_err(|error| {
            RemoteError::new(RemoteErrorKind::NetworkUnavailable, error.message)
        })?;
        let size_bytes = bytes.len() as u64;
        self.files
            .lock()
            .expect("scripted provider lock")
            .insert(path.to_owned(), bytes);
        Ok(RemoteObjectMetadata {
            size_bytes: Some(size_bytes),
            revision: Some(self.revision()),
        })
    }

    fn delete_path(&self, path: &str) -> CommandResult<()> {
        self.files
            .lock()
            .expect("scripted provider lock")
            .remove(path);
        Ok(())
    }

    fn initialize_or_sync(&self) -> CommandResult<Option<String>> {
        Ok(Some(self.revision()))
    }

    fn refresh_existing(&self) -> CommandResult<Option<String>> {
        Ok(Some(self.revision()))
    }
}

impl RemoteMediaSource for ScriptedProvider {
    fn capabilities(&self) -> RemoteMediaSourceCapabilities {
        RemoteMediaSourceCapabilities {
            range_download: true,
        }
    }

    fn download_range(
        &self,
        path: &str,
        destination: &Path,
        offset: u64,
        length: u64,
    ) -> RemoteResult<u64> {
        let bytes = self
            .files
            .lock()
            .expect("scripted provider lock")
            .get(path)
            .cloned()
            .ok_or_else(|| {
                RemoteError::new(
                    RemoteErrorKind::PermissionDenied,
                    format!("missing scripted file {path}"),
                )
            })?;
        let start = offset as usize;
        let end = (offset + length) as usize;
        if end > bytes.len() {
            return Err(RemoteError::new(
                RemoteErrorKind::RemoteIntegrityFailed,
                "range beyond file size",
            ));
        }
        let chunk = &bytes[start..end];
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(destination)
            .map_err(|error| {
                RemoteError::new(RemoteErrorKind::NetworkUnavailable, error.to_string())
            })?;
        file.seek(SeekFrom::Start(offset)).map_err(|error| {
            RemoteError::new(RemoteErrorKind::NetworkUnavailable, error.to_string())
        })?;
        file.write_all(chunk).map_err(|error| {
            RemoteError::new(RemoteErrorKind::NetworkUnavailable, error.to_string())
        })?;
        Ok(chunk.len() as u64)
    }

    fn get_file_size(&self, path: &str) -> CommandResult<Option<u64>> {
        Ok(self
            .files
            .lock()
            .expect("scripted provider lock")
            .get(path)
            .map(|bytes| bytes.len() as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scripted_adapter_uses_the_shared_storage_and_media_seam() {
        let directory = tempdir().expect("temp directory should create");
        let provider = ScriptedProvider::new(directory.path().to_owned());
        assert_provider_capabilities(
            &provider,
            RemoteProviderCapabilities {
                conditional_replace: true,
                resumable_upload: false,
                revision_metadata: true,
                server_side_move: false,
            },
            RemoteMediaSourceCapabilities {
                range_download: true,
            },
        );

        assert!(provider
            .stat("missing")
            .expect("stat should succeed")
            .is_none());
        assert!(provider
            .media_source()
            .get_file_size("missing")
            .expect("size lookup should succeed")
            .is_none());
    }

    #[test]
    fn handles_share_contents_and_revision() {
        let directory = tempdir().expect("temp directory should create");
        let provider = ScriptedProvider::new(directory.path().to_owned());
        let handle = provider.handle();

        provider.put_file("openkara.db", b"contents".to_vec());
        provider.set_revision("rev-2");

        assert_eq!(
            handle
                .get_revision("openkara.db")
                .expect("revision lookup should succeed"),
            Some("rev-2".to_owned())
        );
        assert_eq!(
            handle
                .media_source()
                .get_file_size("openkara.db")
                .expect("size lookup should succeed"),
            Some(8)
        );
    }
}
