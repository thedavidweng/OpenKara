use std::time::Duration;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RemoteProviderCapabilities {
    pub conditional_replace: bool,
    pub resumable_upload: bool,
    pub revision_metadata: bool,
    pub server_side_move: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteObjectMetadata {
    pub size_bytes: Option<u64>,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteErrorKind {
    RemoteConflict,
    ProviderCapabilityUnavailable,
    RemoteIntegrityFailed,
    NetworkUnavailable,
    RateLimited,
    AuthenticationExpired,
    PermissionDenied,
    DiskFull,
    OperationCancelled,
    StaleRequest,
}

impl RemoteErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::RemoteConflict => "remote_conflict",
            Self::ProviderCapabilityUnavailable => "provider_capability_unavailable",
            Self::RemoteIntegrityFailed => "remote_integrity_failed",
            Self::NetworkUnavailable => "network_unavailable",
            Self::RateLimited => "rate_limited",
            Self::AuthenticationExpired => "authentication_expired",
            Self::PermissionDenied => "permission_denied",
            Self::DiskFull => "disk_full",
            Self::OperationCancelled => "operation_cancelled",
            Self::StaleRequest => "stale_request",
        }
    }

    pub fn retryable(self) -> bool {
        matches!(self, Self::NetworkUnavailable | Self::RateLimited)
    }

    pub fn from_code(value: &str) -> Option<Self> {
        match value {
            "remote_conflict" => Some(Self::RemoteConflict),
            "provider_capability_unavailable" => Some(Self::ProviderCapabilityUnavailable),
            "remote_integrity_failed" => Some(Self::RemoteIntegrityFailed),
            "network_unavailable" => Some(Self::NetworkUnavailable),
            "rate_limited" => Some(Self::RateLimited),
            "authentication_expired" => Some(Self::AuthenticationExpired),
            "permission_denied" => Some(Self::PermissionDenied),
            "disk_full" => Some(Self::DiskFull),
            "operation_cancelled" => Some(Self::OperationCancelled),
            "stale_request" => Some(Self::StaleRequest),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteError {
    pub kind: RemoteErrorKind,
    pub code: String,
    pub detail: Option<String>,
    pub retryable: bool,
    pub retry_after: Option<Duration>,
}

impl RemoteError {
    pub fn new(kind: RemoteErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            code: kind.code().to_owned(),
            detail: Some(detail.into()),
            retryable: kind.retryable(),
            retry_after: None,
        }
    }

    pub fn from_kind(kind: RemoteErrorKind) -> Self {
        Self {
            kind,
            code: kind.code().to_owned(),
            detail: None,
            retryable: kind.retryable(),
            retry_after: None,
        }
    }

    pub fn message(&self) -> String {
        match self.detail.as_deref() {
            Some(detail) if !detail.is_empty() => format!("{}: {detail}", self.kind.code()),
            _ => self.kind.code().to_owned(),
        }
    }

    pub fn to_db_columns(&self) -> (Option<String>, Option<String>) {
        (Some(self.code.clone()), self.detail.clone())
    }

    pub fn from_db_columns(error_code: Option<&str>, error_detail: Option<&str>) -> Option<Self> {
        let code = error_code?;
        let kind = RemoteErrorKind::from_code(code)?;
        Some(Self {
            kind,
            code: code.to_owned(),
            detail: error_detail.map(ToOwned::to_owned),
            retryable: kind.retryable(),
            retry_after: None,
        })
    }
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for RemoteError {}

pub type RemoteResult<T> = std::result::Result<T, RemoteError>;

pub fn kind_from_http_status_code(status: u16) -> RemoteErrorKind {
    match status {
        401 => RemoteErrorKind::AuthenticationExpired,
        403 | 404 => RemoteErrorKind::PermissionDenied,
        408 | 425 => RemoteErrorKind::NetworkUnavailable,
        409 | 412 => RemoteErrorKind::RemoteConflict,
        429 => RemoteErrorKind::RateLimited,
        500..=599 => RemoteErrorKind::NetworkUnavailable,
        _ => RemoteErrorKind::NetworkUnavailable,
    }
}

pub fn kind_from_io_error(error: &std::io::Error) -> RemoteErrorKind {
    if error.kind() == std::io::ErrorKind::StorageFull
        || (error.kind() == std::io::ErrorKind::Other && error.raw_os_error() == Some(28))
    {
        return RemoteErrorKind::DiskFull;
    }
    RemoteErrorKind::NetworkUnavailable
}

/// Validate an HTTP `Content-Range` response header against the requested
/// `[offset, offset + length - 1]` window. Expected format: `bytes start-end/total`
/// (with `total` optionally `*`).
pub fn verify_content_range(header: &str, offset: u64, length: u64) -> RemoteResult<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_error_kind_codes_round_trip() {
        let all = [
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
        ];
        for kind in all {
            assert_eq!(RemoteErrorKind::from_code(kind.code()), Some(kind));
        }
        assert_eq!(RemoteErrorKind::from_code("unknown_kind"), None);
    }

    #[test]
    fn retryable_classification_matches_recoverable_transport_failures() {
        assert!(RemoteErrorKind::NetworkUnavailable.retryable());
        assert!(RemoteErrorKind::RateLimited.retryable());

        assert!(!RemoteErrorKind::RemoteConflict.retryable());
        assert!(!RemoteErrorKind::ProviderCapabilityUnavailable.retryable());
        assert!(!RemoteErrorKind::RemoteIntegrityFailed.retryable());
        assert!(!RemoteErrorKind::AuthenticationExpired.retryable());
        assert!(!RemoteErrorKind::PermissionDenied.retryable());
        assert!(!RemoteErrorKind::DiskFull.retryable());
        assert!(!RemoteErrorKind::OperationCancelled.retryable());
        assert!(!RemoteErrorKind::StaleRequest.retryable());
    }

    #[test]
    fn error_db_columns_round_trip() {
        let err = RemoteError::new(RemoteErrorKind::RemoteConflict, "etag mismatch");
        let (code, detail) = err.to_db_columns();
        let restored = RemoteError::from_db_columns(code.as_deref(), detail.as_deref()).unwrap();
        assert_eq!(restored.kind, RemoteErrorKind::RemoteConflict);
        assert_eq!(restored.detail.as_deref(), Some("etag mismatch"));
        assert!(!restored.retryable);
        assert!(RemoteError::from_db_columns(Some("unknown_code"), None).is_none());
        assert!(RemoteError::from_db_columns(None, None).is_none());
    }

    #[test]
    fn http_status_and_io_errors_classify_into_domain_kinds() {
        assert_eq!(
            kind_from_http_status_code(401),
            RemoteErrorKind::AuthenticationExpired
        );
        assert_eq!(
            kind_from_http_status_code(403),
            RemoteErrorKind::PermissionDenied
        );
        assert_eq!(
            kind_from_http_status_code(404),
            RemoteErrorKind::PermissionDenied
        );
        assert_eq!(
            kind_from_http_status_code(409),
            RemoteErrorKind::RemoteConflict
        );
        assert_eq!(
            kind_from_http_status_code(412),
            RemoteErrorKind::RemoteConflict
        );
        assert_eq!(
            kind_from_http_status_code(429),
            RemoteErrorKind::RateLimited
        );
        assert_eq!(
            kind_from_http_status_code(503),
            RemoteErrorKind::NetworkUnavailable
        );

        let full_err = std::io::Error::from_raw_os_error(28);
        assert_eq!(kind_from_io_error(&full_err), RemoteErrorKind::DiskFull);
        let other_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe");
        assert_eq!(
            kind_from_io_error(&other_err),
            RemoteErrorKind::NetworkUnavailable
        );
    }

    #[test]
    fn verify_content_range_accepts_matching_and_rejects_mismatched_windows() {
        assert!(verify_content_range("bytes 100-199/1000", 100, 100).is_ok());
        assert!(verify_content_range("bytes 0-0/1", 0, 1).is_ok());

        let mismatch = verify_content_range("bytes 0-50/500", 0, 100).unwrap_err();
        assert_eq!(mismatch.kind, RemoteErrorKind::RemoteIntegrityFailed);
        let unsatisfied = verify_content_range("bytes */1000", 0, 100).unwrap_err();
        assert_eq!(unsatisfied.kind, RemoteErrorKind::RemoteIntegrityFailed);
        assert!(verify_content_range("bytes garbage", 0, 100).is_err());
    }
}
