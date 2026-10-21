mod credentials;
mod identity;
mod import;
mod job;
mod plugin;
#[cfg(test)]
mod plugin_tests;
mod registry;
mod reveal;
mod streaming;
mod types;
mod video;
mod youtube;

pub use credentials::{clear_session, load_credentials, store_session};
pub use identity::{
    annotate_library_songs, lookup_playlist_id, lookup_song_hash, StreamingTrackIdentity,
};
pub use import::{
    advance_import_session, items_from_tracks, run_streaming_import, StreamingImportItem,
    StreamingImportRequest, StreamingImportSession, StreamingPlaylistOrigin,
};
pub use job::StreamingImportJob;
pub use plugin::{PluginStreamingSource, TrackMetadataCache};
pub use registry::{
    list_online_sources, require_enabled, set_online_source_enabled, OnlineSourceKind,
    OnlineSourceSnapshot, UnknownOnlineSource,
};
pub use reveal::{get_reveal_targets, reveal_path};
pub use streaming::{
    FakeResolveSpec, FakeStreamingSource, FakeTrackSpec, GatedStreamingSource, NoResolveProgress,
    ResolveProgress, StreamingSource,
};
pub use types::StreamingCredentials;
pub use types::{
    is_video_source_queue_id, streaming_preview_queue_id, streaming_preview_reference,
    youtube_queue_id, CatalogError, ImportConflictPrompt, ImportRefusalReason,
    LibraryDecisionAction, OnlineSourceCapabilities, RevealTargets, StreamingImportEvent,
    StreamingImportJobPhase, StreamingImportJobSnapshot, StreamingImportProgress,
    StreamingImportStatus, StreamingImportTrackState, StreamingPlaylistDetail,
    StreamingPlaylistListing, StreamingPlaylistSummary, StreamingPreviewOutcome,
    StreamingSessionSnapshot, StreamingTrack, StreamingTrackListing, VideoQueueItem,
    VideoUnavailableReason, YOUTUBE_SOURCE_ID,
};
pub use video::{FakeVideoPage, FakeVideoSource, GatedVideoSource, VideoSource};
pub use youtube::{parse_youtube_url, LiveYoutubeFetcher, YoutubeVideoSource};
