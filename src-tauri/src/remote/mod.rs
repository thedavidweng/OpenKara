//! Remote Repository domain: providers, auth sessions, registry, sync, and mutations.
//!
//! IPC entry points live in `crate::commands::remote_library` as thin adapters.
//! Domain callers (import, lyrics, separation, playback_source, etc.) import from here.

pub(crate) mod atomic_download;
mod auth;
mod auth_binding;
mod bootstrap;
pub(crate) mod cache_catalog;
pub(crate) mod control_db;
mod dropbox;
pub(crate) mod errors;
pub(crate) mod executor;
#[cfg(test)]
mod fault_injection;
mod google_drive;
pub(crate) mod manifest;
mod mutation;
pub(crate) mod net_policy;
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
        // Single attempt. Callers that can rebuild the request should prefer
        // `net_policy::run_with_default_retry` so transport failures and
        // rate-limits are retried with the shared production policy.
        self.send().map_err(|error| {
            tracing::trace!("{op} request failed: {error}");
            crate::commands::error::CommandError::from(LibraryError::Internal(format!(
                "{op} could not be completed"
            )))
        })
    }
}

/// Send a rebuildable HTTP request with the shared production retry policy.
///
/// `build` is invoked once per attempt so the driver can retry after
/// transport failures, 429, and 5xx. Permanent HTTP failures (400/403/404/
/// 409/412) are returned as successful `Response` values so the caller can
/// classify them.
pub(crate) fn send_with_retry<F>(
    op: &'static str,
    mut build: F,
) -> std::result::Result<reqwest::blocking::Response, crate::remote::errors::RemoteError>
where
    F: FnMut() -> std::result::Result<
        reqwest::blocking::RequestBuilder,
        crate::remote::errors::RemoteError,
    >,
{
    use crate::remote::errors::{RemoteError, RemoteErrorKind};
    use crate::remote::net_policy::{
        classify_reqwest_error, classify_status, parse_retry_after, remote_error_with_retry_after,
        run_with_default_retry, AttemptOutcome,
    };

    run_with_default_retry(|| match build() {
        Ok(builder) => match builder.send() {
            Ok(response) => {
                let status = response.status();
                if status.is_success() || status.as_u16() == 206 {
                    return AttemptOutcome::Ok(response);
                }
                let kind = classify_status(status);
                if kind.retryable() {
                    let retry_after = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok())
                        .and_then(parse_retry_after);
                    // Drop the response body so the connection can be reused
                    // on the next attempt.
                    drop(response);
                    AttemptOutcome::Err(remote_error_with_retry_after(
                        kind,
                        format!("{op} failed with HTTP {status}"),
                        retry_after,
                    ))
                } else {
                    // Permanent failure — hand the response to the caller.
                    AttemptOutcome::Ok(response)
                }
            }
            Err(error) => AttemptOutcome::Err(RemoteError::new(
                classify_reqwest_error(&error),
                format!("{op} could not be completed"),
            )),
        },
        Err(error) => AttemptOutcome::Err(error),
    })
    .map_err(|error| {
        if error.kind == RemoteErrorKind::NetworkUnavailable {
            RemoteError::new(error.kind, format!("{op} could not be completed"))
        } else {
            error
        }
    })
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
