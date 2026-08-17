mod credentials;
mod identity;
mod import;
mod netease;
mod registry;
mod reveal;
mod streaming;
mod types;
mod video;
mod youtube;

pub use credentials::load_session_snapshot;
pub use identity::{lookup_playlist_id, lookup_song_hash, StreamingTrackIdentity};
pub use import::{
    advance_import_session, items_from_tracks, run_streaming_import, StreamingImportItem,
    StreamingImportRequest, StreamingImportSession, StreamingPlaylistOrigin,
};
pub use netease::{china_client_address, LiveNeteaseHttp, NeteaseStreamingSource};
pub use registry::{
    list_online_sources, require_enabled, set_online_source_enabled, OnlineSourceKind,
    OnlineSourceSnapshot, UnknownOnlineSource,
};
pub use reveal::{get_reveal_targets, reveal_path};
pub use streaming::{
    FakeResolveSpec, FakeStreamingSource, FakeTrackSpec, GatedStreamingSource, StreamingSource,
};
pub use types::{
    is_video_source_queue_id, youtube_queue_id, CatalogError, ImportConflictPrompt,
    ImportRefusalReason, LibraryDecisionAction, OnlineSourceCapabilities, RevealTargets,
    StreamingImportProgress, StreamingImportStatus, StreamingPasswordMethod,
    StreamingPlaylistDetail, StreamingPlaylistSummary, StreamingQrChallenge, StreamingQrPoll,
    StreamingSessionSnapshot, StreamingTrack, VideoQueueItem, VideoUnavailableReason,
    NETEASE_SOURCE_ID, YOUTUBE_SOURCE_ID,
};
pub use video::{FakeVideoPage, FakeVideoSource, GatedVideoSource, VideoSource};
pub use youtube::{parse_youtube_url, LiveYoutubeFetcher, YoutubeVideoSource};
