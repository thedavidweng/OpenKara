use crate::{
    commands::error::{CommandError, CommandResult},
    config::{AppConfig, RegisteredLibrary, RemoteLibraryProvider},
    library::error::LibraryError,
    remote::atomic_download::{
        atomic_database_pull, reconcile_database_state_after_restart, DatabasePullOptions,
    },
    remote::control_db::{get_repository_state, LocalState},
    AppState,
};
use rusqlite::Connection;
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
    control_db_conn: &Connection,
    app_data_dir: &Path,
    library: &RegisteredLibrary,
) -> CommandResult<RegisteredLibrary> {
    // Reconcile any restart that completed the rename but not the local-state
    // update before deciding whether to pull again.
    reconcile_after_restart(control_db_conn, app_data_dir, library)?;

    let provider = create_provider(app_data_dir, library)?;
    let revision = provider.initialize_or_sync()?;
    update_remote_revision_in_config(app_data_dir, library.id(), revision)?;
    load_registered_remote_library(app_data_dir, library.id())
}

/// Reconcile repository state if a prior pull completed the rename but not the
/// control-DB state update. This runs before every refresh so a crash between
/// rename and state-update does not leave the control DB describing a stale
/// digest.
fn reconcile_after_restart(
    control_db_conn: &Connection,
    _app_data_dir: &Path,
    library: &RegisteredLibrary,
) -> CommandResult<()> {
    let root_path = library.working_copy_root().ok_or_else(|| {
        CommandError::from(LibraryError::Internal(
            "remote repository is missing a cached working copy".to_string(),
        ))
    })?;
    if !root_path.join(".openkara-library").exists() {
        // First-time setup — no working copy to reconcile against.
        return Ok(());
    }
    let root = crate::library_root::LibraryRoot::open(&root_path)
        .map_err(crate::commands::error::internal_error)?;
    if root.database_path().exists() {
        let _ = reconcile_database_state_after_restart(control_db_conn, &root, library.id());
    }
    Ok(())
}

pub(crate) fn prepare_remote_database_for_mutation(
    control_db_conn: &Connection,
    app_data_dir: &Path,
    library: &RegisteredLibrary,
) -> CommandResult<RegisteredLibrary> {
    // Dirty working-copy protection: before any refresh that would overwrite
    // openkara.db, consult the durable repository state. A dirty/publishing/
    // conflicted/reauth-required working copy holds committed local edits that
    // must NOT be overwritten by an automatic pull — otherwise network loss or
    // a failed publication would silently destroy the user's work.
    if !should_allow_automatic_pull(control_db_conn, library) {
        // Preserve the current working copy. PR#4 drives the actual resume
        // publication; PR#3 only blocks the overwrite.
        tracing::info!(
            "skipping automatic remote database pull for library {} because \
             the working copy is not clean",
            library.id()
        );
        return Ok(library.clone());
    }

    let provider_revision = remote_database_revision(app_data_dir, library)?;
    if remote_database_revision_is_stale(library.remote_revision(), provider_revision.as_deref()) {
        // The remote advanced. Pull a verified candidate atomically. On
        // network/pull failure, fall back to the existing local DB so the
        // mutation can proceed offline instead of blocking the user.
        return pull_remote_database_atomically(
            control_db_conn,
            app_data_dir,
            library,
            provider_revision.as_deref(),
        );
    }
    Ok(library.clone())
}

/// Decide whether an automatic pull may overwrite the working copy. Only
/// `Clean` allows it; every other state preserves the working copy.
fn should_allow_automatic_pull(control_db_conn: &Connection, library: &RegisteredLibrary) -> bool {
    match get_repository_state(control_db_conn, library.id()) {
        Ok(Some(state)) => matches!(state.local_state, LocalState::Clean),
        // No state row yet (e.g. a library registered before the control DB
        // existed): allow the pull so first-time refresh works.
        Ok(None) => true,
        // If the control DB is unreadable, do not block the user — allow the
        // pull. The atomic pull itself validates the candidate, so a corrupt
        // control DB cannot cause a bad overwrite.
        Err(_) => true,
    }
}

/// Atomically pull and validate the remote database into the working copy.
/// On success, updates the stored revision and returns the reloaded library.
/// On failure, falls back to the existing local DB (returns the library
/// unchanged) so the caller can proceed offline.
fn pull_remote_database_atomically(
    control_db_conn: &Connection,
    app_data_dir: &Path,
    library: &RegisteredLibrary,
    provider_revision: Option<&str>,
) -> CommandResult<RegisteredLibrary> {
    let provider = create_provider(app_data_dir, library)?;
    let root = load_remote_root(app_data_dir, library)?;

    let operation_id = format!("pull-{}", provider_revision.unwrap_or("unknown"));

    let expected_size = provider.get_file_size("openkara.db")?;

    match atomic_database_pull(
        provider.as_ref(),
        control_db_conn,
        &root,
        DatabasePullOptions {
            operation_id: &operation_id,
            expected_size,
            expected_digest: None,
            library_id: library.id(),
        },
    ) {
        Ok(_) => {
            update_remote_revision_in_config(
                app_data_dir,
                library.id(),
                provider_revision.map(|s| s.to_owned()),
            )?;
            load_registered_remote_library(app_data_dir, library.id())
        }
        Err(error) => {
            // Offline / network error or integrity failure: fall back to the
            // last verified working copy. Do not block the mutation. The
            // candidate temp file is cleaned up by `atomic_database_pull`.
            tracing::info!(
                "remote database pull failed for library {}; falling back to \
                 the existing local database: {:?}",
                library.id(),
                error
            );
            Ok(library.clone())
        }
    }
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

#[allow(dead_code)] // used by mutation::sync_backend in non-test builds
pub fn sync_active_remote_database_if_needed(app_data_dir: &Path) -> CommandResult<()> {
    let Some(library) = active_remote_library(app_data_dir)? else {
        return Ok(());
    };
    upload_remote_database(app_data_dir, &library)
}

#[allow(dead_code)] // used by mutation::sync_backend in non-test builds
pub fn prepare_active_remote_database_for_mutation(
    control_db_conn: &Connection,
    app_data_dir: &Path,
) -> CommandResult<()> {
    let Some(library) = active_remote_library(app_data_dir)? else {
        return Ok(());
    };
    let _ = prepare_remote_database_for_mutation(control_db_conn, app_data_dir, &library)?;
    Ok(())
}

pub fn ensure_remote_file_cached(app_data_dir: &Path, relative_path: &str) -> CommandResult<()> {
    let Some(library) = active_remote_library(app_data_dir)? else {
        return Ok(());
    };
    let root = load_remote_root(app_data_dir, &library)?;
    let destination = root.resolve(relative_path);

    let provider = create_provider(app_data_dir, &library)?;

    // Fast-path: if the destination exists AND the provider revision is
    // unchanged AND the size matches, skip re-download. This is a minimal
    // cache-validity check; the full verified cache catalog is PR #6.
    // TODO(PR#6): replace existence+revision check with verified cache
    // catalog lookup.
    if destination.exists() {
        let stored_revision = provider.get_revision(relative_path)?;
        let local_size = std::fs::metadata(&destination).map(|m| m.len()).ok();
        let remote_size = provider.get_file_size(relative_path)?;
        if stored_revision.as_deref() == library.remote_revision()
            && local_size.is_some()
            && remote_size == local_size
        {
            return Ok(());
        }
    }

    // Download to a temp file, validate size when known, then atomically
    // rename. This replaces the old direct-to-destination download that
    // could leave a truncated file at the final path (defect #5).
    let expected_size = provider.get_file_size(relative_path)?;
    let operation_id = format!("cache-{}", current_unix_time_ms());
    crate::remote::atomic_download::atomic_download(
        provider.as_ref(),
        crate::remote::atomic_download::AtomicDownloadOptions {
            relative_path,
            destination: &destination,
            expected_size,
            expected_digest: None,
            operation_id: &operation_id,
        },
    )
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
        let control_db_conn = state.remote.control_db.lock().map_err(|_| {
            crate::commands::error::state_lock_error("control DB lock was poisoned")
        })?;
        // Dirty working-copy protection: only pull when the working copy is
        // clean. A dirty/publishing/conflicted/reauth-required copy holds
        // committed local edits that must not be overwritten by a refresh.
        if !should_allow_automatic_pull(&control_db_conn, active_library) {
            tracing::info!(
                "skipping remote database refresh for library {} because the \
                 working copy is not clean",
                active_library.id()
            );
            return Ok(());
        }
        let _ = sync_remote_database_from_provider(
            &control_db_conn,
            &state.shell.app_data_dir,
            active_library,
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RemoteLibraryProvider;
    use crate::remote::control_db::{
        open_control_db, upsert_repository_state, LocalState, RepositoryStateRow,
    };
    use tempfile::TempDir;

    fn make_remote_library(id: &str) -> RegisteredLibrary {
        RegisteredLibrary::remote(
            id.to_owned(),
            "Test".to_owned(),
            RemoteLibraryProvider::WebDav,
            "account-1".to_owned(),
            "https://example.com/dav".to_owned(),
            "/dav".to_owned(),
            None,
            None,
            None,
        )
    }

    fn fresh_control_db() -> (TempDir, Connection) {
        let dir = TempDir::new().expect("temp dir");
        let conn = open_control_db(&dir.path().join("remote-state.db")).expect("open control db");
        (dir, conn)
    }

    fn make_repo_state(library_id: &str, state: LocalState) -> RepositoryStateRow {
        RepositoryStateRow {
            library_id: library_id.to_owned(),
            committed_generation: 0,
            committed_manifest_revision: None,
            local_base_generation: 0,
            local_db_digest: None,
            local_state: state,
            active_operation_id: None,
            last_success_at_ms: None,
            last_error_code: None,
            updated_at_ms: 1000,
        }
    }

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

    // ---- Dirty working-copy protection tests ----

    #[test]
    fn should_allow_automatic_pull_when_clean() {
        let (_dir, conn) = fresh_control_db();
        let library = make_remote_library("lib-1");
        upsert_repository_state(&conn, &make_repo_state("lib-1", LocalState::Clean)).unwrap();

        assert!(should_allow_automatic_pull(&conn, &library));
    }

    #[test]
    fn should_block_automatic_pull_when_dirty() {
        let (_dir, conn) = fresh_control_db();
        let library = make_remote_library("lib-1");
        upsert_repository_state(&conn, &make_repo_state("lib-1", LocalState::Dirty)).unwrap();

        assert!(
            !should_allow_automatic_pull(&conn, &library),
            "dirty working copy must not be overwritten"
        );
    }

    #[test]
    fn should_block_automatic_pull_when_publishing() {
        let (_dir, conn) = fresh_control_db();
        let library = make_remote_library("lib-1");
        upsert_repository_state(&conn, &make_repo_state("lib-1", LocalState::Publishing)).unwrap();

        assert!(
            !should_allow_automatic_pull(&conn, &library),
            "publishing working copy must not be overwritten"
        );
    }

    #[test]
    fn should_block_automatic_pull_when_conflicted() {
        let (_dir, conn) = fresh_control_db();
        let library = make_remote_library("lib-1");
        upsert_repository_state(&conn, &make_repo_state("lib-1", LocalState::Conflicted)).unwrap();

        assert!(
            !should_allow_automatic_pull(&conn, &library),
            "conflicted working copy must not be overwritten"
        );
    }

    #[test]
    fn should_block_automatic_pull_when_reauth_required() {
        let (_dir, conn) = fresh_control_db();
        let library = make_remote_library("lib-1");
        upsert_repository_state(&conn, &make_repo_state("lib-1", LocalState::ReauthRequired))
            .unwrap();

        assert!(
            !should_allow_automatic_pull(&conn, &library),
            "reauth-required working copy must not be overwritten"
        );
    }

    #[test]
    fn should_allow_automatic_pull_when_no_state_row() {
        let (_dir, conn) = fresh_control_db();
        let library = make_remote_library("lib-1");

        // No state row — allow so first-time refresh works.
        assert!(should_allow_automatic_pull(&conn, &library));
    }
}
