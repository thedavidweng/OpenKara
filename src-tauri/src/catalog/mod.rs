mod credentials;
mod identity;
mod import;
mod job;
mod netease;
mod registry;
mod reveal;
mod streaming;
mod types;
mod video;
mod youtube;

pub use credentials::load_session_snapshot;
pub use identity::{
    annotate_library_songs, lookup_playlist_id, lookup_song_hash, StreamingTrackIdentity,
};
pub use import::{
    advance_import_session, items_from_tracks, run_streaming_import, StreamingImportItem,
    StreamingImportRequest, StreamingImportSession, StreamingPlaylistOrigin,
};
pub use job::StreamingImportJob;
pub use netease::{LiveNeteaseApi, NeteaseSidecar, NeteaseSongCache, NeteaseStreamingSource};
pub use registry::{
    list_online_sources, require_enabled, set_online_source_enabled, OnlineSourceKind,
    OnlineSourceSnapshot, UnknownOnlineSource,
};
pub use reveal::{get_reveal_targets, reveal_path};
pub use streaming::{
    FakeResolveSpec, FakeStreamingSource, FakeTrackSpec, GatedStreamingSource, NoResolveProgress,
    ResolveProgress, StreamingSource,
};
pub use types::{
    is_video_source_queue_id, netease_preview_queue_id, netease_preview_remote_id,
    youtube_queue_id, CatalogError, ImportConflictPrompt, ImportRefusalReason,
    LibraryDecisionAction, OnlineSourceCapabilities, RevealTargets, StreamingImportEvent,
    StreamingImportJobPhase, StreamingImportJobSnapshot, StreamingImportProgress,
    StreamingImportStatus, StreamingImportTrackState, StreamingPlaylistDetail,
    StreamingPlaylistSummary, StreamingPreviewOutcome, StreamingSessionSnapshot, StreamingTrack,
    StreamingTrackListing, VideoQueueItem, VideoUnavailableReason, NETEASE_SOURCE_ID,
    YOUTUBE_SOURCE_ID,
};
pub use video::{FakeVideoPage, FakeVideoSource, GatedVideoSource, VideoSource};
pub use youtube::{parse_youtube_url, LiveYoutubeFetcher, YoutubeVideoSource};
