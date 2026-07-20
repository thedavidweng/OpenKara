//! Remote Repository domain: providers, auth sessions, registry, sync, and mutations.
//!
//! IPC entry points live in `crate::commands::remote_library` as thin adapters.
//! Domain callers (import, lyrics, separation, playback_source, etc.) import from here.

mod auth;
mod auth_binding;
mod bootstrap;
pub(crate) mod control_db;
mod dropbox;
mod google_drive;
mod mutation;
pub(crate) mod provider;
pub(crate) mod recovery;
mod registry;
mod sync;
pub(crate) mod types;
mod webdav;

use crate::{
    commands::error::CommandResult, config::RegisteredLibrary, library::error::LibraryError,
};

/// Extension trait for `reqwest::RequestBuilder` that wraps `.send()` with
/// safe error handling: error details are logged at trace level for debugging,
/// while the user-facing error message is static and contains no sensitive data.
pub(crate) trait RequestSendExt {
    type Response;
    fn send_network(
        self,
        op: &'static str,
    ) -> std::result::Result<Self::Response, crate::commands::error::CommandError>;
}

impl RequestSendExt for reqwest::blocking::RequestBuilder {
    type Response = reqwest::blocking::Response;
    fn send_network(
        self,
        op: &'static str,
    ) -> std::result::Result<reqwest::blocking::Response, crate::commands::error::CommandError>
    {
        self.send().map_err(|_error| {
            tracing::trace!("{op} request failed");
            crate::commands::error::CommandError::from(LibraryError::Internal(format!(
                "{op} could not be completed"
            )))
        })
    }
}

pub(crate) use auth::{begin_remote_auth, cancel_remote_auth, open_external_url, poll_remote_auth};
pub(crate) use mutation::{
    publish_song_to_active_remote_if_ready, run_active_library_mirror_mutation,
    run_database_then_library_mirror_mutation, run_imported_songs_mutation,
    run_song_database_mutation, run_song_database_mutation_with_result,
    run_songs_database_mutation, run_updated_songs_mutation, song_ids_from_songs,
};
pub(crate) use registry::{
    create_remote_library, list_remote_library_roots, reauthorize_remote_library,
    register_remote_library, remove_remote_library_credentials, resolve_remote_library_candidate,
};
pub(crate) use sync::{
    active_remote_library, ensure_remote_file_cached, get_all_upload_statuses,
    mirror_local_library_to_remote, publish_song_to_remote, publish_songs_to_remote,
    sync_active_remote_library,
};
pub use types::{
    RemoteAuthSession, RemoteAuthStart, RemoteAuthState, RemoteAuthStatus, RemoteLibraryCandidate,
    UploadState, UploadStatusSnapshot,
};

pub(crate) fn delete_remote_library_root(
    app_data_dir: &std::path::Path,
    library: &RegisteredLibrary,
) -> CommandResult<()> {
    let provider = provider::create_provider(app_data_dir, library)?;
    provider.delete_path("")
}
