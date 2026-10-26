//! Application boundary adapter for `openkara_remote::error`.
//!
//! Error details NEVER include OAuth tokens, passwords, request URLs containing
//! credentials, or raw provider response bodies. Only sanitized, stable code
//! strings are persisted to `remote_operations.error_code`.

use crate::commands::error::CommandError;

pub(crate) use openkara_remote::error::{
    kind_from_http_status_code, kind_from_io_error, verify_content_range, RemoteError,
    RemoteErrorKind, RemoteObjectMetadata, RemoteProviderCapabilities, RemoteResult,
};

/// Map a core `RemoteError` into the Tauri IPC `CommandError` at the
/// application boundary.
pub(crate) fn remote_error_to_command_error(error: &RemoteError) -> CommandError {
    // Map retryable remote errors to NetworkUnavailable so the existing
    // ErrorCode enum carries the retry intent; non-retryable errors use
    // Internal with retryable=false so the caller does not retry.
    if error.retryable {
        CommandError::new(
            crate::commands::error::ErrorCode::NetworkUnavailable,
            error.message(),
            true,
            crate::commands::error::FallbackAction::Retry,
        )
    } else {
        CommandError::new(
            crate::commands::error::ErrorCode::Internal,
            error.message(),
            false,
            crate::commands::error::FallbackAction::KeepCurrentState,
        )
    }
}

impl From<RemoteError> for CommandError {
    fn from(err: RemoteError) -> Self {
        remote_error_to_command_error(&err)
    }
}

pub(crate) fn kind_from_http_status(status: reqwest::StatusCode) -> RemoteErrorKind {
    kind_from_http_status_code(status.as_u16())
}

/// Convenience: build a `RemoteError` from an HTTP status with a sanitized
/// detail string (the status code only — never the response body).
pub(crate) fn remote_error_from_status(status: reqwest::StatusCode, context: &str) -> RemoteError {
    let kind = kind_from_http_status(status);
    RemoteError::new(kind, format!("{context} failed with HTTP {status}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_status_maps_to_expected_kinds() {
        assert_eq!(
            kind_from_http_status(reqwest::StatusCode::UNAUTHORIZED),
            RemoteErrorKind::AuthenticationExpired
        );
        assert_eq!(
            kind_from_http_status(reqwest::StatusCode::FORBIDDEN),
            RemoteErrorKind::PermissionDenied
        );
        assert_eq!(
            kind_from_http_status(reqwest::StatusCode::CONFLICT),
            RemoteErrorKind::RemoteConflict
        );
        assert_eq!(
            kind_from_http_status(reqwest::StatusCode::PRECONDITION_FAILED),
            RemoteErrorKind::RemoteConflict
        );
        assert_eq!(
            kind_from_http_status(reqwest::StatusCode::TOO_MANY_REQUESTS),
            RemoteErrorKind::RateLimited
        );
        assert_eq!(
            kind_from_http_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR),
            RemoteErrorKind::NetworkUnavailable
        );
    }

    #[test]
    fn from_remote_error_preserves_retryable_flag_and_message() {
        let retryable = RemoteError::new(RemoteErrorKind::NetworkUnavailable, "connection reset");
        let cmd = CommandError::from(retryable);
        assert!(cmd.retryable);
        assert_eq!(cmd.message, "network_unavailable: connection reset");

        let non_retryable = RemoteError::from_kind(RemoteErrorKind::RemoteConflict);
        let cmd = CommandError::from(non_retryable);
        assert!(!cmd.retryable);
        assert_eq!(cmd.message, "remote_conflict");
    }
}
