use crate::{
    commands::error::{CommandError, CommandResult},
    config::{AppConfig, RegisteredLibrary},
    library::error::LibraryError,
    remote::atomic_download::{
        atomic_database_pull, reconcile_database_state_after_restart, DatabasePullOptions,
    },
    remote::control_db::{get_repository_state, LocalState},
    remote::manifest::{read_manifest, MANIFEST_PATH},
    AppState,
};
use rusqlite::Connection;
use std::path::Path;

use super::super::types::{
    current_unix_time_ms, load_app_config, load_remote_root, persist_app_config,
};

use super::super::provider::create_repository_storage;

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
    let provider = create_repository_storage(app_data_dir, library)?;
    // For manifest-based repositories, the manifest revision is the staleness
    // signal: a new generation always produces a new manifest write. For
    // legacy repositories (no manifest), fall back to the `openkara.db`
    // revision.
    let manifest_rev = provider.get_revision(MANIFEST_PATH)?;
    if manifest_rev.is_some() {
        return Ok(manifest_rev);
    }
    provider.get_revision("openkara.db")
}

pub(crate) fn remote_database_revision_is_stale(
    stored_revision: Option<&str>,
    provider_revision: Option<&str>,
) -> bool {
    provider_revision.is_some_and(|revision| Some(revision) != stored_revision)
}

#[cfg(test)]
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

    // Unified read protocol: resolve the committed database through the
    // manifest (or the legacy root openkara.db when no manifest exists).
    // Do NOT call provider.initialize_or_sync() here — that bootstrap path
    // always downloads root openkara.db and would silently replace a
    // generation-specific working copy with a stale legacy object after
    // the repository has been migrated to the manifest protocol.
    let provider_revision = remote_database_revision(app_data_dir, library)?;
    if !remote_database_revision_is_stale(library.remote_revision(), provider_revision.as_deref()) {
        return Ok(library.clone());
    }
    pull_remote_database_atomically(
        control_db_conn,
        app_data_dir,
        library,
        provider_revision.as_deref(),
    )
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
        // Preserve the current working copy. The publication executor can
        // resume the pending operation without an automatic pull overwriting it.
        tracing::info!(
            "skipping automatic remote database pull for library {} because \
             the working copy is not clean",
            library.id()
        );
        return Ok(library.clone());
    }

    let provider_revision = remote_database_revision(app_data_dir, library)?;
    if remote_database_revision_is_stale(library.remote_revision(), provider_revision.as_deref()) {
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
///
/// If the control DB is unreadable, fail closed: do NOT allow an automatic
/// pull. The atomic pull validates the remote candidate, but that does not
/// prove the local database has no unpublished edits. Overwriting a dirty
/// working copy when the control plane is unavailable would silently lose
/// local mutations.
fn should_allow_automatic_pull(control_db_conn: &Connection, library: &RegisteredLibrary) -> bool {
    match get_repository_state(control_db_conn, library.id()) {
        Ok(Some(state)) => matches!(state.local_state, LocalState::Clean),
        Ok(None) => true,
        // If the control DB is unreadable, fail closed. Do not allow an
        // automatic pull over the working copy — the local database may
        // contain unpublished edits that the control plane cannot verify.
        Err(_) => false,
    }
}

/// Atomically pull and validate the remote database into the working copy.
///
/// Reads the repository manifest (`.openkara-repository.json`) to discover the
/// committed generation, then downloads the generation-specific database from
/// `.openkara/databases/<generation>.sqlite` with size and SHA-256 verification
/// against the manifest's `database_size_bytes` and `database_sha256`. On success,
/// updates the stored revision and `committed_generation` in the control DB.
///
/// For legacy repositories (no manifest yet), falls back to pulling
/// `openkara.db` directly with size-only verification.
///
/// On failure, falls back to the existing local DB (returns the library
/// unchanged) so the caller can proceed offline.
fn pull_remote_database_atomically(
    control_db_conn: &Connection,
    app_data_dir: &Path,
    library: &RegisteredLibrary,
    provider_revision: Option<&str>,
) -> CommandResult<RegisteredLibrary> {
    let provider = create_repository_storage(app_data_dir, library)?;
    let root = load_remote_root(app_data_dir, library)?;

    // Sanitize the provider revision before embedding it in the operation
    // id (which becomes part of a temp filename). WebDAV ETags can contain
    // quotes ("abc123") and weak prefixes (W/"abc123"); other providers may
    // return revision strings with slashes. Replace any character outside
    // [A-Za-z0-9._-] with an underscore so the temp filename is always
    // valid across platforms.
    let sanitized_revision: String = provider_revision
        .unwrap_or("unknown")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let operation_id = format!("pull-{sanitized_revision}");

    // Read the repository manifest. When present, pull the generation-specific
    // database with full size + digest verification. When absent (legacy
    // repository or first publication), fall back to pulling `openkara.db`
    // directly with size-only verification.
    let manifest = read_manifest(provider.as_ref())?;

    let (remote_db_path, expected_size, expected_digest, committed_generation) = match &manifest {
        Some(m) => {
            let size = provider.media_source().get_file_size(&m.database_path)?;
            (
                m.database_path.as_str(),
                size.or(Some(m.database_size_bytes)),
                Some(m.database_sha256.as_str()),
                m.generation,
            )
        }
        None => {
            let size = provider.media_source().get_file_size("openkara.db")?;
            ("openkara.db", size, None, 0)
        }
    };

    match atomic_database_pull(
        provider.as_ref(),
        control_db_conn,
        &root,
        DatabasePullOptions {
            operation_id: &operation_id,
            expected_size,
            expected_digest,
            library_id: library.id(),
            remote_database_path: remote_db_path,
            committed_generation,
        },
    ) {
        Ok(_) => {
            // Use the manifest path's revision when available, falling back
            // to the legacy `openkara.db` revision for pre-manifest repos.
            let new_revision = if manifest.is_some() {
                provider.get_revision(MANIFEST_PATH)?
            } else {
                provider_revision.map(|s| s.to_owned())
            };
            update_remote_revision_in_config(app_data_dir, library.id(), new_revision)?;
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

pub fn ensure_remote_file_cached(app_data_dir: &Path, relative_path: &str) -> CommandResult<()> {
    let Some(library) = active_remote_library(app_data_dir)? else {
        return Ok(());
    };
    let root = load_remote_root(app_data_dir, &library)?;
    let destination = root.resolve(relative_path);

    let provider = create_repository_storage(app_data_dir, &library)?;

    // Verified cache catalog lookup. The cache key is derived from the
    // identity tuple (library_id, relative_path, provider_revision,
    // expected_size). A complete, verified catalog entry means the file is
    // already cached and reusable; otherwise fall through to an atomic
    // download. This replaces the old existence+revision+size check that could
    // reuse bytes from an older provider revision (defect #7).
    let provider_revision = provider.get_revision(relative_path)?;
    let remote_size = provider.media_source().get_file_size(relative_path)?;
    let revision = provider_revision
        .clone()
        .or_else(|| library.remote_revision().map(str::to_owned));

    // Open the control DB to consult the cache catalog. This is the same DB
    // the AppState holds; opening a short-lived read connection here is safe
    // because WAL mode allows concurrent readers.
    let control_db_path = crate::remote::control_db::control_db_path(app_data_dir);
    if let Ok(conn) = crate::remote::control_db::open_control_db(&control_db_path) {
        if let (Some(size), Some(rev)) = (remote_size, revision.as_deref()) {
            let identity = crate::remote::cache_catalog::CacheIdentity {
                library_id: library.id().to_owned(),
                relative_path: relative_path.to_owned(),
                provider_revision: Some(rev.to_owned()),
                expected_size: size,
            };
            let cache_key = identity.cache_key();
            if let Ok(Some(row)) = crate::remote::control_db::get_cache_entry(&conn, &cache_key) {
                if row.complete
                    && row.expected_size == size as i64
                    && std::path::Path::new(&row.data_path).is_absolute()
                    && std::fs::metadata(&row.data_path).is_ok()
                {
                    return Ok(());
                }
                // For the working-copy destination path (not the streaming
                // cache), check the destination directly when the catalog row
                // is complete and the data file matches.
                if row.complete
                    && row.expected_size == size as i64
                    && destination.exists()
                    && std::fs::metadata(&destination).map(|m| m.len()).ok() == Some(size)
                {
                    return Ok(());
                }
            }
        }
    }

    // Fallback fast-path: if the destination file already exists and its
    // size matches the remote size, skip the download. The catalog lookup
    // above only succeeds when a `remote_cache_entries` row exists, but
    // `atomic_download` does not create catalog rows — it writes directly
    // to the destination. Without this fallback, non-streaming remote media,
    // CDG graphics, and imported remote files are re-downloaded on every
    // access even when a valid copy is already present.
    if let Some(size) = remote_size {
        if destination.exists()
            && std::fs::metadata(&destination).map(|m| m.len()).ok() == Some(size)
        {
            return Ok(());
        }
    }

    let expected_size = remote_size;
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

pub(crate) fn refresh_remote_repository(state: &AppState) -> CommandResult<()> {
    state.remote.ensure_available()?;

    let config = load_app_config(&state.shell.app_data_dir)?;
    let Some(active_library) = config.active_library() else {
        return Err(CommandError::from(LibraryError::Internal(
            "no library is currently active".to_string(),
        )));
    };

    if matches!(active_library, RegisteredLibrary::Remote { .. }) {
        let control_db_conn = state.remote.control_db()?.lock().map_err(|_| {
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
            repository_id: None,
            writer_id: None,
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

        assert!(should_allow_automatic_pull(&conn, &library));
    }
}
