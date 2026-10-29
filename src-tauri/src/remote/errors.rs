//! Application boundary adapter for `openkara_remote::error`.
//!
//! Error details NEVER include OAuth tokens, passwords, request URLs containing
//! credentials, or raw provider response bodies. Only sanitized, stable code
//! strings are persisted to `remote_operations.error_code`.

use crate::commands::error::CommandError;

pub(crate) use openkara_remote::error::{
    remote_error_from_status, verify_content_range, RemoteError, RemoteErrorKind,
    RemoteObjectMetadata, RemoteProviderCapabilities, RemoteResult,
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

#[cfg(test)]
mod tests {
    use super::*;

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
