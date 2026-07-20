//! Machine-readable remote error kinds.
//!
//! Provider HTTP responses and IO failures are mapped to a small, stable set
//! of error kinds so the operation executor, recovery, and UI can branch on
//! the cause without parsing free-text messages.
//!
//! ## Sanitization
//!
//! Error details NEVER include OAuth tokens, passwords, request URLs containing
//! credentials, or raw provider response bodies. Only sanitized, stable code
//! strings are persisted to `remote_operations.error_code`.

use crate::commands::error::{CommandError, CommandResult};

/// Capabilities a remote provider supports. Providers report these so the
/// operation executor can fail closed when a safe-write path requires a
/// capability the provider cannot enforce.
///
/// A provider that returns `false` for `conditional_replace` must NOT be used
/// for manifest publication — the executor returns
/// [`RemoteErrorKind::ProviderCapabilityUnavailable`] instead of downgrading
/// to last-writer-wins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RemoteProviderCapabilities {
    /// The provider can enforce compare-and-swap on a single object via
    /// `conditional_replace` (ETag/If-Match, Dropbox rev, etc.).
    pub conditional_replace: bool,
    /// The provider supports resumable uploads with offset query/resume.
    /// PR#5 fills this `true` where supported; PR#4 leaves it `false`.
    // used by PR#5: resumable uploads
    #[allow(dead_code)]
    pub resumable_upload: bool,
    /// The provider supports HTTP Range downloads.
    pub range_download: bool,
    /// The provider exposes stable revision metadata (ETag / rev /
    /// headRevisionId) usable for change detection.
    pub revision_metadata: bool,
    /// The provider can move objects server-side (rename without re-upload).
    pub server_side_move: bool,
}

/// Metadata for a remote object returned by `stat`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteObjectMetadata {
    /// Byte length of the object, when known.
    pub size: Option<u64>,
    /// Provider-specific revision token (ETag, Dropbox rev, Google Drive
    /// headRevisionId). Used as the `expected_revision` for
    /// `conditional_replace`.
    pub revision: Option<String>,
}

/// Machine-readable error kind for remote operations.
///
/// Maps to the `error_code` column in `remote_operations` via
/// [`RemoteErrorKind::code`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteErrorKind {
    /// A compare-and-swap precondition failed — another device committed a
    /// newer generation first. The local operation must NOT retry as an
    /// unconditional overwrite.
    RemoteConflict,
    /// The provider does not support a required capability (e.g. conditional
    /// replacement) for a safe write. Fail closed rather than downgrading to
    /// last-writer-wins.
    ProviderCapabilityUnavailable,
    /// A downloaded or uploaded object failed an integrity check (size, digest,
    /// or SQLite integrity).
    RemoteIntegrityFailed,
    /// The network is unreachable or the request timed out.
    NetworkUnavailable,
    /// The provider rate-limited the request (HTTP 429 or retry-after).
    RateLimited,
    /// OAuth credentials expired or were revoked. The repository should
    /// transition to `ReauthRequired`.
    AuthenticationExpired,
    /// The authenticated user lacks permission for the operation.
    PermissionDenied,
    /// The local disk is full.
    #[allow(dead_code)]
    DiskFull,
    /// The operation was cancelled by the user or a coalescing decision.
    #[allow(dead_code)]
    OperationCancelled,
}

impl RemoteErrorKind {
    /// Stable, machine-readable code string persisted to
    /// `remote_operations.error_code` and emitted to IPC consumers.
    pub(crate) fn code(self) -> &'static str {
        match self {
            RemoteErrorKind::RemoteConflict => "remote_conflict",
            RemoteErrorKind::ProviderCapabilityUnavailable => "provider_capability_unavailable",
            RemoteErrorKind::RemoteIntegrityFailed => "remote_integrity_failed",
            RemoteErrorKind::NetworkUnavailable => "network_unavailable",
            RemoteErrorKind::RateLimited => "rate_limited",
            RemoteErrorKind::AuthenticationExpired => "authentication_expired",
            RemoteErrorKind::PermissionDenied => "permission_denied",
            RemoteErrorKind::DiskFull => "disk_full",
            RemoteErrorKind::OperationCancelled => "operation_cancelled",
        }
    }

    /// Whether an operation in this error kind should be retried automatically.
    /// Conflicts, capability errors, permission errors, integrity failures, and
    /// cancellations are never auto-retried.
    pub(crate) fn retryable(self) -> bool {
        matches!(
            self,
            RemoteErrorKind::NetworkUnavailable | RemoteErrorKind::RateLimited
        )
    }

    #[allow(dead_code)]
    fn from_db(value: &str) -> Option<Self> {
        Some(match value {
            "remote_conflict" => RemoteErrorKind::RemoteConflict,
            "provider_capability_unavailable" => RemoteErrorKind::ProviderCapabilityUnavailable,
            "remote_integrity_failed" => RemoteErrorKind::RemoteIntegrityFailed,
            "network_unavailable" => RemoteErrorKind::NetworkUnavailable,
            "rate_limited" => RemoteErrorKind::RateLimited,
            "authentication_expired" => RemoteErrorKind::AuthenticationExpired,
            "permission_denied" => RemoteErrorKind::PermissionDenied,
            "disk_full" => RemoteErrorKind::DiskFull,
            "operation_cancelled" => RemoteErrorKind::OperationCancelled,
            _ => return None,
        })
    }
}

/// A typed remote error carrying a sanitized code, optional detail, and a
/// retryable flag. The detail must never contain credentials or raw provider
/// responses.
#[derive(Debug, Clone)]
pub(crate) struct RemoteError {
    pub kind: RemoteErrorKind,
    pub code: String,
    pub detail: Option<String>,
    pub retryable: bool,
}

impl RemoteError {
    pub(crate) fn new(kind: RemoteErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            code: kind.code().to_owned(),
            detail: Some(detail.into()),
            retryable: kind.retryable(),
        }
    }

    pub(crate) fn from_kind(kind: RemoteErrorKind) -> Self {
        Self {
            kind,
            code: kind.code().to_owned(),
            detail: None,
            retryable: kind.retryable(),
        }
    }

    /// Serialize to the `(error_code, error_detail)` pair stored in
    /// `remote_operations`.
    pub(crate) fn to_db_columns(&self) -> (Option<String>, Option<String>) {
        (Some(self.code.clone()), self.detail.clone())
    }

    /// Reconstruct from the stored columns. Returns `None` when the code is
    /// absent or unrecognized (e.g. a row written by an older version).
    #[allow(dead_code)]
    pub(crate) fn from_db_columns(
        error_code: Option<&str>,
        error_detail: Option<&str>,
    ) -> Option<Self> {
        let code = error_code?;
        let kind = RemoteErrorKind::from_db(code)?;
        Some(Self {
            kind,
            code: code.to_owned(),
            detail: error_detail.map(str::to_owned),
            retryable: kind.retryable(),
        })
    }

    /// Convert to a `CommandError` for the IPC/command layer.
    pub(crate) fn to_command_error(&self) -> CommandError {
        let message = match &self.detail {
            Some(detail) => format!("{}: {detail}", self.kind.code()),
            None => self.kind.code().to_owned(),
        };
        // Map retryable remote errors to NetworkUnavailable so the existing
        // ErrorCode enum carries the retry intent; non-retryable errors use
        // Internal with retryable=false so the caller does not retry.
        if self.retryable {
            CommandError::new(
                crate::commands::error::ErrorCode::NetworkUnavailable,
                message,
                true,
                crate::commands::error::FallbackAction::Retry,
            )
        } else {
            CommandError::new(
                crate::commands::error::ErrorCode::Internal,
                message,
                false,
                crate::commands::error::FallbackAction::KeepCurrentState,
            )
        }
    }
}

impl From<RemoteError> for CommandError {
    fn from(err: RemoteError) -> Self {
        err.to_command_error()
    }
}

/// Map an HTTP status code to a `RemoteErrorKind`. Used by provider
/// implementations when a request fails with a known status.
///
/// - 401 → AuthenticationExpired
/// - 403 → PermissionDenied
/// - 404 → PermissionDenied (treat missing-as-forbidden for safe writes; the
///   caller distinguishes "absent" via `stat` returning `None`)
/// - 408 → NetworkUnavailable (request timeout)
/// - 409, 412 → RemoteConflict (precondition failed)
/// - 425 → NetworkUnavailable (too early)
/// - 429 → RateLimited
/// - 5xx → NetworkUnavailable (server error, retryable)
pub(crate) fn kind_from_http_status(status: reqwest::StatusCode) -> RemoteErrorKind {
    match status.as_u16() {
        401 => RemoteErrorKind::AuthenticationExpired,
        403 => RemoteErrorKind::PermissionDenied,
        404 => RemoteErrorKind::PermissionDenied,
        408 => RemoteErrorKind::NetworkUnavailable,
        409 | 412 => RemoteErrorKind::RemoteConflict,
        425 => RemoteErrorKind::NetworkUnavailable,
        429 => RemoteErrorKind::RateLimited,
        500..=599 => RemoteErrorKind::NetworkUnavailable,
        _ => RemoteErrorKind::NetworkUnavailable,
    }
}

/// Map an `std::io::Error` to a `RemoteErrorKind` when it occurs during a
/// file write (e.g. uploading bytes read from disk). Disk-full is detected by
/// error kind; everything else is treated as a local IO failure (not a remote
/// kind, but reported as NetworkUnavailable so the operation retries).
#[allow(dead_code)]
pub(crate) fn kind_from_io_error(error: &std::io::Error) -> RemoteErrorKind {
    if error.kind() == std::io::ErrorKind::Other {
        // `ErrorKind::Other` may wrap ENOSPC on some platforms via
        // `std::io::Error::other`; check the raw OS error number for ENOSPC
        // (28 on most Unix systems).
        if error.raw_os_error() == Some(28) {
            return RemoteErrorKind::DiskFull;
        }
    }
    RemoteErrorKind::NetworkUnavailable
}

/// Convenience: build a `RemoteError` from an HTTP status with a sanitized
/// detail string (the status code only — never the response body).
pub(crate) fn remote_error_from_status(status: reqwest::StatusCode, context: &str) -> RemoteError {
    let kind = kind_from_http_status(status);
    RemoteError::new(kind, format!("{context} failed with HTTP {status}"))
}

/// Result alias for operations that produce a typed `RemoteError`.
pub(crate) type RemoteResult<T> = std::result::Result<T, RemoteError>;

/// Wrap a `CommandResult` into a `RemoteResult`, mapping the opaque
/// `CommandError` to a `NetworkUnavailable` kind (the command layer already
/// sanitized the message). Used when a provider helper returns a
/// `CommandError` but the caller needs the typed kind.
#[allow(dead_code)]
pub(crate) fn remote_result_from_command<T>(
    result: CommandResult<T>,
    fallback_kind: RemoteErrorKind,
) -> RemoteResult<T> {
    result.map_err(|err| RemoteError::new(fallback_kind, err.message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_code_round_trips() {
        for kind in [
            RemoteErrorKind::RemoteConflict,
            RemoteErrorKind::ProviderCapabilityUnavailable,
            RemoteErrorKind::RemoteIntegrityFailed,
            RemoteErrorKind::NetworkUnavailable,
            RemoteErrorKind::RateLimited,
            RemoteErrorKind::AuthenticationExpired,
            RemoteErrorKind::PermissionDenied,
            RemoteErrorKind::DiskFull,
            RemoteErrorKind::OperationCancelled,
        ] {
            let code = kind.code();
            let back = RemoteErrorKind::from_db(code).expect("round trip");
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn only_network_and_rate_limit_are_retryable() {
        assert!(RemoteErrorKind::NetworkUnavailable.retryable());
        assert!(RemoteErrorKind::RateLimited.retryable());
        assert!(!RemoteErrorKind::RemoteConflict.retryable());
        assert!(!RemoteErrorKind::ProviderCapabilityUnavailable.retryable());
        assert!(!RemoteErrorKind::AuthenticationExpired.retryable());
        assert!(!RemoteErrorKind::PermissionDenied.retryable());
        assert!(!RemoteErrorKind::RemoteIntegrityFailed.retryable());
        assert!(!RemoteErrorKind::DiskFull.retryable());
        assert!(!RemoteErrorKind::OperationCancelled.retryable());
    }

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
    fn db_columns_round_trip() {
        let err = RemoteError::new(
            RemoteErrorKind::RemoteConflict,
            "manifest generation 3 expected, 4 found",
        );
        let (code, detail) = err.to_db_columns();
        let back = RemoteError::from_db_columns(code.as_deref(), detail.as_deref()).unwrap();
        assert_eq!(back.kind, RemoteErrorKind::RemoteConflict);
        assert_eq!(back.code, "remote_conflict");
        assert!(!back.retryable);
    }

    #[test]
    fn from_db_columns_returns_none_for_unknown_code() {
        assert!(RemoteError::from_db_columns(Some("unknown_code"), None).is_none());
        assert!(RemoteError::from_db_columns(None, None).is_none());
    }

    #[test]
    fn to_command_error_preserves_retryable_flag() {
        let retryable = RemoteError::from_kind(RemoteErrorKind::NetworkUnavailable);
        let cmd = retryable.to_command_error();
        assert!(cmd.retryable);

        let non_retryable = RemoteError::from_kind(RemoteErrorKind::RemoteConflict);
        let cmd = non_retryable.to_command_error();
        assert!(!cmd.retryable);
    }
}
