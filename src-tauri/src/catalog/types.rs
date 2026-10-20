use crate::commands::error::{
    internal_error, online_source_disabled, streaming_auth_failed, streaming_session_expired,
    video_source_unavailable, CommandError,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const YOUTUBE_SOURCE_ID: &str = "youtube";
pub const NETEASE_SOURCE_ID: &str = "netease";
pub const YOUTUBE_QUEUE_PREFIX: &str = "yt:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    UnknownSource { source_id: String },
    SourceDisabled { source_id: String },
    AuthFailed { source_id: String, detail: String },
    SessionExpired { source_id: String },
    Network(String),
    Cancelled,
    VideoUnavailable { reason: VideoUnavailableReason },
    Internal(String),
}

impl From<CatalogError> for CommandError {
    fn from(error: CatalogError) -> Self {
        match error {
            CatalogError::UnknownSource { source_id } => {
                internal_error(format!("unknown online source: {source_id}"))
            }
            CatalogError::SourceDisabled { source_id } => {
                online_source_disabled(format!("online source {source_id} is off"))
            }
            CatalogError::AuthFailed { source_id, detail } => {
                streaming_auth_failed(format!("sign-in for {source_id} failed: {detail}"))
            }
            CatalogError::SessionExpired { source_id } => {
                streaming_session_expired(format!("streaming session for {source_id} expired"))
            }
            CatalogError::Network(message) => CommandError::new(
                crate::commands::error::ErrorCode::NetworkUnavailable,
                message,
                true,
                crate::commands::error::FallbackAction::Retry,
            ),
            CatalogError::Cancelled => internal_error("streaming import was cancelled"),
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
            NETEASE_SOURCE_ID => Self {
                sign_in: true,
                browse: true,
                import: true,
                resolve_video: false,
            },
            _ => Self::none(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingCredentials {
    pub music_u: String,
    pub csrf: String,
}

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
    /// HTTPS album cover URL (unsized; the UI appends a size parameter).
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingPlaylistSummary {
    pub remote_id: String,
    pub name: String,
    pub track_count: u32,
    /// HTTPS playlist cover URL (unsized).
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingPlaylistDetail {
    pub remote_id: String,
    pub name: String,
    pub tracks: Vec<StreamingTrack>,
    pub cover_url: Option<String>,
    pub creator: Option<String>,
    pub description: Option<String>,
    pub track_count: u32,
    /// Last update time in milliseconds since the Unix epoch, when known.
    pub updated_at_ms: Option<u64>,
    /// Every track id of the playlist, in order. Tracks not yet in `tracks`
    /// are fetched by id with `get_streaming_tracks`.
    pub track_ids: Vec<String>,
    /// True when `tracks` holds only the first batch of `track_ids`.
    pub partial: bool,
}

/// A track listing whose first batch is loaded and the rest is fetched by id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingTrackListing {
    pub tracks: Vec<StreamingTrack>,
    pub track_ids: Vec<String>,
    pub partial: bool,
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

/// A direct audio URL a track can be previewed from, without importing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStreamingUrl {
    pub url: String,
    /// Container extension (`mp3`, `flac`, ...), used as the decoder hint.
    pub extension: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

const NETEASE_PREVIEW_PREFIX: &str = "ne:";

/// Queue id of a NetEase track played as a preview, without being imported.
pub fn netease_preview_queue_id(remote_track_id: &str) -> String {
    format!("{NETEASE_PREVIEW_PREFIX}{remote_track_id}")
}

/// The NetEase track id behind a preview queue id.
pub fn netease_preview_remote_id(queue_id: &str) -> Option<&str> {
    queue_id
        .strip_prefix(NETEASE_PREVIEW_PREFIX)
        .filter(|id| !id.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_queue_ids_use_prefix() {
        assert_eq!(netease_preview_queue_id("42"), "ne:42");
        assert_eq!(netease_preview_remote_id("ne:42"), Some("42"));
        assert_eq!(netease_preview_remote_id("ne:"), None);
        assert_eq!(netease_preview_remote_id("yt:42"), None);
        assert_eq!(netease_preview_remote_id("deadbeef"), None);
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
