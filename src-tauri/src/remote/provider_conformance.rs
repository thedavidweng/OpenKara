use super::{
    errors::{RemoteProviderCapabilities, RemoteResult},
    provider::{RemoteMediaSource, RemoteMediaSourceCapabilities, RepositoryStorage},
};

pub(crate) fn assert_provider_capabilities(
    storage: &dyn RepositoryStorage,
    expected_storage: RemoteProviderCapabilities,
    expected_media: RemoteMediaSourceCapabilities,
) {
    assert_eq!(storage.capabilities(), expected_storage);
    assert_eq!(storage.media_source().capabilities(), expected_media);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        commands::error::{internal_error, CommandResult},
        remote::{errors::RemoteErrorKind, provider::ConditionalSource},
    };
    use std::{
        collections::HashMap,
        fs,
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    };
    use tempfile::tempdir;

    struct ScriptedProvider {
        files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        root: PathBuf,
    }

    impl ScriptedProvider {
        fn new(root: PathBuf) -> Self {
            Self {
                files: Arc::new(Mutex::new(HashMap::new())),
                root,
            }
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
                .then(|| "scripted-revision".to_owned()))
        }

        fn stat(
            &self,
            path: &str,
        ) -> CommandResult<Option<crate::remote::errors::RemoteObjectMetadata>> {
            let files = self.files.lock().expect("scripted provider lock");
            Ok(files
                .get(path)
                .map(|bytes| crate::remote::errors::RemoteObjectMetadata {
                    size_bytes: Some(bytes.len() as u64),
                    revision: Some("scripted-revision".to_owned()),
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
        ) -> RemoteResult<crate::remote::errors::RemoteObjectMetadata> {
            let bytes = source.read_bytes().map_err(|error| {
                crate::remote::errors::RemoteError::new(
                    RemoteErrorKind::NetworkUnavailable,
                    error.message,
                )
            })?;
            let size_bytes = bytes.len() as u64;
            self.files
                .lock()
                .expect("scripted provider lock")
                .insert(path.to_owned(), bytes);
            Ok(crate::remote::errors::RemoteObjectMetadata {
                size_bytes: Some(size_bytes),
                revision: Some("scripted-revision".to_owned()),
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
            Ok(Some("scripted-revision".to_owned()))
        }

        fn refresh_existing(&self) -> CommandResult<Option<String>> {
            Ok(Some("scripted-revision".to_owned()))
        }
    }

    impl RemoteMediaSource for ScriptedProvider {
        fn capabilities(&self) -> RemoteMediaSourceCapabilities {
            RemoteMediaSourceCapabilities {
                range_download: true,
            }
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
}
