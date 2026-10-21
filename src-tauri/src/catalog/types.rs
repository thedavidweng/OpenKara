use crate::commands::error::{
    internal_error, online_source_disabled, streaming_auth_failed, streaming_session_expired,
    video_source_unavailable, CommandError,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const YOUTUBE_SOURCE_ID: &str = "youtube";
pub const YOUTUBE_QUEUE_PREFIX: &str = "yt:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    UnknownSource {
        source_id: String,
    },
    SourceDisabled {
        source_id: String,
    },
    AuthFailed {
        source_id: String,
        detail: String,
    },
    SessionExpired {
        source_id: String,
    },
    SourceFailure {
        code: String,
        message: String,
        retryable: bool,
    },
    Network(String),
    Cancelled,
    VideoUnavailable {
        reason: VideoUnavailableReason,
    },
    Internal(String),
}

impl From<CatalogError> for CommandError {
    fn from(error: CatalogError) -> Self {
        match error {
            CatalogError::UnknownSource { source_id } => CommandError::new(
                crate::commands::error::ErrorCode::StreamingSourceNotFound,
                format!("unknown online source: {source_id}"),
                false,
                crate::commands::error::FallbackAction::KeepCurrentState,
            ),
            CatalogError::SourceDisabled { source_id } => {
                online_source_disabled(format!("online source {source_id} is off"))
            }
            CatalogError::AuthFailed { source_id, detail } => {
                streaming_auth_failed(format!("sign-in for {source_id} failed: {detail}"))
            }
            CatalogError::SessionExpired { source_id } => {
                streaming_session_expired(format!("streaming session for {source_id} expired"))
            }
            CatalogError::SourceFailure {
                code,
                message,
                retryable,
            } => CommandError::new(
                match code.as_str() {
                    "session_expired" => crate::commands::error::ErrorCode::StreamingSessionExpired,
                    "authentication_required" => {
                        crate::commands::error::ErrorCode::StreamingAuthFailed
                    }
                    "source_disabled" => crate::commands::error::ErrorCode::OnlineSourceDisabled,
                    "not_found" => crate::commands::error::ErrorCode::StreamingSourceNotFound,
                    "unsupported_operation" => {
                        crate::commands::error::ErrorCode::StreamingUnsupportedOperation
                    }
                    "challenge_expired" => {
                        crate::commands::error::ErrorCode::StreamingChallengeExpired
                    }
                    "invalid_cursor" => crate::commands::error::ErrorCode::StreamingInvalidCursor,
                    "active_work" | "busy" => {
                        crate::commands::error::ErrorCode::StreamingWorkActive
                    }
                    "cancelled" => crate::commands::error::ErrorCode::StreamingCancelled,
                    "network" => crate::commands::error::ErrorCode::NetworkUnavailable,
                    _ => crate::commands::error::ErrorCode::StreamingSourceUnavailable,
                },
                format!("{code}: {message}"),
                retryable,
                if retryable {
                    crate::commands::error::FallbackAction::Retry
                } else {
                    crate::commands::error::FallbackAction::KeepCurrentState
                },
            ),
            CatalogError::Network(message) => CommandError::new(
                crate::commands::error::ErrorCode::NetworkUnavailable,
                message,
                true,
                crate::commands::error::FallbackAction::Retry,
            ),
            CatalogError::Cancelled => CommandError::new(
                crate::commands::error::ErrorCode::StreamingCancelled,
                "streaming operation was cancelled",
                false,
                crate::commands::error::FallbackAction::KeepCurrentState,
            ),
            CatalogError::VideoUnavailable { reason } => {
                video_source_unavailable(reason.as_str().to_owned())
            }
            CatalogError::Internal(message) => internal_error(message),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoUnavailableReason {
    InvalidUrl,
    AgeRestricted,
    Private,
    Unlisted,
    Unavailable,
}

impl VideoUnavailableReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidUrl => "invalid_url",
            Self::AgeRestricted => "age_restricted",
            Self::Private => "private",
            Self::Unlisted => "unlisted",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnlineSourceCapabilities {
    pub sign_in: bool,
    pub browse: bool,
    pub import: bool,
    pub resolve_video: bool,
}

impl OnlineSourceCapabilities {
    pub const fn none() -> Self {
        Self {
            sign_in: false,
            browse: false,
            import: false,
            resolve_video: false,
        }
    }

    pub fn for_source(source_id: &str, enabled: bool) -> Self {
        if !enabled {
            return Self::none();
        }
        match source_id {
            YOUTUBE_SOURCE_ID => Self {
                sign_in: false,
                browse: false,
                import: false,
                resolve_video: true,
            },
            _ => Self::none(),
        }
    }
}

pub type StreamingCredentials = std::collections::BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingSessionSnapshot {
    pub source_id: String,
    pub signed_in: bool,
    pub display_name: Option<String>,
    pub expired: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportRefusalReason {
    NoPlayRights,
    TrialClip,
    EmptyUrl,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRefusal {
    pub reason: ImportRefusalReason,
    pub title: String,
    pub artist: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingTrack {
    pub source_id: String,
    pub remote_track_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub refusal: Option<ImportRefusal>,
    /// Song id (hash) of the library song this track was already imported as,
    /// from the persisted Streaming Track Identity. Filled by the command
    /// layer; sources always return `None`.
    pub library_song_id: Option<String>,
    /// Host-managed artwork handle; never a source CDN URL.
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingPlaylistSummary {
    pub source_id: String,
    pub remote_id: String,
    pub name: String,
    pub track_count: Option<u32>,
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingPlaylistDetail {
    pub source_id: String,
    pub remote_id: String,
    pub name: String,
    pub tracks: Vec<StreamingTrack>,
    pub cover_url: Option<String>,
    pub creator: Option<String>,
    pub description: Option<String>,
    pub track_count: Option<u32>,
    pub updated_at_ms: Option<u64>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingTrackListing {
    pub tracks: Vec<StreamingTrack>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingPlaylistListing {
    pub items: Vec<StreamingPlaylistSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStreamingFile {
    pub path: PathBuf,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamingResolveOutcome {
    File(ResolvedStreamingFile),
    Refusal(ImportRefusal),
}

#[derive(Debug, Clone)]
pub struct ResolvedStreamingUrl {
    pub descriptor: crate::source_host::ResourceDescriptor,
    /// Container extension (`mp3`, `flac`, ...), used as the decoder hint.
    pub extension: String,
}

#[derive(Debug, Clone)]
pub enum StreamingPreviewOutcome {
    Stream(ResolvedStreamingUrl),
    Refusal(ImportRefusal),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryDecisionMeta {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub format: String,
    pub bit_rate_bps: Option<u32>,
    pub duration_ms: Option<u64>,
    pub file_size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportConflictPrompt {
    pub source_id: String,
    pub remote_track_id: String,
    pub library: LibraryDecisionMeta,
    pub incoming: LibraryDecisionMeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryDecisionAction {
    Keep,
    Replace,
    ApplyKeep,
    ApplyReplace,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamingImportFailureReason {
    Refusal,
    Cancelled,
    ImportFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingImportFailure {
    pub remote_track_id: String,
    pub title: String,
    pub artist: String,
    pub reason: StreamingImportFailureReason,
    pub refusal: Option<ImportRefusal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamingImportStatus {
    AwaitingDecision,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingImportProgress {
    pub source_id: String,
    pub status: StreamingImportStatus,
    pub imported_song_ids: Vec<String>,
    pub failed: Vec<StreamingImportFailure>,
    pub playlist_id: Option<String>,
    pub conflict: Option<ImportConflictPrompt>,
}

/// Per-track state of a running Streaming Import, reported through the
/// `streaming-import-progress` event and the job snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamingImportTrackState {
    Queued,
    Downloading {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        percent: Option<u8>,
    },
    Importing,
    Imported {
        song_id: String,
    },
    Skipped {
        refusal: ImportRefusal,
    },
    Failed {
        reason: StreamingImportFailureReason,
    },
}

impl StreamingImportTrackState {
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Imported { .. } | Self::Skipped { .. } | Self::Failed { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingImportEvent {
    pub source_id: String,
    pub job_id: String,
    pub remote_track_id: String,
    pub title: String,
    pub artist: String,
    pub state: StreamingImportTrackState,
    pub completed: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamingImportJobPhase {
    Running,
    AwaitingDecision,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingImportTrackSnapshot {
    pub remote_track_id: String,
    pub title: String,
    pub artist: String,
    pub state: StreamingImportTrackState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingImportJobSnapshot {
    pub source_id: String,
    pub job_id: String,
    pub phase: StreamingImportJobPhase,
    pub completed: u32,
    pub total: u32,
    pub tracks: Vec<StreamingImportTrackSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoQueueItem {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub duration_ms: Option<u64>,
    pub thumbnail_url: Option<String>,
    pub watch_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevealTarget {
    pub available: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevealTargets {
    pub song_file: RevealTarget,
    pub stems: RevealTarget,
}

pub fn youtube_queue_id(video_id: &str) -> String {
    format!("{YOUTUBE_QUEUE_PREFIX}{video_id}")
}

pub fn is_video_source_queue_id(id: &str) -> bool {
    id.starts_with(YOUTUBE_QUEUE_PREFIX)
}

pub fn streaming_preview_queue_id(source_id: &str, remote_track_id: &str) -> String {
    let mut encoded = String::new();
    for byte in remote_track_id.as_bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(byte) {
            encoded.push(*byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    format!("stream:{source_id}:{encoded}")
}

pub fn streaming_preview_reference(queue_id: &str) -> Option<(String, String)> {
    let (source_id, encoded) = queue_id.strip_prefix("stream:")?.split_once(':')?;
    if source_id.is_empty()
        || !source_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        || encoded.is_empty()
    {
        return None;
    }
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut input = encoded.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = (input.next()? as char).to_digit(16)?;
            let low = (input.next()? as char).to_digit(16)?;
            bytes.push((high * 16 + low) as u8);
        } else {
            bytes.push(byte);
        }
    }
    Some((source_id.to_owned(), String::from_utf8(bytes).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_queue_ids_isolate_sources_and_opaque_remote_ids() {
        let first = streaming_preview_queue_id("fixture", "42:/夜");
        let second = streaming_preview_queue_id("other", "42:/夜");
        assert_ne!(first, second);
        assert_eq!(first, "stream:fixture:42%3A%2F%E5%A4%9C");
        assert_eq!(
            streaming_preview_reference(&first),
            Some(("fixture".into(), "42:/夜".into()))
        );
        assert_eq!(streaming_preview_reference("stream:fixture:%ZZ"), None);
        assert_eq!(streaming_preview_reference("ne:42"), None);
        assert_eq!(streaming_preview_reference("deadbeef"), None);
    }

    #[test]
    fn video_queue_ids_use_prefix() {
        assert_eq!(youtube_queue_id("abc"), "yt:abc");
        assert!(is_video_source_queue_id("yt:abc"));
        assert!(!is_video_source_queue_id("deadbeef"));
    }

    #[test]
    fn conflict_prompt_json_omits_hash() {
        let prompt = ImportConflictPrompt {
            source_id: "netease".to_owned(),
            remote_track_id: "1".to_owned(),
            library: LibraryDecisionMeta {
                title: Some("A".to_owned()),
                artist: Some("B".to_owned()),
                album: Some("C".to_owned()),
                format: "MP3".to_owned(),
                bit_rate_bps: Some(192),
                duration_ms: Some(1000),
                file_size_bytes: 10,
            },
            incoming: LibraryDecisionMeta {
                title: Some("A".to_owned()),
                artist: Some("B".to_owned()),
                album: Some("C".to_owned()),
                format: "MP3".to_owned(),
                bit_rate_bps: Some(320),
                duration_ms: Some(1000),
                file_size_bytes: 20,
            },
        };
        let json = serde_json::to_string(&prompt).expect("json");
        assert!(!json.contains("hash"));
        assert!(json.contains("title"));
        assert!(json.contains("artist"));
        assert!(json.contains("album"));
        assert!(json.contains("format"));
        assert!(json.contains("bit_rate_bps"));
        assert!(json.contains("duration_ms"));
        assert!(json.contains("file_size_bytes"));
    }
}
