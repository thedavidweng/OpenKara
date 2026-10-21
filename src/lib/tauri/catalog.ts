import type { CatalogBackend } from "@/lib/backend/types";
import type { InvokeCommand } from "./invoke";

export function createCatalogCommands(invoke: InvokeCommand): CatalogBackend {
  return {
    listStreamingSources: () => invoke("list_streaming_sources"),
    refreshStreamingSources: () => invoke("refresh_streaming_sources"),
    installStreamingSource: (sourceId, version = null) =>
      invoke("install_streaming_source", { sourceId, version }),
    cancelStreamingSourceInstall: (sourceId) =>
      invoke("cancel_streaming_source_install", { sourceId }),
    setStreamingSourceEnabled: (sourceId, enabled, stopWork = false) =>
      invoke("set_streaming_source_enabled", { sourceId, enabled, stopWork }),
    uninstallStreamingSource: (sourceId, stopWork, removeCredentials) =>
      invoke("uninstall_streaming_source", {
        sourceId,
        stopWork,
        removeCredentials,
      }),
    pinStreamingSource: (sourceId, version = null) =>
      invoke("pin_streaming_source", { sourceId, version }),
    getStreamingSession: (sourceId) =>
      invoke("get_streaming_session", { sourceId }),
    openStreamingSignIn: (sourceId) =>
      invoke("open_streaming_sign_in", { sourceId }),
    pollStreamingSignIn: (sourceId, challenge) =>
      invoke("poll_streaming_sign_in", { sourceId, challenge }),
    cancelStreamingSignIn: (sourceId, challenge) =>
      invoke("cancel_streaming_sign_in", { sourceId, challenge }),
    signOutStreamingSource: (sourceId, stopWork = false) =>
      invoke("sign_out_streaming_source", { sourceId, stopWork }),
    listStreamingLikedTracks: (sourceId, cursor = null) =>
      invoke("list_streaming_liked_tracks", { sourceId, cursor }),
    getStreamingTracks: (sourceId, remoteTrackIds) =>
      invoke("get_streaming_tracks", { sourceId, remoteTrackIds }),
    listStreamingPlaylists: (sourceId, cursor = null) =>
      invoke("list_streaming_playlists", { sourceId, cursor }),
    getStreamingPlaylist: (sourceId, remotePlaylistId, cursor = null) =>
      invoke("get_streaming_playlist", {
        sourceId,
        remotePlaylistId,
        cursor,
      }),
    searchStreamingSource: (sourceId, query, cursor = null) =>
      invoke("search_streaming_source", { sourceId, query, cursor }),
    startStreamingImport: (sourceId, remoteTrackIds, remotePlaylistId, jobId) =>
      invoke("start_streaming_import", {
        sourceId,
        remoteTrackIds,
        remotePlaylistId: remotePlaylistId ?? null,
        jobId,
      }),
    continueStreamingImport: (action) =>
      invoke("continue_streaming_import", { action }),
    cancelStreamingImport: () => invoke("cancel_streaming_import"),
    getStreamingImportStatus: () => invoke("get_streaming_import_status"),
    resolveVideoSourceUrl: (sourceId, url) =>
      invoke("resolve_video_source_url", { sourceId, url }),
    getRevealTargets: (songId) => invoke("get_reveal_targets", { songId }),
    revealInFolder: (path) => invoke("reveal_in_folder", { path }),
  };
}
