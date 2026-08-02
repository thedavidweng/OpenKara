//! Machine-readable remote error kinds.
//!
//! ## Sanitization
//!
//! Error details NEVER include OAuth tokens, passwords, request URLs containing
//! credentials, or raw provider response bodies. Only sanitized, stable code
//! strings are persisted to `remote_operations.error_code`.

use crate::commands::error::CommandError;

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
    pub conditional_replace: bool,
    pub resumable_upload: bool,
    pub revision_metadata: bool,
    /// The provider can move objects server-side (rename without re-upload).
    pub server_side_move: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteObjectMetadata {
    pub size_bytes: Option<u64>,
    pub revision: Option<String>,
}

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
    RemoteIntegrityFailed,
    NetworkUnavailable,
    RateLimited,
    AuthenticationExpired,
    PermissionDenied,
    DiskFull,
    OperationCancelled,
    /// The playback request that initiated this operation is no longer
    /// current — the user skipped to a different song (or a newer request
    /// superseded this one) while the operation was in flight. Used by the
    /// async stale-guard in `ensure_remote_stem_set_cached_guarded`
    /// so a late stem-set completion does not install files for a song the
    /// user has already moved past. Never retried: a stale request must
    /// abort, not retry.
    StaleRequest,
}

impl RemoteErrorKind {
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
            RemoteErrorKind::StaleRequest => "stale_request",
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
            "stale_request" => RemoteErrorKind::StaleRequest,
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
    pub retry_after: Option<std::time::Duration>,
}

impl RemoteError {
    pub(crate) fn new(kind: RemoteErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            code: kind.code().to_owned(),
            detail: Some(detail.into()),
            retryable: kind.retryable(),
            retry_after: None,
        }
    }

    pub(crate) fn from_kind(kind: RemoteErrorKind) -> Self {
        Self {
            kind,
            code: kind.code().to_owned(),
            detail: None,
            retryable: kind.retryable(),
            retry_after: None,
        }
    }

    pub(crate) fn to_db_columns(&self) -> (Option<String>, Option<String>) {
        (Some(self.code.clone()), self.detail.clone())
    }

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
            retry_after: None,
        })
    }

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
pub(crate) fn kind_from_io_error(error: &std::io::Error) -> RemoteErrorKind {
    if error.kind() == std::io::ErrorKind::Other && error.raw_os_error() == Some(28) {
        return RemoteErrorKind::DiskFull;
    }
    RemoteErrorKind::NetworkUnavailable
}

/// Convenience: build a `RemoteError` from an HTTP status with a sanitized
/// detail string (the status code only — never the response body).
pub(crate) fn remote_error_from_status(status: reqwest::StatusCode, context: &str) -> RemoteError {
    let kind = kind_from_http_status(status);
    RemoteError::new(kind, format!("{context} failed with HTTP {status}"))
}

pub(crate) fn verify_content_range(header: &str, offset: u64, length: u64) -> RemoteResult<()> {
    let expected_start = offset;
    let expected_end = offset + length - 1;

    let rest = header.strip_prefix("bytes").unwrap_or(header).trim();

    if rest.starts_with('*') {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "Content-Range indicates unsatisfied range ({header}) \
                 — requested {expected_start}-{expected_end}"
            ),
        ));
    }

    let (range_part, _total_part) = rest.split_once('/').unwrap_or((rest, ""));
    let (start_str, end_str) = range_part.split_once('-').ok_or_else(|| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!("malformed Content-Range header: {header}"),
        )
    })?;
    let start: u64 = start_str.parse().map_err(|_| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!("malformed Content-Range start: {header}"),
        )
    })?;
    let end: u64 = end_str.parse().map_err(|_| {
        RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!("malformed Content-Range end: {header}"),
        )
    })?;

    if start != expected_start || end != expected_end {
        return Err(RemoteError::new(
            RemoteErrorKind::RemoteIntegrityFailed,
            format!(
                "Content-Range mismatch: header says {start}-{end}, \
                 requested {expected_start}-{expected_end}"
            ),
        ));
    }
    Ok(())
}

pub(crate) type RemoteResult<T> = std::result::Result<T, RemoteError>;

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
            RemoteErrorKind::StaleRequest,
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
        assert!(!RemoteErrorKind::StaleRequest.retryable());
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

    #[test]
    fn content_range_accepts_exact_match() {
        verify_content_range("bytes 100-199/1000", 100, 100).expect("valid range");
        verify_content_range("bytes 0-0/1", 0, 1).expect("single byte");
    }

    #[test]
    fn content_range_rejects_wrong_start_or_end() {
        let err = verify_content_range("bytes 0-99/1000", 100, 100).unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::RemoteIntegrityFailed);

        let err = verify_content_range("bytes 100-150/1000", 100, 100).unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::RemoteIntegrityFailed);
    }

    #[test]
    fn content_range_rejects_unsatisfied_and_malformed() {
        let err = verify_content_range("bytes */1000", 0, 100).unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::RemoteIntegrityFailed);

        let err = verify_content_range("bytes garbage", 0, 100).unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::RemoteIntegrityFailed);
    }
}
