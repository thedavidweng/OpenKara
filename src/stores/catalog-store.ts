import { create } from "zustand";
import { tauriBackend, type Backend } from "@/lib/backend";
import { notifyError } from "@/lib/errors";
import type {
  ImportConflictPrompt,
  LibraryDecisionAction,
  StreamingImportFailure,
  StreamingPlaylistDetail,
  StreamingPlaylistSummary,
  StreamingQrChallenge,
  StreamingQrStatus,
  StreamingSessionSnapshot,
  StreamingTrack,
  VideoQueueItem,
} from "@/types/ipc";

type CatalogView = "library" | "netease";

interface CatalogState {
  activeView: CatalogView;
  session: StreamingSessionSnapshot | null;
  qr: StreamingQrChallenge | null;
  qrStatus: StreamingQrStatus | null;
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
  startQr: () => Promise<void>;
  pollQr: () => Promise<void>;
  signInPassword: (
    method: "phone" | "email",
    identifier: string,
    password: string,
    countryCode?: string,
  ) => Promise<void>;
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
  const store = create<CatalogState>((set, get) => ({
    activeView: "library",
    session: null,
    qr: null,
    qrStatus: null,
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
        notifyError(error);
      }
    },

    startQr: async () => {
      try {
        const qr = await backend.catalog.startStreamingQrSignin("netease");
        set({ qr, qrStatus: "waiting" });
      } catch (error) {
        notifyError(error);
      }
    },

    pollQr: async () => {
      const key = get().qr?.key;
      if (!key) return;
      try {
        const poll = await backend.catalog.pollStreamingQrSignin(
          "netease",
          key,
        );
        if (poll.session) {
          set({
            session: poll.session,
            qr: null,
            qrStatus: null,
            liked: [],
            playlists: [],
            playlistDetail: null,
            searchResults: [],
          });
          return;
        }
        set({ qrStatus: poll.status });
      } catch (error) {
        notifyError(error);
      }
    },

    signInPassword: async (method, identifier, password, countryCode) => {
      try {
        const session = await backend.catalog.signInStreamingSource(
          "netease",
          method,
          identifier,
          password,
          countryCode,
        );
        set({
          session,
          qr: null,
          qrStatus: null,
          liked: [],
          playlists: [],
          playlistDetail: null,
          searchResults: [],
        });
      } catch (error) {
        notifyError(error);
      }
    },

    signOut: async () => {
      try {
        const session = await backend.catalog.signOutStreamingSource("netease");
        set({
          session,
          qr: null,
          qrStatus: null,
          liked: [],
          playlists: [],
          playlistDetail: null,
          searchResults: [],
        });
      } catch (error) {
        notifyError(error);
      }
    },

    loadLiked: async () => {
      try {
        const liked = await backend.catalog.listStreamingLikedTracks("netease");
        set({ liked, playlistDetail: null });
      } catch (error) {
        notifyError(error);
      }
    },

    loadPlaylists: async () => {
      try {
        const playlists =
          await backend.catalog.listStreamingPlaylists("netease");
        set({ playlists });
      } catch (error) {
        notifyError(error);
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
        notifyError(error);
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
        notifyError(error);
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
        notifyError(error);
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
        notifyError(error);
      }
    },
  }));

  return store;
}

export const useCatalogStore = createCatalogStore();
