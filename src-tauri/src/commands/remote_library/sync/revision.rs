use crate::{
    commands::error::{CommandError, CommandResult},
    config::{AppConfig, RegisteredLibrary, RemoteLibraryProvider},
    library::error::LibraryError,
    AppState,
};
use std::path::Path;

use super::super::types::{
    current_unix_time_ms, load_app_config, load_remote_root, persist_app_config,
};

use super::super::provider::create_provider;

pub(crate) fn update_remote_revision_in_config(
    app_data_dir: &Path,
    library_id: &str,
    remote_revision: Option<String>,
) -> CommandResult<()> {
    let mut config = load_app_config(app_data_dir)?;
    if let Some(RegisteredLibrary::Remote {
        remote_revision: revision,
        ..
    }) = config
        .libraries
        .iter_mut()
        .find(|entry| entry.id() == library_id)
    {
        *revision = remote_revision.or(Some(current_unix_time_ms().to_string()));
    }
    persist_app_config(app_data_dir, &config)
}

pub(crate) fn load_registered_remote_library(
    app_data_dir: &Path,
    library_id: &str,
) -> CommandResult<RegisteredLibrary> {
    let config = load_app_config(app_data_dir)?;
    let library = config
        .libraries
        .iter()
        .find(|entry| entry.id() == library_id)
        .ok_or_else(|| {
            CommandError::from(LibraryError::Internal(format!(
                "remote repository {library_id} was not found"
            )))
        })?;
    if !matches!(library, RegisteredLibrary::Remote { .. }) {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "library {library_id} is not a remote repository"
        ))));
    }
    Ok(library.clone())
}

pub(crate) fn remote_database_revision(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
) -> CommandResult<Option<String>> {
    let provider = create_provider(app_data_dir, library)?;
    provider.get_revision("openkara.db")
}

pub(crate) fn remote_database_revision_is_stale(
    stored_revision: Option<&str>,
    provider_revision: Option<&str>,
) -> bool {
    provider_revision.is_some_and(|revision| Some(revision) != stored_revision)
}

pub(crate) fn remote_database_conflict_error(provider_revision: Option<&str>) -> CommandError {
    let revision_detail = provider_revision
        .map(|revision| format!(" Remote revision: {revision}."))
        .unwrap_or_default();
    CommandError::from(LibraryError::Internal(format!(
        "Remote repository database changed on another device before this publish. \
         OpenKara stopped before overwriting it. Use Settings > Karaoke Library > \
         Refresh remote repository, then retry this edit. If refresh fails because authentication \
         or the server changed, use Reauthorize remote repository first.{revision_detail}"
    )))
}

pub(crate) fn sync_remote_database_from_provider(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
) -> CommandResult<RegisteredLibrary> {
    let provider = create_provider(app_data_dir, library)?;
    let revision = provider.initialize_or_sync()?;
    update_remote_revision_in_config(app_data_dir, library.id(), revision)?;
    load_registered_remote_library(app_data_dir, library.id())
}

pub(crate) fn prepare_remote_database_for_mutation(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
) -> CommandResult<RegisteredLibrary> {
    let provider_revision = remote_database_revision(app_data_dir, library)?;
    if remote_database_revision_is_stale(library.remote_revision(), provider_revision.as_deref()) {
        return sync_remote_database_from_provider(app_data_dir, library);
    }
    Ok(library.clone())
}

pub(crate) fn ensure_remote_database_upload_safe(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
) -> CommandResult<()> {
    let provider_revision = remote_database_revision(app_data_dir, library)?;
    if remote_database_revision_is_stale(library.remote_revision(), provider_revision.as_deref()) {
        return Err(remote_database_conflict_error(provider_revision.as_deref()));
    }
    Ok(())
}

pub(crate) fn upload_remote_database(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
) -> CommandResult<()> {
    ensure_remote_database_upload_safe(app_data_dir, library)?;

    let provider = create_provider(app_data_dir, library)?;
    provider.upload_file("openkara.db")?;
    let new_revision = provider.get_revision("openkara.db")?;
    // WebDAV servers that don't return an ETag yield Ok(None) here; that is
    // acceptable — we persist None as the revision rather than failing a
    // successful upload.
    if new_revision.is_none()
        && matches!(
            library.provider(),
            Some(RemoteLibraryProvider::GoogleDrive) | Some(RemoteLibraryProvider::Dropbox)
        )
    {
        return Err(CommandError::from(LibraryError::Internal(format!(
            "{} database file is missing after upload",
            match library.provider() {
                Some(RemoteLibraryProvider::GoogleDrive) => "Google Drive",
                Some(RemoteLibraryProvider::Dropbox) => "Dropbox",
                _ => "Remote",
            }
        ))));
    }
    update_remote_revision_in_config(app_data_dir, library.id(), new_revision)
}

pub fn active_remote_library(app_data_dir: &Path) -> CommandResult<Option<RegisteredLibrary>> {
    let config = load_app_config(app_data_dir)?;
    let Some(active_library) = config.active_library() else {
        return Ok(None);
    };
    if !matches!(active_library, RegisteredLibrary::Remote { .. }) {
        return Ok(None);
    }
    Ok(Some(active_library.clone()))
}

pub fn sync_active_remote_database_if_needed(app_data_dir: &Path) -> CommandResult<()> {
    let Some(library) = active_remote_library(app_data_dir)? else {
        return Ok(());
    };
    upload_remote_database(app_data_dir, &library)
}

pub fn prepare_active_remote_database_for_mutation(app_data_dir: &Path) -> CommandResult<()> {
    let Some(library) = active_remote_library(app_data_dir)? else {
        return Ok(());
    };
    let _ = prepare_remote_database_for_mutation(app_data_dir, &library)?;
    Ok(())
}

pub fn ensure_remote_file_cached(app_data_dir: &Path, relative_path: &str) -> CommandResult<()> {
    let Some(library) = active_remote_library(app_data_dir)? else {
        return Ok(());
    };
    let root = load_remote_root(app_data_dir, &library)?;
    let destination = root.resolve(relative_path);
    if destination.exists() {
        return Ok(());
    }

    let provider = create_provider(app_data_dir, &library)?;
    provider.download_file(relative_path, &destination)
}

pub(crate) fn resolve_active_remote(config: &AppConfig) -> Option<RegisteredLibrary> {
    config.active_library().and_then(|library| match library {
        RegisteredLibrary::Remote { .. } => Some(library.clone()),
        RegisteredLibrary::Local { .. } => None,
    })
}

pub(crate) fn sync_active_remote_library(state: &AppState) -> CommandResult<()> {
    let config = load_app_config(&state.shell.app_data_dir)?;
    let Some(active_library) = config.active_library() else {
        return Err(CommandError::from(LibraryError::Internal(
            "no library is currently active".to_string(),
        )));
    };

    if matches!(active_library, RegisteredLibrary::Remote { .. }) {
        let _ = sync_remote_database_from_provider(&state.shell.app_data_dir, active_library)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_revision_is_stale_when_provider_revision_changed() {
        assert!(!remote_database_revision_is_stale(None, None));
        assert!(!remote_database_revision_is_stale(Some("rev-1"), None));
        assert!(!remote_database_revision_is_stale(
            Some("rev-1"),
            Some("rev-1")
        ));
        assert!(remote_database_revision_is_stale(None, Some("rev-1")));
        assert!(remote_database_revision_is_stale(
            Some("rev-1"),
            Some("rev-2")
        ));
    }

    #[test]
    fn database_conflict_error_points_to_settings_recovery_actions() {
        let error = remote_database_conflict_error(Some("rev-2"));

        assert!(error.retryable);
        assert!(error.message.contains("Refresh remote repository"));
        assert!(error.message.contains("Reauthorize remote repository"));
    }
}
