use crate::{
    commands::error::{CommandError, CommandResult},
    config::{AppConfig, RegisteredLibrary, RemoteLibraryProvider},
    library::error::LibraryError,
    remote::atomic_download::{
        atomic_database_pull, update_repository_state_after_pull, DatabasePullOptions,
    },
    remote::control_db::{
        delete_transfer_parts, get_repository_state, upsert_transfer_part, LocalState,
        TransferDirection, TransferPartRow,
    },
    remote::manifest::{read_manifest, MANIFEST_PATH},
};
use openkara_remote::RepositoryStorage as CoreStorage;
use rusqlite::Connection;
use std::path::Path;

use super::super::types::{
    current_unix_time_ms, load_app_config, load_remote_root, persist_app_config,
};

use super::super::provider::{create_repository_storage, RepositoryStorage};

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

/// Refresh the Local Working Copy of a WebDAV repository through the core
/// reader: probe the revision, plan against the manifest (or the legacy
/// `openkara.db`), then run the resumable snapshot download with the
/// clean/dirty policy applied by the app.
///
/// The part file's durable progress is mirrored into `remote_transfer_parts`
/// like the other providers' path, so a restart resumes the transfer. A
/// failed pull leaves the current working copy in place and returns the
/// library unchanged, so the caller can proceed offline.
pub(in crate::remote) fn refresh_working_copy_via_core(
    control_db_conn: &Connection,
    app_data_dir: &Path,
    library: &RegisteredLibrary,
) -> CommandResult<RegisteredLibrary> {
    let secret = crate::remote::webdav::load_webdav_secret(app_data_dir, library)?;
    let storage =
        openkara_remote::WebDavStorage::new(&secret.root_url, secret.username, secret.password)
            .map_err(CommandError::from)?;
    let root = load_remote_root(app_data_dir, library)?;

    let remote_revision =
        openkara_remote::probe_remote_revision(&storage).map_err(CommandError::from)?;
    if !openkara_remote::remote_revision_is_stale(
        library.remote_revision(),
        remote_revision.as_deref(),
    ) {
        retain_clean_base_if_missing(control_db_conn, library.id(), &root);
        return Ok(library.clone());
    }
    let plan = openkara_remote::plan_refresh(&storage).map_err(CommandError::from)?;

    let operation_id = openkara_remote::snapshot_operation_id(plan.remote_revision.as_deref());
    let working_copy = openkara_remote::WorkingCopyDatabase::new(root.database_path());
    let policy = CleanWorkingCopyPolicy {
        control_db_conn,
        library,
    };
    let mut record_progress = |transferred: u64| {
        let _ = upsert_transfer_part(
            control_db_conn,
            &TransferPartRow {
                operation_id: operation_id.clone(),
                relative_path: plan.database.remote_path.clone(),
                direction: TransferDirection::Download,
                expected_size: plan.database.expected_size.map(|size| size as i64),
                expected_digest: plan.database.expected_digest.clone(),
                provider_revision: None,
                provider_session_id: None,
                transferred_bytes: transferred as i64,
                state: "in_progress".to_owned(),
                updated_at_ms: current_unix_time_ms(),
            },
        );
    };

    match openkara_remote::pull_database_snapshot(
        &storage,
        &plan,
        &working_copy,
        &operation_id,
        &policy,
        &mut record_progress,
    ) {
        Ok(None) => Ok(library.clone()),
        Ok(Some(pulled)) => {
            let _ = delete_transfer_parts(control_db_conn, &operation_id);
            if let Err(error) = update_repository_state_after_pull(
                control_db_conn,
                library.id(),
                &pulled.database_sha256,
                pulled.generation,
            ) {
                tracing::info!(
                    "failed to persist repository state after refreshing library {}: {:?}",
                    library.id(),
                    error
                );
                return Ok(library.clone());
            }
            // The pulled snapshot is the base a later publication rebases from.
            if let Err(error) = openkara_remote::retain_base_snapshot(
                root.root(),
                pulled.generation,
                &root.database_path(),
            ) {
                tracing::info!(
                    "failed to retain the base snapshot of library {}: {:?}",
                    library.id(),
                    error
                );
            }
            let new_revision = if plan.manifest.is_some() {
                storage
                    .stat(MANIFEST_PATH)
                    .map_err(CommandError::from)?
                    .and_then(|metadata| metadata.revision)
            } else {
                plan.remote_revision.clone()
            };
            update_remote_revision_in_config(app_data_dir, library.id(), new_revision)?;
            load_registered_remote_library(app_data_dir, library.id())
        }
        Err(error) => {
            // Fall back to the last verified working copy; do not block the mutation.
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

/// A clean working copy still holds exactly its base generation, so it can
/// serve as the base snapshot when none was retained (working copies created
/// before bases were kept).
fn retain_clean_base_if_missing(
    control_db_conn: &Connection,
    library_id: &str,
    root: &crate::library_root::LibraryRoot,
) {
    let Ok(Some(state)) = get_repository_state(control_db_conn, library_id) else {
        return;
    };
    if state.local_state != LocalState::Clean
        || openkara_remote::base_snapshot_path(root.root(), state.local_base_generation).is_file()
    {
        return;
    }
    let database = root.database_path();
    let digest = crate::remote::atomic_download::sha256_file(&database).ok();
    if digest.is_none() || digest != state.local_db_digest {
        return;
    }
    if let Err(error) =
        openkara_remote::retain_base_snapshot(root.root(), state.local_base_generation, &database)
    {
        tracing::info!("failed to retain the base snapshot of library {library_id}: {error:?}");
    }
}

/// Applies the app's clean/dirty policy to the core reader's policy hook.
struct CleanWorkingCopyPolicy<'a> {
    control_db_conn: &'a Connection,
    library: &'a RegisteredLibrary,
}

impl openkara_remote::RefreshPolicy for CleanWorkingCopyPolicy<'_> {
    fn may_replace_working_copy(&self) -> bool {
        should_allow_automatic_pull(self.control_db_conn, self.library)
    }
}

pub(in crate::remote) fn remote_database_revision(
    provider: &dyn RepositoryStorage,
) -> CommandResult<Option<String>> {
    // Manifest revision is the staleness signal; legacy falls back to openkara.db.
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

pub(crate) fn prepare_remote_database_for_mutation(
    control_db_conn: &Connection,
    app_data_dir: &Path,
    library: &RegisteredLibrary,
) -> CommandResult<RegisteredLibrary> {
    // Dirty/publishing/conflicted/reauth working copies must not be auto-pulled over.
    if !should_allow_automatic_pull(control_db_conn, library) {
        tracing::info!(
            "skipping automatic remote database pull for library {} because \
             the working copy is not clean",
            library.id()
        );
        return Ok(library.clone());
    }

    if matches!(library.provider(), Some(RemoteLibraryProvider::WebDav)) {
        return refresh_working_copy_via_core(control_db_conn, app_data_dir, library);
    }

    let provider = create_repository_storage(app_data_dir, library)?;
    let provider_revision = remote_database_revision(provider.as_ref())?;
    if remote_database_revision_is_stale(library.remote_revision(), provider_revision.as_deref()) {
        return pull_remote_database_atomically(
            provider.as_ref(),
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
pub(in crate::remote) fn should_allow_automatic_pull(
    control_db_conn: &Connection,
    library: &RegisteredLibrary,
) -> bool {
    match get_repository_state(control_db_conn, library.id()) {
        Ok(Some(state)) => matches!(state.local_state, LocalState::Clean),
        Ok(None) => true,
        // Fail closed when control DB is unreadable.
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
pub(in crate::remote) fn pull_remote_database_atomically(
    provider: &dyn RepositoryStorage,
    control_db_conn: &Connection,
    app_data_dir: &Path,
    library: &RegisteredLibrary,
    provider_revision: Option<&str>,
) -> CommandResult<RegisteredLibrary> {
    let root = load_remote_root(app_data_dir, library)?;

    // Sanitize revision for temp filenames (ETags may include quotes/slashes).
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

    let manifest = read_manifest(provider)?;

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
        provider,
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
            let new_revision = if manifest.is_some() {
                provider.get_revision(MANIFEST_PATH)?
            } else {
                provider_revision.map(|s| s.to_owned())
            };
            update_remote_revision_in_config(app_data_dir, library.id(), new_revision)?;
            load_registered_remote_library(app_data_dir, library.id())
        }
        Err(error) => {
            // Fall back to last verified working copy; do not block the mutation.
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
    if matches!(library.provider(), Some(RemoteLibraryProvider::WebDav)) {
        return ensure_remote_file_cached_via_core(app_data_dir, &library, relative_path);
    }
    let root = load_remote_root(app_data_dir, &library)?;
    let destination = root.resolve(relative_path);

    let provider = create_repository_storage(app_data_dir, &library)?;

    // Identity-keyed cache catalog (library_id, path, revision, size) — defect #7.
    let provider_revision = provider.get_revision(relative_path)?;
    let remote_size = provider.media_source().get_file_size(relative_path)?;
    let revision = provider_revision
        .clone()
        .or_else(|| library.remote_revision().map(str::to_owned));

    if cached_asset_copy_is_current(
        app_data_dir,
        &library,
        relative_path,
        remote_size,
        revision.as_deref(),
        &destination,
    ) {
        return Ok(());
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

/// Fetch one asset of a WebDAV repository into the Local Working Copy
/// through the core: reuse a copy the cache catalog or the size check
/// already proves current, otherwise download with range resume and keep
/// only a copy verified against the digest the content-addressed path
/// names.
pub(in crate::remote) fn ensure_remote_file_cached_via_core(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
    relative_path: &str,
) -> CommandResult<()> {
    let root = load_remote_root(app_data_dir, library)?;
    let destination = root.resolve(relative_path);
    let secret = crate::remote::webdav::load_webdav_secret(app_data_dir, library)?;
    let storage =
        openkara_remote::WebDavStorage::new(&secret.root_url, secret.username, secret.password)
            .map_err(CommandError::from)?;
    let source =
        openkara_remote::plan_asset_fetch(&storage, relative_path).map_err(CommandError::from)?;

    let revision = source
        .revision
        .clone()
        .or_else(|| library.remote_revision().map(str::to_owned));
    if cached_asset_copy_is_current(
        app_data_dir,
        library,
        relative_path,
        source.expected_size,
        revision.as_deref(),
        &destination,
    ) {
        return Ok(());
    }

    let operation_id =
        openkara_remote::asset_operation_id(relative_path, source.revision.as_deref());
    openkara_remote::fetch_asset(&storage, &source, &destination, &operation_id, &mut |_| {})
        .map_err(CommandError::from)?;
    Ok(())
}

/// The asset at `destination` is current when a complete identity-keyed
/// cache catalog row covers it or its size matches the remote size.
fn cached_asset_copy_is_current(
    app_data_dir: &Path,
    library: &RegisteredLibrary,
    relative_path: &str,
    remote_size: Option<u64>,
    revision: Option<&str>,
    destination: &Path,
) -> bool {
    let control_db_path = crate::remote::control_db::control_db_path(app_data_dir);
    if let Ok(conn) = crate::remote::control_db::open_control_db(&control_db_path) {
        if let (Some(size), Some(rev)) = (remote_size, revision) {
            let identity = openkara_remote::media_cache::CacheIdentity {
                library_id: library.id().to_owned(),
                relative_path: relative_path.to_owned(),
                provider_revision: Some(rev.to_owned()),
                expected_size: size,
            };
            let cache_key = identity.cache_key();
            if let Ok(Some(row)) = openkara_remote::media_cache::read_entry(&conn, &cache_key) {
                if row.complete
                    && row.expected_size == size as i64
                    && std::path::Path::new(&row.data_path).is_absolute()
                    && std::fs::metadata(&row.data_path).is_ok()
                {
                    return true;
                }
                // Working-copy destination: reuse when size matches a complete catalog row.
                if row.complete
                    && row.expected_size == size as i64
                    && destination.exists()
                    && std::fs::metadata(destination).map(|m| m.len()).ok() == Some(size)
                {
                    return true;
                }
            }
        }
    }

    // Size match at destination skips re-download (atomic_download does not catalog rows).
    if let Some(size) = remote_size {
        if destination.exists()
            && std::fs::metadata(destination).map(|m| m.len()).ok() == Some(size)
        {
            return true;
        }
    }
    false
}

pub(crate) fn resolve_active_remote(config: &AppConfig) -> Option<RegisteredLibrary> {
    config.active_library().and_then(|library| match library {
        RegisteredLibrary::Remote { .. } => Some(library.clone()),
        RegisteredLibrary::Local { .. } => None,
    })
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
