import { create } from "zustand";
import { tauriBackend, type Backend } from "@/lib/backend";
import { notifyError } from "@/lib/errors";
import type {
  ImportConflictPrompt,
  LibraryDecisionAction,
  StreamingImportFailure,
  StreamingPlaylistDetail,
  StreamingPlaylistSummary,
  StreamingSessionSnapshot,
  StreamingTrack,
  VideoQueueItem,
} from "@/types/ipc";

type CatalogView = "library" | "netease";

interface CatalogState {
  activeView: CatalogView;
  session: StreamingSessionSnapshot | null;
  liked: StreamingTrack[];
  playlists: StreamingPlaylistSummary[];
  playlistDetail: StreamingPlaylistDetail | null;
  searchResults: StreamingTrack[];
  importFailures: StreamingImportFailure[];
  pendingConflict: ImportConflictPrompt | null;
  videoItems: Record<string, VideoQueueItem>;
  setActiveView: (view: CatalogView) => void;
  rememberVideoItems: (items: VideoQueueItem[]) => void;
  getVideoItem: (id: string) => VideoQueueItem | undefined;
  loadSession: () => Promise<void>;
  openSignIn: () => Promise<void>;
  completeSignIn: () => Promise<void>;
  signOut: () => Promise<void>;
  loadLiked: () => Promise<void>;
  loadPlaylists: () => Promise<void>;
  openPlaylist: (remoteId: string) => Promise<void>;
  search: (query: string) => Promise<void>;
  importTracks: (
    remoteTrackIds: string[],
    remotePlaylistId?: string | null,
  ) => Promise<void>;
  resolveConflict: (action: LibraryDecisionAction) => Promise<void>;
}

export function createCatalogStore(backend: Backend = tauriBackend) {
  const reportError = (error: unknown) => {
    if (
      typeof error === "object" &&
      error !== null &&
      "code" in error &&
      error.code === "streaming_session_expired"
    ) {
      store.setState({
        session: {
          source_id: "netease",
          signed_in: false,
          display_name: null,
          expired: true,
        },
        liked: [],
        playlists: [],
        playlistDetail: null,
        searchResults: [],
      });
    }
    notifyError(error);
  };
  const store = create<CatalogState>((set, get) => ({
    activeView: "library",
    session: null,
    liked: [],
    playlists: [],
    playlistDetail: null,
    searchResults: [],
    importFailures: [],
    pendingConflict: null,
    videoItems: {},

    setActiveView: (view) => set({ activeView: view }),

    rememberVideoItems: (items) => {
      const next = { ...get().videoItems };
      for (const item of items) {
        next[item.id] = item;
      }
      set({ videoItems: next });
    },

    getVideoItem: (id) => get().videoItems[id],

    loadSession: async () => {
      try {
        const session = await backend.catalog.getStreamingSession("netease");
        set({ session });
      } catch (error) {
        reportError(error);
      }
    },

    openSignIn: async () => {
      try {
        await backend.catalog.openStreamingSignIn("netease");
      } catch (error) {
        reportError(error);
      }
    },
    completeSignIn: async () => {
      try {
        const session =
          await backend.catalog.completeStreamingSignIn("netease");
        set({
          session,
          liked: [],
          playlists: [],
          playlistDetail: null,
          searchResults: [],
        });
      } catch (error) {
        reportError(error);
      }
    },

    signOut: async () => {
      try {
        const session = await backend.catalog.signOutStreamingSource("netease");
        set({
          session,
          liked: [],
          playlists: [],
          playlistDetail: null,
          searchResults: [],
        });
      } catch (error) {
        reportError(error);
      }
    },

    loadLiked: async () => {
      try {
        const liked = await backend.catalog.listStreamingLikedTracks("netease");
        set({ liked, playlistDetail: null });
      } catch (error) {
        reportError(error);
      }
    },

    loadPlaylists: async () => {
      try {
        const playlists =
          await backend.catalog.listStreamingPlaylists("netease");
        set({ playlists });
      } catch (error) {
        reportError(error);
      }
    },

    openPlaylist: async (remoteId) => {
      try {
        const playlistDetail = await backend.catalog.getStreamingPlaylist(
          "netease",
          remoteId,
        );
        set({ playlistDetail });
      } catch (error) {
        reportError(error);
      }
    },

    search: async (query) => {
      try {
        const searchResults = await backend.catalog.searchStreamingSource(
          "netease",
          query,
        );
        set({ searchResults, playlistDetail: null });
      } catch (error) {
        reportError(error);
      }
    },

    importTracks: async (remoteTrackIds, remotePlaylistId) => {
      try {
        const progress = await backend.catalog.startStreamingImport(
          "netease",
          remoteTrackIds,
          remotePlaylistId,
        );
        set({
          importFailures: progress.failed,
          pendingConflict: progress.conflict,
        });
      } catch (error) {
        reportError(error);
      }
    },

    resolveConflict: async (action) => {
      try {
        const progress = await backend.catalog.continueStreamingImport(action);
        set({
          importFailures: progress.failed,
          pendingConflict: progress.conflict,
        });
      } catch (error) {
        reportError(error);
      }
    },
  }));

  return store;
}

export const useCatalogStore = createCatalogStore();
