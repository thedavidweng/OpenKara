import { create } from "zustand";
import { tauriBackend, type Backend } from "@/lib/backend";
import { notifyError } from "@/lib/errors";
import { onlinePreviewQueueId } from "@/playback";
import type {
  CommandError,
  ImportConflictPrompt,
  LibraryDecisionAction,
  PreviewQueueItem,
  StreamingImportEvent,
  StreamingImportFailure,
  StreamingImportProgress,
  StreamingPlaylistDetail,
  StreamingPlaylistSummary,
  StreamingImportTrackState,
  StreamingSessionSnapshot,
  StreamingTrack,
  VideoQueueItem,
} from "@/types/ipc";

type CatalogView = "library" | "netease";
export type NeteaseTab = "liked" | "playlists" | "results";
export type LoadStatus = "idle" | "loading" | "ready" | "error";

/** The most recent Streaming Import. While `running`, `completed` follows the
 * backend's per-track progress events. */
export interface ImportRun {
  jobId: string | null;
  phase: "running" | "done";
  total: number;
  completed: number;
  imported: number;
  skipped: number;
  /** Playlist or track name shown in the progress bar; null for a bare batch. */
  label: string | null;
  cancelling: boolean;
}

interface CatalogState {
  activeView: CatalogView;
  session: StreamingSessionSnapshot | null;
  liked: StreamingTrack[];
  playlists: StreamingPlaylistSummary[];
  playlistDetail: StreamingPlaylistDetail | null;
  /** Loaded playlist details by remote id, so reopening one is instant. */
  playlistDetails: Record<string, StreamingPlaylistDetail>;
  /** Hide tracks the account cannot import (default on). */
  hideUnavailable: boolean;
  searchResults: StreamingTrack[];
  importFailures: StreamingImportFailure[];
  pendingConflict: ImportConflictPrompt | null;
  videoItems: Record<string, VideoQueueItem>;
  /** NetEase tracks played as previews, by `ne:` queue id. */
  previewItems: Record<string, PreviewQueueItem>;
  browserTab: NeteaseTab;
  searchQuery: string;
  likedStatus: LoadStatus;
  /** True while the rest of the liked list is still loading by id. */
  likedLoadingMore: boolean;
  playlistsStatus: LoadStatus;
  searchStatus: LoadStatus;
  playlistDetailStatus: LoadStatus;
  selectedPlaylistId: string | null;
  /** Latest import state per remote track id. */
  trackStates: Record<string, StreamingImportTrackState>;
  importRun: ImportRun | null;
  activeImportIds: string[];
  signInError: CommandError | null;
  youtubeLinkOpen: boolean;
  setActiveView: (view: CatalogView) => void;
  setBrowserTab: (tab: NeteaseTab) => void;
  setHideUnavailable: (hide: boolean) => void;
  closePlaylist: () => void;
  clearSearch: () => void;
  dismissImportRun: () => void;
  setSignInError: (error: CommandError) => void;
  clearSignInError: () => void;
  setYoutubeLinkOpen: (open: boolean) => void;
  rememberVideoItems: (items: VideoQueueItem[]) => void;
  getVideoItem: (id: string) => VideoQueueItem | undefined;
  rememberPreviewTracks: (tracks: StreamingTrack[]) => void;
  loadSession: () => Promise<void>;
  openSignIn: () => Promise<void>;
  acceptSignIn: (session: StreamingSessionSnapshot) => void;
  signOut: () => Promise<void>;
  loadLiked: () => Promise<void>;
  loadPlaylists: () => Promise<void>;
  openPlaylist: (remoteId: string) => Promise<void>;
  search: (query: string) => Promise<void>;
  importTracks: (
    remoteTrackIds: string[],
    remotePlaylistId?: string | null,
    label?: string | null,
  ) => Promise<void>;
  resolveConflict: (action: LibraryDecisionAction) => Promise<void>;
  applyImportEvent: (event: StreamingImportEvent) => void;
  cancelImport: () => Promise<void>;
  recoverImportJob: () => Promise<void>;
}

const EMPTY_BROWSE_STATE = {
  liked: [] as StreamingTrack[],
  playlists: [] as StreamingPlaylistSummary[],
  playlistDetail: null,
  playlistDetails: {} as Record<string, StreamingPlaylistDetail>,
  searchResults: [] as StreamingTrack[],
  browserTab: "playlists" as NeteaseTab,
  searchQuery: "",
  likedStatus: "idle" as LoadStatus,
  likedLoadingMore: false,
  playlistsStatus: "idle" as LoadStatus,
  searchStatus: "idle" as LoadStatus,
  playlistDetailStatus: "idle" as LoadStatus,
  selectedPlaylistId: null,
  trackStates: {} as Record<string, StreamingImportTrackState>,
  importRun: null as ImportRun | null,
  activeImportIds: [] as string[],
  importFailures: [] as StreamingImportFailure[],
};

/** Ids per follow-up page, and pages in flight at once. */
const TRACK_PAGE_SIZE = 500;
const TRACK_PAGE_CONCURRENCY = 3;

const isTerminal = (state: StreamingImportTrackState | undefined) =>
  state?.kind === "imported" ||
  state?.kind === "skipped" ||
  state?.kind === "failed";

function failureFromState(
  remoteTrackId: string,
  title: string,
  artist: string,
  state: StreamingImportTrackState,
): StreamingImportFailure | null {
  if (state.kind === "skipped") {
    return {
      remote_track_id: remoteTrackId,
      title,
      artist,
      reason: "refusal",
      refusal: state.refusal,
    };
  }
  if (state.kind === "failed") {
    return {
      remote_track_id: remoteTrackId,
      title,
      artist,
      reason: state.reason,
      refusal: null,
    };
  }
  return null;
}

export function createCatalogStore(backend: Backend = tauriBackend) {
  /** Bumped when browse state is reset so in-flight page loads stop. */
  let browseEpoch = 0;

  /**
   * Loads the ids missing from `initial` in pages, a few at a time, and calls
   * `apply` after each page with the loaded tracks in `ids` order. Stops
   * quietly when the browse state is reset.
   */
  const loadRemainingTracks = async (
    ids: string[],
    initial: StreamingTrack[],
    apply: (tracks: StreamingTrack[], done: boolean) => void,
  ) => {
    const epoch = browseEpoch;
    const byId = new Map(
      initial.map((track) => [track.remote_track_id, track]),
    );
    const missing = ids.filter((id) => !byId.has(id));
    const pages: string[][] = [];
    for (let start = 0; start < missing.length; start += TRACK_PAGE_SIZE) {
      pages.push(missing.slice(start, start + TRACK_PAGE_SIZE));
    }
    const ordered = () => ids.flatMap((id) => byId.get(id) ?? []);
    let next = 0;
    const worker = async () => {
      while (epoch === browseEpoch) {
        const page = pages[next++];
        if (!page) return;
        const tracks = await backend.catalog.getStreamingTracks(
          "netease",
          page,
        );
        if (epoch !== browseEpoch) return;
        for (const track of tracks) byId.set(track.remote_track_id, track);
        apply(ordered(), false);
      }
    };
    try {
      await Promise.all(
        Array.from(
          { length: Math.min(TRACK_PAGE_CONCURRENCY, pages.length) },
          worker,
        ),
      );
    } catch (error) {
      if (epoch === browseEpoch) {
        apply(ordered(), true);
        reportError(error);
      }
      return;
    }
    if (epoch === browseEpoch) apply(ordered(), true);
  };

  const reportError = (error: unknown) => {
    if (
      typeof error === "object" &&
      error !== null &&
      "code" in error &&
      error.code === "streaming_session_expired"
    ) {
      browseEpoch += 1;
      store.setState({
        session: {
          source_id: "netease",
          signed_in: false,
          display_name: null,
          expired: true,
        },
        ...EMPTY_BROWSE_STATE,
      });
    }
    notifyError(error);
  };

  const findTrack = (remoteTrackId: string) => {
    const state = store.getState();
    return [
      ...state.liked,
      ...state.searchResults,
      ...(state.playlistDetail?.tracks ?? []),
    ].find((track) => track.remote_track_id === remoteTrackId);
  };

  /** Backend events can carry empty titles (bare id imports); fill from the
   * lists the user is looking at. */
  const describeTrack = (
    remoteTrackId: string,
    title: string,
    artist: string,
  ) => {
    if (title) return { title, artist };
    const known = findTrack(remoteTrackId);
    return { title: known?.title ?? "", artist: known?.artist ?? artist };
  };

  const countRun = (
    ids: string[],
    trackStates: Record<string, StreamingImportTrackState>,
  ) => {
    let imported = 0;
    let skipped = 0;
    for (const id of ids) {
      const kind = trackStates[id]?.kind;
      if (kind === "imported") imported += 1;
      else if (kind === "skipped" || kind === "failed") skipped += 1;
    }
    return { imported, skipped, completed: imported + skipped };
  };

  /** Fold the backend's final response into the run summary. */
  const applyImportProgress = (progress: StreamingImportProgress) => {
    if (progress.status === "awaiting_decision") {
      store.setState({
        importFailures: progress.failed,
        pendingConflict: progress.conflict,
      });
      return;
    }
    const state = store.getState();
    const requested = state.activeImportIds;
    const failed = new Map(progress.failed.map((f) => [f.remote_track_id, f]));
    const trackStates = { ...state.trackStates };
    // Events normally settle every track; cover any the backend did not report.
    for (const id of requested) {
      if (isTerminal(trackStates[id])) continue;
      const failure = failed.get(id);
      trackStates[id] = failure
        ? failure.refusal
          ? { kind: "skipped", refusal: failure.refusal }
          : { kind: "failed", reason: failure.reason }
        : { kind: "imported", song_id: "" };
    }
    const counts = countRun(requested, trackStates);
    store.setState({
      trackStates,
      importFailures: progress.failed,
      pendingConflict: progress.conflict,
      activeImportIds: [],
      importRun: state.importRun
        ? {
            ...state.importRun,
            phase: "done",
            total: requested.length || state.importRun.total,
            completed: counts.completed,
            imported: counts.imported,
            skipped: Math.max(counts.skipped, progress.failed.length),
            cancelling: false,
          }
        : null,
    });
  };

  const failImport = (error: unknown) => {
    const state = store.getState();
    const requested = state.activeImportIds;
    const trackStates = { ...state.trackStates };
    for (const id of requested) {
      if (!isTerminal(trackStates[id])) {
        trackStates[id] = { kind: "failed", reason: "import_failed" };
      }
    }
    const counts = countRun(requested, trackStates);
    store.setState({
      trackStates,
      activeImportIds: [],
      importRun: state.importRun
        ? {
            ...state.importRun,
            phase: "done",
            completed: counts.completed,
            imported: counts.imported,
            skipped: counts.skipped,
            cancelling: false,
          }
        : null,
    });
    reportError(error);
  };
  const store = create<CatalogState>((set, get) => ({
    activeView: "library",
    session: null,
    liked: [],
    playlists: [],
    playlistDetail: null,
    playlistDetails: {},
    hideUnavailable: true,
    searchResults: [],
    importFailures: [],
    pendingConflict: null,
    videoItems: {},
    previewItems: {},
    browserTab: "playlists",
    searchQuery: "",
    likedStatus: "idle",
    likedLoadingMore: false,
    playlistsStatus: "idle",
    searchStatus: "idle",
    playlistDetailStatus: "idle",
    selectedPlaylistId: null,
    trackStates: {},
    importRun: null,
    activeImportIds: [],
    signInError: null,
    youtubeLinkOpen: false,

    setActiveView: (view) => set({ activeView: view }),
    setBrowserTab: (tab) => set({ browserTab: tab }),
    setHideUnavailable: (hide) => set({ hideUnavailable: hide }),
    closePlaylist: () =>
      set({
        playlistDetail: null,
        selectedPlaylistId: null,
        playlistDetailStatus: "idle",
      }),
    clearSearch: () =>
      set((state) => ({
        searchQuery: "",
        searchResults: [],
        searchStatus: "idle",
        browserTab:
          state.browserTab === "results" ? "playlists" : state.browserTab,
      })),
    dismissImportRun: () => {
      if (get().importRun?.phase === "running") return;
      set({ importRun: null, importFailures: [] });
    },
    setSignInError: (error) => set({ signInError: error }),
    clearSignInError: () => set({ signInError: null }),
    setYoutubeLinkOpen: (open) => set({ youtubeLinkOpen: open }),

    rememberVideoItems: (items) => {
      const next = { ...get().videoItems };
      for (const item of items) {
        next[item.id] = item;
      }
      set({ videoItems: next });
    },

    getVideoItem: (id) => get().videoItems[id],

    rememberPreviewTracks: (tracks) => {
      const next = { ...get().previewItems };
      for (const track of tracks) {
        const id = onlinePreviewQueueId(track.remote_track_id);
        next[id] = {
          id,
          remote_track_id: track.remote_track_id,
          title: track.title,
          artist: track.artist,
          album: track.album,
          duration_ms: track.duration_ms,
          cover_url: track.cover_url,
        };
      }
      set({ previewItems: next });
    },

    loadSession: async () => {
      try {
        const session = await backend.catalog.getStreamingSession("netease");
        set({ session });
      } catch (error) {
        reportError(error);
      }
    },

    openSignIn: async () => {
      set({ signInError: null });
      try {
        await backend.catalog.openStreamingSignIn("netease");
      } catch (error) {
        reportError(error);
      }
    },
    acceptSignIn: (session) => {
      browseEpoch += 1;
      set({
        session,
        signInError: null,
        ...EMPTY_BROWSE_STATE,
      });
    },

    signOut: async () => {
      try {
        const session = await backend.catalog.signOutStreamingSource("netease");
        browseEpoch += 1;
        set({ session, ...EMPTY_BROWSE_STATE });
      } catch (error) {
        reportError(error);
      }
    },

    loadLiked: async () => {
      set({ likedStatus: "loading" });
      try {
        const epoch = browseEpoch;
        const listing =
          await backend.catalog.listStreamingLikedTracks("netease");
        if (epoch !== browseEpoch) return;
        set({
          liked: listing.tracks,
          likedStatus: "ready",
          likedLoadingMore: listing.partial,
        });
        if (listing.partial) {
          void loadRemainingTracks(
            listing.track_ids,
            listing.tracks,
            (liked, done) => set({ liked, likedLoadingMore: !done }),
          );
        }
      } catch (error) {
        set({ likedStatus: "error" });
        reportError(error);
      }
    },

    loadPlaylists: async () => {
      set({ playlistsStatus: "loading" });
      try {
        const playlists =
          await backend.catalog.listStreamingPlaylists("netease");
        set({ playlists, playlistsStatus: "ready" });
      } catch (error) {
        set({ playlistsStatus: "error" });
        reportError(error);
      }
    },

    openPlaylist: async (remoteId) => {
      const cached = get().playlistDetails[remoteId];
      if (cached) {
        set({
          selectedPlaylistId: remoteId,
          playlistDetail: cached,
          playlistDetailStatus: "ready",
        });
        return;
      }
      set({
        selectedPlaylistId: remoteId,
        playlistDetail: null,
        playlistDetailStatus: "loading",
      });
      try {
        const playlistDetail = await backend.catalog.getStreamingPlaylist(
          "netease",
          remoteId,
        );
        if (get().selectedPlaylistId !== remoteId) return;
        set((state) => ({
          playlistDetail,
          playlistDetailStatus: "ready",
          playlistDetails: {
            ...state.playlistDetails,
            [remoteId]: playlistDetail,
          },
        }));
        if (playlistDetail.partial) {
          void loadRemainingTracks(
            playlistDetail.track_ids,
            playlistDetail.tracks,
            (tracks, done) =>
              set((state) => {
                const detail = {
                  ...(state.playlistDetails[remoteId] ?? playlistDetail),
                  tracks,
                  partial: !done,
                };
                return {
                  playlistDetails: {
                    ...state.playlistDetails,
                    [remoteId]: detail,
                  },
                  playlistDetail:
                    state.selectedPlaylistId === remoteId
                      ? detail
                      : state.playlistDetail,
                };
              }),
          );
        }
      } catch (error) {
        if (get().selectedPlaylistId === remoteId) {
          set({ playlistDetailStatus: "error" });
        }
        reportError(error);
      }
    },

    search: async (query) => {
      const trimmed = query.trim();
      if (!trimmed) {
        get().clearSearch();
        return;
      }
      set({
        searchQuery: trimmed,
        searchStatus: "loading",
        browserTab: "results",
      });
      try {
        const searchResults = await backend.catalog.searchStreamingSource(
          "netease",
          trimmed,
        );
        if (get().searchQuery !== trimmed) return;
        set({ searchResults, searchStatus: "ready" });
      } catch (error) {
        if (get().searchQuery === trimmed) set({ searchStatus: "error" });
        reportError(error);
      }
    },

    importTracks: async (remoteTrackIds, remotePlaylistId, label) => {
      if (get().importRun?.phase === "running" || remoteTrackIds.length === 0) {
        return;
      }
      const first =
        remoteTrackIds.length === 1 ? findTrack(remoteTrackIds[0]) : undefined;
      set((state) => {
        const trackStates = { ...state.trackStates };
        for (const id of remoteTrackIds) trackStates[id] = { kind: "queued" };
        return {
          trackStates,
          activeImportIds: remoteTrackIds,
          importFailures: [],
          importRun: {
            jobId: null,
            phase: "running",
            total: remoteTrackIds.length,
            completed: 0,
            imported: 0,
            skipped: 0,
            label: label ?? first?.title ?? null,
            cancelling: false,
          },
        };
      });
      try {
        const progress = await backend.catalog.startStreamingImport(
          "netease",
          remoteTrackIds,
          remotePlaylistId,
        );
        applyImportProgress(progress);
      } catch (error) {
        failImport(error);
      }
    },

    resolveConflict: async (action) => {
      set({ pendingConflict: null });
      try {
        const progress = await backend.catalog.continueStreamingImport(action);
        applyImportProgress(progress);
      } catch (error) {
        failImport(error);
      }
    },

    applyImportEvent: (event) => {
      const state = get();
      const run = state.importRun;
      if (!run || run.phase !== "running") return;
      if (run.jobId && run.jobId !== event.job_id) return;
      const trackStates = {
        ...state.trackStates,
        [event.remote_track_id]: event.state,
      };
      const { title, artist } = describeTrack(
        event.remote_track_id,
        event.title,
        event.artist,
      );
      const failure = failureFromState(
        event.remote_track_id,
        title,
        artist,
        event.state,
      );
      const failures = failure
        ? [
            ...state.importFailures.filter(
              (f) => f.remote_track_id !== event.remote_track_id,
            ),
            failure,
          ]
        : state.importFailures;
      const counts = countRun(state.activeImportIds, trackStates);
      set({
        trackStates,
        importFailures: failures,
        importRun: {
          ...run,
          jobId: event.job_id,
          total: event.total,
          completed: event.completed,
          imported: counts.imported,
          skipped: counts.skipped,
        },
      });
    },

    cancelImport: async () => {
      const run = get().importRun;
      if (!run || run.phase !== "running" || run.cancelling) return;
      set({ importRun: { ...run, cancelling: true } });
      try {
        await backend.catalog.cancelStreamingImport();
      } catch (error) {
        reportError(error);
      }
    },

    recoverImportJob: async () => {
      if (get().importRun) return;
      try {
        const snapshot = await backend.catalog.getStreamingImportStatus();
        if (!snapshot || snapshot.phase === "completed" || get().importRun) {
          return;
        }
        const trackStates: Record<string, StreamingImportTrackState> = {};
        const failures: StreamingImportFailure[] = [];
        for (const track of snapshot.tracks) {
          trackStates[track.remote_track_id] = track.state;
          const failure = failureFromState(
            track.remote_track_id,
            track.title,
            track.artist,
            track.state,
          );
          if (failure) failures.push(failure);
        }
        const ids = snapshot.tracks.map((track) => track.remote_track_id);
        const counts = countRun(ids, trackStates);
        set((state) => ({
          trackStates: { ...state.trackStates, ...trackStates },
          activeImportIds: ids,
          importFailures: failures,
          importRun: {
            jobId: snapshot.job_id,
            phase: "running",
            total: snapshot.total,
            completed: snapshot.completed,
            imported: counts.imported,
            skipped: counts.skipped,
            label: null,
            cancelling: false,
          },
        }));
        if (snapshot.phase === "awaiting_decision") {
          // The decision prompt did not survive the reload; stop the paused
          // import so a new one can start.
          const progress =
            await backend.catalog.continueStreamingImport("cancel");
          applyImportProgress(progress);
        }
      } catch (error) {
        reportError(error);
      }
    },
  }));

  return store;
}

export const useCatalogStore = createCatalogStore();
