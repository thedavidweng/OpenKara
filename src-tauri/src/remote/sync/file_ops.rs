use crate::{
    commands::error::{internal_error, CommandError, CommandResult},
    library::error::LibraryError,
    library_root::LibraryRoot,
};
use std::{fs, path::Path};

pub(crate) fn copy_file_if_present(source: Option<&Path>, destination: &Path) -> CommandResult<()> {
    let Some(source) = source else {
        return Ok(());
    };

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to create {}: {error}",
                parent.display()
            )))
        })?;
    }

    fs::copy(source, destination).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to copy {} to {}: {error}",
            source.display(),
            destination.display()
        )))
    })?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn copy_directory_recursive(source: &Path, destination: &Path) -> CommandResult<()> {
    if !source.exists() {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "source directory {} does not exist",
            source.display()
        ))));
    }

    if destination.exists() {
        fs::remove_dir_all(destination).map_err(|error| {
            CommandError::from(LibraryError::Internal(format!(
                "failed to clear destination directory {}: {error}",
                destination.display()
            )))
        })?;
    }
    fs::create_dir_all(destination).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to create destination directory {}: {error}",
            destination.display()
        )))
    })?;

    for entry in fs::read_dir(source).map_err(|error| {
        CommandError::from(LibraryError::Internal(format!(
            "failed to read directory {}: {error}",
            source.display()
        )))
    })? {
        let entry = entry.map_err(internal_error)?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());

        if source_path.is_dir() {
            copy_directory_recursive(&source_path, &destination_path)?;
        } else {
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    CommandError::from(LibraryError::Internal(format!(
                        "failed to create destination directory {}: {error}",
                        parent.display()
                    )))
                })?;
            }
            fs::copy(&source_path, &destination_path).map_err(|error| {
                CommandError::from(LibraryError::Internal(format!(
                    "failed to copy {} to {}: {error}",
                    source_path.display(),
                    destination_path.display()
                )))
            })?;
        }
    }

    Ok(())
}

pub(crate) fn copy_remote_song_assets(
    local_root: &LibraryRoot,
    remote_root: &LibraryRoot,
    source_relative_path: &str,
    destination_relative_path: &str,
) -> CommandResult<()> {
    let source_path = local_root.resolve(source_relative_path);
    let destination_path = remote_root.resolve(destination_relative_path);
    copy_file_if_present(Some(&source_path), &destination_path)
}
