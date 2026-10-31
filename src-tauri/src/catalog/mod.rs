#[cfg(feature = "streaming-sources")]
mod credentials;
#[cfg(feature = "streaming-sources")]
mod identity;
#[cfg(feature = "streaming-sources")]
mod import;
#[cfg(feature = "streaming-sources")]
mod job;
#[cfg(feature = "streaming-sources")]
mod plugin;
#[cfg(all(test, feature = "streaming-sources"))]
mod plugin_tests;
mod registry;
mod reveal;
#[cfg(feature = "streaming-sources")]
mod streaming;
mod types;
mod video;
mod youtube;

#[cfg(all(test, feature = "streaming-sources"))]
pub(crate) use credentials::store_legacy_session;
#[cfg(feature = "streaming-sources")]
pub use credentials::{clear_retired_sessions, clear_session, load_credentials, store_session};
#[cfg(feature = "streaming-sources")]
pub use identity::{
    annotate_library_songs, lookup_playlist_id, lookup_song_hash, StreamingTrackIdentity,
};
#[cfg(feature = "streaming-sources")]
pub use import::{
    advance_import_session, items_from_tracks, run_streaming_import, StreamingImportItem,
    StreamingImportRequest, StreamingImportSession, StreamingPlaylistOrigin,
};
#[cfg(feature = "streaming-sources")]
pub use job::StreamingImportJob;
#[cfg(feature = "streaming-sources")]
pub use plugin::{PluginStreamingSource, TrackMetadataCache};
pub use registry::{
    list_online_sources, require_enabled, set_online_source_enabled, OnlineSourceKind,
    OnlineSourceSnapshot, UnknownOnlineSource,
};
pub use reveal::{get_reveal_targets, reveal_path};
#[cfg(feature = "streaming-sources")]
pub use streaming::{
    FakeResolveSpec, FakeStreamingSource, FakeTrackSpec, GatedStreamingSource, NoResolveProgress,
    ResolveProgress, StreamingSource,
};
pub use types::StreamingCredentials;
#[cfg(feature = "streaming-sources")]
pub use types::StreamingPreviewOutcome;
pub use types::{
    is_video_source_queue_id, streaming_preview_queue_id, streaming_preview_reference,
    youtube_queue_id, CatalogError, ImportConflictPrompt, ImportRefusalReason,
    LibraryDecisionAction, OnlineSourceCapabilities, RevealTargets, StreamingImportEvent,
    StreamingImportJobPhase, StreamingImportJobSnapshot, StreamingImportProgress,
    StreamingImportStatus, StreamingImportTrackState, StreamingPlaylistDetail,
    StreamingPlaylistListing, StreamingPlaylistSummary, StreamingSessionSnapshot, StreamingTrack,
    StreamingTrackListing, VideoQueueItem, VideoUnavailableReason, YOUTUBE_SOURCE_ID,
};
pub use video::{FakeVideoPage, FakeVideoSource, GatedVideoSource, VideoSource};
pub use youtube::{parse_youtube_url, LiveYoutubeFetcher, YoutubeVideoSource};
