use crate::{
    commands::{
        error::{internal_error, state_lock_error, CommandError, CommandResult},
        library_setup::LibraryRegistrySnapshot,
    },
    config::{RegisteredLibrary, RemoteLibraryConnectionConfig, RemoteLibraryProvider},
    library::error::LibraryError,
    library_root::LibraryRoot,
    AppState,
};
use rusqlite::Connection;
use std::path::Path;

use super::{
    atomic_download::reconcile_database_state_after_restart,
    auth_binding::BindContext,
    provider::{compute_remote_path_display, create_repository_storage, RepositoryStorage},
    registry::{mark_session_binding, ready_session_binding},
    sync::{
        pull_remote_database_atomically, remote_database_revision,
        remote_database_revision_is_stale, should_allow_automatic_pull,
    },
    types::{current_unix_time_ms, load_app_config, persist_app_config, ProviderSessionData},
};

/// Everything the Remote Repository access actions need from the Remote
/// Provider. Both entries are bound to one registered repository: credentials
/// must be stored before storage can be opened for it.
pub(crate) trait RepositoryAccess {
    fn bind_credentials(
        &self,
        session: &ProviderSessionData,
        app_data_dir: &Path,
        library_id: &str,
        context: BindContext,
    ) -> CommandResult<RemoteLibraryConnectionConfig>;

    fn open_storage<'a>(
        &self,
        app_data_dir: &'a Path,
        library: &'a RegisteredLibrary,
    ) -> CommandResult<Box<dyn RepositoryStorage + 'a>>;
}

pub(crate) struct ProviderRepositoryAccess;

impl RepositoryAccess for ProviderRepositoryAccess {
    fn bind_credentials(
        &self,
        session: &ProviderSessionData,
        app_data_dir: &Path,
        library_id: &str,
        context: BindContext,
    ) -> CommandResult<RemoteLibraryConnectionConfig> {
        session.bind_repository_credentials(app_data_dir, library_id, context)
    }

    fn open_storage<'a>(
        &self,
        app_data_dir: &'a Path,
        library: &'a RegisteredLibrary,
    ) -> CommandResult<Box<dyn RepositoryStorage + 'a>> {
        create_repository_storage(app_data_dir, library)
    }
}

static PROVIDER_REPOSITORY_ACCESS: ProviderRepositoryAccess = ProviderRepositoryAccess;

enum RepositoryRecoveryAction {
    Reauthorize,
    Relocate,
}

/// Owns the access and recovery actions of a registered Remote Repository:
/// Reauthorize Repository, Relocate Repository, and Refresh Repository.
///
/// Guarantees every caller may rely on:
///
/// - Reauthorize Repository renews Repository Credentials against the
///   registered Remote Repository Location and rejects any other location.
/// - Relocate Repository requires a different Remote Repository Location, keeps
///   the existing Local Working Copy directory, only accepts a location that
///   already holds an OpenKara repository (the new location is opened, never
///   created), performs Refresh Repository against it immediately, and records
///   the resulting Remote Revision.
/// - Both recovery actions require a Ready auth session whose Remote Provider —
///   and, except for WebDAV, whose account — matches the registered repository,
///   and both leave the repository active with its working copy reopened.
/// - Refresh Repository is a no-op unless a Remote Repository is active, and it
///   never overwrites a Local Working Copy that is not clean.
pub(crate) struct RemoteRepositoryLifecycle<'a> {
    state: &'a AppState,
    app_data_dir: &'a Path,
    access: &'a dyn RepositoryAccess,
}

impl<'a> RemoteRepositoryLifecycle<'a> {
    pub(crate) fn new(state: &'a AppState, app_data_dir: &'a Path) -> Self {
        Self::with_access(state, app_data_dir, &PROVIDER_REPOSITORY_ACCESS)
    }

    pub(crate) fn with_access(
        state: &'a AppState,
        app_data_dir: &'a Path,
        access: &'a dyn RepositoryAccess,
    ) -> Self {
        Self {
            state,
            app_data_dir,
            access,
        }
    }

    pub(crate) fn reauthorize(
        &self,
        library_id: String,
        session_id: String,
        remote_root_locator: String,
        display_name: String,
    ) -> CommandResult<LibraryRegistrySnapshot> {
        self.recover(
            RepositoryRecoveryAction::Reauthorize,
            library_id,
            session_id,
            remote_root_locator,
            display_name,
        )
    }

    pub(crate) fn relocate(
        &self,
        library_id: String,
        session_id: String,
        remote_root_locator: String,
        display_name: String,
    ) -> CommandResult<LibraryRegistrySnapshot> {
        self.recover(
            RepositoryRecoveryAction::Relocate,
            library_id,
            session_id,
            remote_root_locator,
            display_name,
        )
    }

    pub(crate) fn refresh(&self) -> CommandResult<()> {
        self.state.remote.ensure_available()?;

        let config = load_app_config(self.app_data_dir)?;
        let Some(active_library) = config.active_library() else {
            return Err(CommandError::from(LibraryError::Internal(
                "no library is currently active".to_string(),
            )));
        };
        if !matches!(active_library, RegisteredLibrary::Remote { .. }) {
            return Ok(());
        }

        let control_db_conn = self
            .state
            .remote
            .control_db()?
            .lock()
            .map_err(|_| state_lock_error("control DB lock was poisoned"))?;
        if !should_allow_automatic_pull(&control_db_conn, active_library) {
            tracing::info!(
                "skipping remote database refresh for library {} because the \
                 working copy is not clean",
                active_library.id()
            );
            return Ok(());
        }
        self.pull_working_copy_from_remote(&control_db_conn, active_library)
    }

    fn recover(
        &self,
        action: RepositoryRecoveryAction,
        library_id: String,
        session_id: String,
        remote_root_locator: String,
        display_name: String,
    ) -> CommandResult<LibraryRegistrySnapshot> {
        let config = load_app_config(self.app_data_dir)?;
        let existing = config
            .libraries
            .iter()
            .find(|entry| entry.id() == library_id)
            .cloned()
            .ok_or_else(|| {
                CommandError::from(LibraryError::Internal(format!(
                    "remote repository {library_id} was not found"
                )))
            })?;
        if !matches!(existing, RegisteredLibrary::Remote { .. }) {
            return Err(CommandError::from(LibraryError::Internal(format!(
                "repository {library_id} is not a remote repository"
            ))));
        }

        let (account_id, provider, provider_session) =
            ready_session_binding(self.state, &session_id)?;

        validate_recovery_request(
            &existing,
            provider,
            &account_id,
            &remote_root_locator,
            &action,
        )?;

        let root_path = existing.working_copy_root().ok_or_else(|| {
            CommandError::from(LibraryError::Internal(
                "remote repository is missing a local working copy".to_string(),
            ))
        })?;
        let connection_config = self.access.bind_credentials(
            &provider_session,
            self.app_data_dir,
            &library_id,
            BindContext::Reauthorize,
        )?;
        let remote_path_display =
            compute_remote_path_display(provider, &remote_root_locator, &display_name);

        let provisional_library = RegisteredLibrary::remote(
            library_id.clone(),
            display_name.clone(),
            provider,
            account_id.clone(),
            remote_root_locator.clone(),
            remote_path_display.clone(),
            Some(connection_config.clone()),
            Some(root_path.join("openkara.db").display().to_string()),
            existing.remote_revision().map(str::to_owned),
        );
        let remote_revision = {
            let storage = self
                .access
                .open_storage(self.app_data_dir, &provisional_library)?;
            // Open the existing root only; neither action may create layout.
            storage.refresh_existing()?
        };

        mark_session_binding(self.state, &session_id, &remote_root_locator, &display_name)?;

        let mut config = load_app_config(self.app_data_dir)?;
        let updated_library = RegisteredLibrary::remote(
            library_id.clone(),
            display_name,
            provider,
            account_id,
            remote_root_locator,
            remote_path_display,
            Some(connection_config),
            Some(root_path.join("openkara.db").display().to_string()),
            remote_revision.or_else(|| Some(current_unix_time_ms().to_string())),
        );
        if let Some(existing_entry) = config
            .libraries
            .iter_mut()
            .find(|entry| entry.id() == library_id)
        {
            *existing_entry = updated_library;
        }
        config.active_library_id = Some(library_id);
        persist_app_config(self.app_data_dir, &config)?;

        let mut guard = self
            .state
            .shell
            .library
            .lock()
            .map_err(|_| state_lock_error("library lock was poisoned"))?;
        *guard = Some(LibraryRoot::open(&root_path).map_err(internal_error)?);

        Ok(LibraryRegistrySnapshot {
            active_library_id: config.active_library_id.clone(),
            libraries: config.libraries.clone(),
        })
    }

    fn pull_working_copy_from_remote(
        &self,
        control_db_conn: &Connection,
        library: &RegisteredLibrary,
    ) -> CommandResult<()> {
        reconcile_working_copy_after_restart(control_db_conn, library)?;

        // Pull via manifest (or legacy root DB). Do not call initialize_or_sync —
        // it always downloads root openkara.db and can clobber a generation
        // working copy.
        let storage = self.access.open_storage(self.app_data_dir, library)?;
        let provider_revision = remote_database_revision(storage.as_ref())?;
        if !remote_database_revision_is_stale(
            library.remote_revision(),
            provider_revision.as_deref(),
        ) {
            return Ok(());
        }
        pull_remote_database_atomically(
            storage.as_ref(),
            control_db_conn,
            self.app_data_dir,
            library,
            provider_revision.as_deref(),
        )
        .map(|_| ())
    }
}

fn validate_recovery_request(
    existing: &RegisteredLibrary,
    provider: RemoteLibraryProvider,
    account_id: &str,
    remote_root_locator: &str,
    action: &RepositoryRecoveryAction,
) -> CommandResult<bool> {
    if existing.provider() != Some(provider) {
        return Err(CommandError::from(LibraryError::Internal(
            "reauthorization provider does not match the remote repository".to_owned(),
        )));
    }
    if provider != RemoteLibraryProvider::WebDav && existing.account_id() != Some(account_id) {
        return Err(CommandError::from(LibraryError::Internal(
            "reauthorization account does not match the remote repository".to_owned(),
        )));
    }

    let is_relocation = existing.remote_root_locator() != Some(remote_root_locator);
    match action {
        RepositoryRecoveryAction::Reauthorize if is_relocation => {
            Err(CommandError::from(LibraryError::Internal(
                "Reauthorize Repository cannot change the Remote Repository Location.".to_owned(),
            )))
        }
        RepositoryRecoveryAction::Relocate if !is_relocation => {
            Err(CommandError::from(LibraryError::Internal(
                "Relocate Repository requires a different Remote Repository Location.".to_owned(),
            )))
        }
        _ => Ok(is_relocation),
    }
}

/// Reconcile repository state if a prior pull completed the rename but not the
/// control-DB state update. This runs before every refresh so a crash between
/// rename and state-update does not leave the control DB describing a stale
/// digest.
fn reconcile_working_copy_after_restart(
    control_db_conn: &Connection,
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
    let root = LibraryRoot::open(&root_path).map_err(internal_error)?;
    if root.database_path().exists() {
        let _ = reconcile_database_state_after_restart(control_db_conn, &root, library.id());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cache,
        config::AppConfig,
        remote::{
            control_db::{upsert_repository_state, LocalState, RepositoryStateRow},
            provider_conformance::ScriptedProvider,
            types::{RemoteAuthSession, RemoteAuthState, WebDavSessionData},
        },
    };
    use std::{path::PathBuf, sync::Mutex};
    use tempfile::TempDir;

    const LIBRARY_ID: &str = "remote-repository-1";
    const SESSION_ID: &str = "remote-auth-session-1";
    const ACCOUNT_ID: &str = "account-1";
    const SERVER_URL: &str = "https://example.com/dav";
    const ORIGINAL_LOCATION: &str = "https://example.com/dav/original/";
    const NEW_LOCATION: &str = "https://example.com/dav/moved/";

    /// Test adapter for the Remote Provider seam. Records the Remote Repository
    /// Location every time the lifecycle opens storage, so a test can prove
    /// which location an action reached.
    struct ScriptedAccess {
        provider: ScriptedProvider,
        opened_locations: Mutex<Vec<String>>,
    }

    impl ScriptedAccess {
        fn new(root: PathBuf) -> Self {
            Self {
                provider: ScriptedProvider::new(root),
                opened_locations: Mutex::new(Vec::new()),
            }
        }

        fn opened_locations(&self) -> Vec<String> {
            self.opened_locations
                .lock()
                .expect("scripted access lock")
                .clone()
        }
    }

    impl RepositoryAccess for ScriptedAccess {
        fn bind_credentials(
            &self,
            _session: &ProviderSessionData,
            _app_data_dir: &Path,
            _library_id: &str,
            _context: BindContext,
        ) -> CommandResult<RemoteLibraryConnectionConfig> {
            Ok(RemoteLibraryConnectionConfig::WebDav {
                server_url: SERVER_URL.to_owned(),
            })
        }

        fn open_storage<'b>(
            &self,
            _app_data_dir: &'b Path,
            library: &'b RegisteredLibrary,
        ) -> CommandResult<Box<dyn RepositoryStorage + 'b>> {
            self.opened_locations
                .lock()
                .expect("scripted access lock")
                .push(library.remote_root_locator().unwrap_or_default().to_owned());
            Ok(Box::new(self.provider.handle()))
        }
    }

    struct Fixture {
        _temp: TempDir,
        app_data_dir: PathBuf,
        working_copy: PathBuf,
        state: AppState,
        access: ScriptedAccess,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = TempDir::new().expect("temp directory");
            let app_data_dir = temp.path().join("app-data");
            std::fs::create_dir_all(&app_data_dir).expect("app data directory");

            let working_copy = app_data_dir.join("remote-libraries").join(LIBRARY_ID);
            let root = LibraryRoot::create(&working_copy).expect("local working copy");
            cache::initialize_library_database(&root.database_path())
                .expect("local working copy database");

            let config = AppConfig {
                libraries: vec![registered_repository(
                    ORIGINAL_LOCATION,
                    &root.database_path(),
                )],
                active_library_id: Some(LIBRARY_ID.to_owned()),
                ..AppConfig::default()
            };
            persist_app_config(&app_data_dir, &config).expect("seed config");

            let mut state = AppState::test_fixture();
            state.shell.app_data_dir = app_data_dir.clone();
            state
                .remote
                .remote_auth_sessions
                .lock()
                .expect("remote auth session lock")
                .insert(SESSION_ID.to_owned(), ready_session());

            let access = ScriptedAccess::new(temp.path().to_owned());
            Self {
                _temp: temp,
                app_data_dir,
                working_copy,
                state,
                access,
            }
        }

        fn lifecycle(&self) -> RemoteRepositoryLifecycle<'_> {
            RemoteRepositoryLifecycle::with_access(&self.state, &self.app_data_dir, &self.access)
        }

        fn registered(&self) -> RegisteredLibrary {
            load_app_config(&self.app_data_dir)
                .expect("config")
                .libraries
                .into_iter()
                .find(|entry| entry.id() == LIBRARY_ID)
                .expect("registered remote repository")
        }

        fn working_copy_has_song(&self, hash: &str) -> bool {
            let connection = cache::open_database(&self.working_copy.join("openkara.db"))
                .expect("working copy database");
            let mut statement = connection
                .prepare("SELECT 1 FROM songs WHERE hash = ?1")
                .expect("song lookup");
            statement.exists([hash]).expect("song lookup")
        }

        fn seed_remote_database(&self, revision: &str) {
            self.access
                .provider
                .put_file("openkara.db", remote_database_bytes(self._temp.path()));
            self.access.provider.set_revision(revision);
        }

        fn set_local_state(&self, local_state: LocalState) {
            let connection = self
                .state
                .remote
                .control_db()
                .expect("control db")
                .lock()
                .expect("control db lock");
            upsert_repository_state(
                &connection,
                &RepositoryStateRow {
                    library_id: LIBRARY_ID.to_owned(),
                    committed_generation: 0,
                    committed_manifest_revision: None,
                    local_base_generation: 0,
                    local_db_digest: None,
                    local_state,
                    active_operation_id: None,
                    last_success_at_ms: None,
                    last_error_code: None,
                    updated_at_ms: 1,
                    repository_id: None,
                    writer_id: None,
                },
            )
            .expect("seed repository state");
        }
    }

    fn registered_repository(remote_root_locator: &str, database_path: &Path) -> RegisteredLibrary {
        RegisteredLibrary::remote(
            LIBRARY_ID.to_owned(),
            "Repository".to_owned(),
            RemoteLibraryProvider::WebDav,
            ACCOUNT_ID.to_owned(),
            remote_root_locator.to_owned(),
            "/original".to_owned(),
            Some(RemoteLibraryConnectionConfig::WebDav {
                server_url: SERVER_URL.to_owned(),
            }),
            Some(database_path.display().to_string()),
            None,
        )
    }

    fn ready_session() -> RemoteAuthSession {
        RemoteAuthSession {
            provider: RemoteLibraryProvider::WebDav,
            state: RemoteAuthState::Ready,
            remote_root_locator: Some(ORIGINAL_LOCATION.to_owned()),
            display_name: Some("Repository".to_owned()),
            account_id: ACCOUNT_ID.to_owned(),
            error: None,
            session: ProviderSessionData::WebDav(WebDavSessionData {
                server_url: SERVER_URL.to_owned(),
                username: "user".to_owned(),
                password: "secret".to_owned(),
                root_path: None,
            }),
        }
    }

    fn remote_database_bytes(temp: &Path) -> Vec<u8> {
        let remote_root = LibraryRoot::create(&temp.join("remote-side")).expect("remote root");
        let database_path = remote_root.database_path();
        cache::initialize_library_database(&database_path).expect("remote database");
        {
            let connection = cache::open_database(&database_path).expect("remote database");
            connection
                .execute(
                    "INSERT INTO songs (hash, title, imported_at) VALUES ('song-remote', 'Remote Song', 1)",
                    [],
                )
                .expect("remote song row");
        }
        std::fs::read(&database_path).expect("remote database bytes")
    }

    #[test]
    fn reauthorize_keeps_the_remote_repository_location() {
        let fixture = Fixture::new();
        fixture.access.provider.set_revision("renewed-revision");

        let snapshot = fixture
            .lifecycle()
            .reauthorize(
                LIBRARY_ID.to_owned(),
                SESSION_ID.to_owned(),
                ORIGINAL_LOCATION.to_owned(),
                "Repository".to_owned(),
            )
            .expect("reauthorize should succeed");

        assert_eq!(snapshot.active_library_id.as_deref(), Some(LIBRARY_ID));
        let registered = fixture.registered();
        assert_eq!(
            registered.remote_root_locator(),
            Some(ORIGINAL_LOCATION),
            "Reauthorize Repository must not change the Remote Repository Location"
        );
        assert_eq!(
            registered.working_copy_root().as_deref(),
            Some(fixture.working_copy.as_path())
        );
        assert_eq!(registered.remote_revision(), Some("renewed-revision"));
        assert_eq!(fixture.access.opened_locations(), vec![ORIGINAL_LOCATION]);
    }

    #[test]
    fn reauthorize_rejects_a_different_remote_repository_location() {
        let fixture = Fixture::new();

        let error = fixture
            .lifecycle()
            .reauthorize(
                LIBRARY_ID.to_owned(),
                SESSION_ID.to_owned(),
                NEW_LOCATION.to_owned(),
                "Repository".to_owned(),
            )
            .expect_err("reauthorize must reject a new location");
        assert!(error
            .message
            .contains("cannot change the Remote Repository Location"));

        let registered = fixture.registered();
        assert_eq!(registered.remote_root_locator(), Some(ORIGINAL_LOCATION));
        assert_eq!(registered.remote_revision(), None);
        assert!(
            fixture.access.opened_locations().is_empty(),
            "a rejected reauthorization must not reach the Remote Provider"
        );
    }

    #[test]
    fn relocate_moves_the_location_and_keeps_the_local_working_copy() {
        let fixture = Fixture::new();
        fixture.access.provider.set_revision("relocated-revision");

        let snapshot = fixture
            .lifecycle()
            .relocate(
                LIBRARY_ID.to_owned(),
                SESSION_ID.to_owned(),
                NEW_LOCATION.to_owned(),
                "Repository".to_owned(),
            )
            .expect("relocate should succeed");

        assert_eq!(snapshot.active_library_id.as_deref(), Some(LIBRARY_ID));
        let registered = fixture.registered();
        assert_eq!(registered.remote_root_locator(), Some(NEW_LOCATION));
        assert_eq!(
            registered.working_copy_root().as_deref(),
            Some(fixture.working_copy.as_path()),
            "Relocate Repository keeps the existing Local Working Copy"
        );
        assert_eq!(
            registered.remote_revision(),
            Some("relocated-revision"),
            "Relocate Repository records the Remote Revision of the new location"
        );
        assert_eq!(
            fixture.access.opened_locations(),
            vec![NEW_LOCATION],
            "Relocate Repository refreshes from the new location immediately"
        );
        assert_eq!(
            fixture
                .state
                .shell
                .library
                .lock()
                .expect("library lock")
                .as_ref()
                .map(|root| root.root().to_owned()),
            Some(fixture.working_copy.clone())
        );
    }

    #[test]
    fn relocate_rejects_the_current_remote_repository_location() {
        let fixture = Fixture::new();

        let error = fixture
            .lifecycle()
            .relocate(
                LIBRARY_ID.to_owned(),
                SESSION_ID.to_owned(),
                ORIGINAL_LOCATION.to_owned(),
                "Repository".to_owned(),
            )
            .expect_err("relocate must require a different location");
        assert!(error
            .message
            .contains("requires a different Remote Repository Location"));
        assert!(fixture.access.opened_locations().is_empty());
    }

    #[test]
    fn refresh_updates_the_local_working_copy_from_the_current_remote_state() {
        let fixture = Fixture::new();
        fixture.seed_remote_database("remote-revision-2");
        assert!(!fixture.working_copy_has_song("song-remote"));

        fixture
            .lifecycle()
            .refresh()
            .expect("refresh should succeed");

        assert!(
            fixture.working_copy_has_song("song-remote"),
            "Refresh Repository installs the current remote database"
        );
        assert_eq!(
            fixture.registered().remote_revision(),
            Some("remote-revision-2")
        );
        assert_eq!(fixture.access.opened_locations(), vec![ORIGINAL_LOCATION]);
    }

    #[test]
    fn refresh_is_a_no_op_when_the_working_copy_is_not_clean() {
        let fixture = Fixture::new();
        fixture.seed_remote_database("remote-revision-2");
        fixture.set_local_state(LocalState::Dirty);

        fixture
            .lifecycle()
            .refresh()
            .expect("refresh should succeed");

        assert!(
            !fixture.working_copy_has_song("song-remote"),
            "a dirty Local Working Copy must never be overwritten"
        );
        assert_eq!(fixture.registered().remote_revision(), None);
    }

    #[test]
    fn refresh_skips_the_remote_provider_when_the_revision_is_unchanged() {
        let fixture = Fixture::new();
        fixture.seed_remote_database("remote-revision-2");
        fixture
            .lifecycle()
            .refresh()
            .expect("first refresh should succeed");

        assert!(!fixture.working_copy_has_song("song-other"));
        fixture
            .access
            .provider
            .put_file("openkara.db", b"corrupt".to_vec());

        fixture
            .lifecycle()
            .refresh()
            .expect("second refresh should succeed");

        assert!(
            fixture.working_copy_has_song("song-remote"),
            "an unchanged Remote Revision must not re-pull"
        );
    }

    fn google_drive_repository() -> RegisteredLibrary {
        RegisteredLibrary::remote(
            "repository-id".to_owned(),
            "Repository".to_owned(),
            RemoteLibraryProvider::GoogleDrive,
            "account-id".to_owned(),
            "root-id".to_owned(),
            "Repository".to_owned(),
            None,
            Some("/tmp/openkara-test-repository/openkara.db".to_owned()),
            None,
        )
    }

    #[test]
    fn reauthorize_requires_the_same_location() {
        assert!(!validate_recovery_request(
            &google_drive_repository(),
            RemoteLibraryProvider::GoogleDrive,
            "account-id",
            "root-id",
            &RepositoryRecoveryAction::Reauthorize,
        )
        .unwrap());
        assert!(validate_recovery_request(
            &google_drive_repository(),
            RemoteLibraryProvider::GoogleDrive,
            "account-id",
            "new-root",
            &RepositoryRecoveryAction::Reauthorize,
        )
        .is_err());
    }

    #[test]
    fn relocate_requires_a_different_location() {
        assert!(validate_recovery_request(
            &google_drive_repository(),
            RemoteLibraryProvider::GoogleDrive,
            "account-id",
            "new-root",
            &RepositoryRecoveryAction::Relocate,
        )
        .unwrap());
        assert!(validate_recovery_request(
            &google_drive_repository(),
            RemoteLibraryProvider::GoogleDrive,
            "account-id",
            "root-id",
            &RepositoryRecoveryAction::Relocate,
        )
        .is_err());
    }

    #[test]
    fn recovery_requires_matching_provider_and_account() {
        assert!(validate_recovery_request(
            &google_drive_repository(),
            RemoteLibraryProvider::Dropbox,
            "account-id",
            "root-id",
            &RepositoryRecoveryAction::Reauthorize,
        )
        .is_err());
        assert!(validate_recovery_request(
            &google_drive_repository(),
            RemoteLibraryProvider::GoogleDrive,
            "other-account",
            "root-id",
            &RepositoryRecoveryAction::Reauthorize,
        )
        .is_err());
    }
}
