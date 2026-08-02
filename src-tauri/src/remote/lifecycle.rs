use crate::{
    commands::{error::CommandResult, library_setup::LibraryRegistrySnapshot},
    AppState,
};
use std::path::Path;

/// Owns the lifecycle of a registered Remote Repository.
pub(crate) struct RemoteRepositoryLifecycle<'a> {
    state: &'a AppState,
    app_data_dir: &'a Path,
}

impl<'a> RemoteRepositoryLifecycle<'a> {
    pub(crate) fn new(state: &'a AppState, app_data_dir: &'a Path) -> Self {
        Self {
            state,
            app_data_dir,
        }
    }

    pub(crate) fn reauthorize(
        &self,
        library_id: String,
        session_id: String,
        remote_root_locator: String,
        display_name: String,
    ) -> CommandResult<LibraryRegistrySnapshot> {
        super::registry::reauthorize_remote_repository(
            self.state,
            self.app_data_dir,
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
        super::registry::relocate_remote_repository(
            self.state,
            self.app_data_dir,
            library_id,
            session_id,
            remote_root_locator,
            display_name,
        )
    }

    pub(crate) fn refresh(&self) -> CommandResult<()> {
        super::sync::refresh_remote_repository(self.state)
    }
}
