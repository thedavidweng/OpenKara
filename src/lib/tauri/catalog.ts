import type { CatalogBackend } from "@/lib/backend/types";
import type { InvokeCommand } from "./invoke";

export function createCatalogCommands(invoke: InvokeCommand): CatalogBackend {
  return {
    getStreamingSession: (sourceId) =>
      invoke("get_streaming_session", { sourceId }),
    startStreamingQrSignin: (sourceId) =>
      invoke("start_streaming_qr_signin", { sourceId }),
    pollStreamingQrSignin: (sourceId, key) =>
      invoke("poll_streaming_qr_signin", { sourceId, key }),
    signInStreamingSource: (
      sourceId,
      method,
      identifier,
      password,
      countryCode,
    ) =>
      invoke("sign_in_streaming_source", {
        sourceId,
        method,
        identifier,
        password,
        countryCode: countryCode ?? null,
      }),
    signOutStreamingSource: (sourceId) =>
      invoke("sign_out_streaming_source", { sourceId }),
    listStreamingLikedTracks: (sourceId) =>
      invoke("list_streaming_liked_tracks", { sourceId }),
    listStreamingPlaylists: (sourceId) =>
      invoke("list_streaming_playlists", { sourceId }),
    getStreamingPlaylist: (sourceId, remotePlaylistId) =>
      invoke("get_streaming_playlist", {
        sourceId,
        remotePlaylistId,
      }),
    searchStreamingSource: (sourceId, query) =>
      invoke("search_streaming_source", { sourceId, query }),
    startStreamingImport: (sourceId, remoteTrackIds, remotePlaylistId) =>
      invoke("start_streaming_import", {
        sourceId,
        remoteTrackIds,
        remotePlaylistId: remotePlaylistId ?? null,
      }),
    continueStreamingImport: (action) =>
      invoke("continue_streaming_import", { action }),
    resolveVideoSourceUrl: (sourceId, url) =>
      invoke("resolve_video_source_url", { sourceId, url }),
    getRevealTargets: (songId) => invoke("get_reveal_targets", { songId }),
    revealInFolder: (path) => invoke("reveal_in_folder", { path }),
  };
}
