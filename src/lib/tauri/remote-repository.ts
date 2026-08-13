import type { RemoteRepositoryBackend } from "@/lib/backend/types";
import type {
  CacheUsage,
  LibraryRegistrySnapshot,
  RemoteAuthStart,
  RemoteAuthStatus,
  RemoteDiagnostics,
  RemoteLibraryCandidate,
  UploadStatusSnapshot,
} from "@/types/ipc";
import type { InvokeCommand } from "./invoke";

export function createRemoteRepositoryCommands(
  invoke: InvokeCommand,
): RemoteRepositoryBackend {
  return {
    beginRemoteAuth: (provider, payload = null) =>
      invoke<RemoteAuthStart>("begin_remote_auth", { provider, payload }),

    pollRemoteAuth: (sessionId) =>
      invoke<RemoteAuthStatus>("poll_remote_auth", { sessionId }),

    cancelRemoteAuth: (sessionId) =>
      invoke<void>("cancel_remote_auth", { sessionId }),

    openExternalUrl: (url) => invoke<void>("open_external_url", { url }),

    listRemoteLibraryRoots: (sessionId) =>
      invoke<RemoteLibraryCandidate[]>("list_remote_library_roots", {
        sessionId,
      }),

    createRemoteLibrary: (sessionId, displayName) =>
      invoke<RemoteLibraryCandidate>("create_remote_library", {
        sessionId,
        displayName,
      }),

    resolveRemoteLibraryCandidate: (sessionId, displayName) =>
      invoke<RemoteLibraryCandidate>("resolve_remote_library_candidate", {
        sessionId,
        displayName,
      }),

    registerRemoteLibrary: (sessionId, remoteRootLocator, displayName) =>
      invoke<LibraryRegistrySnapshot>("register_remote_library", {
        sessionId,
        remoteRootLocator,
        displayName: displayName ?? null,
      }),

    reauthorizeRemoteRepository: (
      libraryId,
      sessionId,
      remoteRootLocator,
      displayName,
    ) =>
      invoke<LibraryRegistrySnapshot>("reauthorize_remote_repository", {
        libraryId,
        sessionId,
        remoteRootLocator,
        displayName,
      }),

    relocateRemoteRepository: (
      libraryId,
      sessionId,
      remoteRootLocator,
      displayName,
    ) =>
      invoke<LibraryRegistrySnapshot>("relocate_remote_repository", {
        libraryId,
        sessionId,
        remoteRootLocator,
        displayName,
      }),

    mirrorLocalLibraryToRemote: (localLibraryId, remoteLibraryId) =>
      invoke<void>("mirror_local_library_to_remote", {
        localLibraryId,
        remoteLibraryId,
      }),

    refreshRemoteRepository: () => invoke<void>("refresh_remote_repository"),

    publishSongToRemote: (songId) =>
      invoke<unknown>("publish_song_to_remote", { songId }),

    publishSongsToRemote: (songIds) =>
      invoke<unknown>("publish_songs_to_remote", { songIds }),

    getAllUploadStatuses: () =>
      invoke<UploadStatusSnapshot[]>("get_all_upload_statuses"),

    getRemoteCacheUsage: () => invoke<CacheUsage>("get_remote_cache_usage"),

    clearRemoteCache: () => invoke<number>("clear_remote_cache"),

    resolveRemoteConflict: (resolution) =>
      invoke<void>("resolve_remote_conflict", { resolution }),

    getRemoteDiagnostics: () =>
      invoke<RemoteDiagnostics>("get_remote_diagnostics"),
  };
}
