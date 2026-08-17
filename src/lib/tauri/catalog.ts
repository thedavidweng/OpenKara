import type { CatalogBackend } from "@/lib/backend/types";
import type { InvokeCommand } from "./invoke";

export function createCatalogCommands(invoke: InvokeCommand): CatalogBackend {
  return {
    getStreamingSession: (sourceId) =>
      invoke("get_streaming_session", { source_id: sourceId }),
    startStreamingQrSignin: (sourceId) =>
      invoke("start_streaming_qr_signin", { source_id: sourceId }),
    pollStreamingQrSignin: (sourceId, key) =>
      invoke("poll_streaming_qr_signin", { source_id: sourceId, key }),
    signInStreamingSource: (
      sourceId,
      method,
      identifier,
      password,
      countryCode,
    ) =>
      invoke("sign_in_streaming_source", {
        source_id: sourceId,
        method,
        identifier,
        password,
        country_code: countryCode ?? null,
      }),
    signOutStreamingSource: (sourceId) =>
      invoke("sign_out_streaming_source", { source_id: sourceId }),
    listStreamingLikedTracks: (sourceId) =>
      invoke("list_streaming_liked_tracks", { source_id: sourceId }),
    listStreamingPlaylists: (sourceId) =>
      invoke("list_streaming_playlists", { source_id: sourceId }),
    getStreamingPlaylist: (sourceId, remotePlaylistId) =>
      invoke("get_streaming_playlist", {
        source_id: sourceId,
        remote_playlist_id: remotePlaylistId,
      }),
    searchStreamingSource: (sourceId, query) =>
      invoke("search_streaming_source", { source_id: sourceId, query }),
    startStreamingImport: (sourceId, remoteTrackIds, remotePlaylistId) =>
      invoke("start_streaming_import", {
        source_id: sourceId,
        remote_track_ids: remoteTrackIds,
        remote_playlist_id: remotePlaylistId ?? null,
      }),
    continueStreamingImport: (action) =>
      invoke("continue_streaming_import", { action }),
    resolveVideoSourceUrl: (sourceId, url) =>
      invoke("resolve_video_source_url", { source_id: sourceId, url }),
    getRevealTargets: (songId) =>
      invoke("get_reveal_targets", { song_id: songId }),
    revealInFolder: (path) => invoke("reveal_in_folder", { path }),
  };
}
