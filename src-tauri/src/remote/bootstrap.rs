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
    CreateOrOpen,
    RequireExisting,
}

pub(crate) trait RemoteBootstrapStorage {
    fn location_label(&self) -> &'static str;

    fn ensure_layout(&mut self) -> CommandResult<()>;

    fn marker_exists(&mut self) -> CommandResult<bool>;

    fn upload_marker(&mut self, marker_bytes: &[u8]) -> CommandResult<()>;

    fn probe_remote_database(&mut self) -> CommandResult<Option<Option<String>>>;

    fn download_database(&mut self, destination: &Path) -> CommandResult<()>;

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
        LibraryRoot::open(&root_path).map_err(internal_error)?
    } else {
        LibraryRoot::create(&root_path).map_err(internal_error)?
    };
    cache::initialize_library_database(&root.database_path())
        .map_err(|e| CommandError::from(LibraryError::DatabaseUnavailable(e.to_string())))?;
    Ok(root)
}
