import { create, type StoreApi, type UseBoundStore } from "zustand";
import { tauriBackend, type Backend } from "@/lib/backend";
import { notifyError } from "@/lib/errors";
import { onlinePreviewQueueId } from "@/playback";
import type {
  AuthChallenge,
  CommandError,
  ImportConflictPrompt,
  LibraryDecisionAction,
  PreviewQueueItem,
  SourceSnapshot,
  StreamingImportEvent,
  StreamingImportCompleteEvent,
  StreamingImportFailure,
  StreamingImportProgress,
  StreamingImportTrackState,
  StreamingPlaylistDetail,
  StreamingPlaylistSummary,
  StreamingSessionSnapshot,
  StreamingTrack,
  VideoQueueItem,
} from "@/types/ipc";

export type StreamingTab = "liked" | "playlists" | "results";
export type LoadStatus = "idle" | "loading" | "ready" | "error";
export interface ImportRun {
  sourceId: string;
  jobId: string | null;
  phase: "running" | "done";
  total: number;
  completed: number;
  imported: number;
  skipped: number;
  label: string | null;
  cancelling: boolean;
}
export interface SourceBrowseState {
  session: StreamingSessionSnapshot | null;
  sessionStatus: LoadStatus;
  authStatus: LoadStatus;
  liked: StreamingTrack[];
  playlists: StreamingPlaylistSummary[];
  playlistDetail: StreamingPlaylistDetail | null;
  playlistDetails: Record<string, StreamingPlaylistDetail>;
  hideUnavailable: boolean;
  searchResults: StreamingTrack[];
  browserTab: StreamingTab;
  searchQuery: string;
  searchDraft: string;
  likedStatus: LoadStatus;
  likedLoadingMore: boolean;
  playlistsStatus: LoadStatus;
  searchStatus: LoadStatus;
  playlistDetailStatus: LoadStatus;
  selectedPlaylistId: string | null;
  likedCursor: string | null;
  playlistsCursor: string | null;
  searchCursor: string | null;
  selected: Set<string>;
  selecting: boolean;
  signInError: CommandError | null;
  challenge: AuthChallenge | null;
  errors: Partial<Record<string, string>>;
}
export interface CatalogState extends SourceBrowseState {
  activeView: "library" | "streaming";
  activeSourceId: string | null;
  sources: SourceSnapshot[];
  sourceStates: Record<string, SourceBrowseState>;
  sourcesStatus: LoadStatus;
  sourcesError: string | null;
  importFailures: StreamingImportFailure[];
  pendingConflict: ImportConflictPrompt | null;
  videoItems: Record<string, VideoQueueItem>;
  previewItems: Record<string, PreviewQueueItem>;
  trackStates: Record<string, StreamingImportTrackState>;
  importRun: ImportRun | null;
  activeImportIds: string[];
  youtubeLinkOpen: boolean;
  applySources: (sources: SourceSnapshot[]) => void;
  loadSources: (refresh?: boolean) => Promise<void>;
  applySource: (source: SourceSnapshot) => void;
  setActiveView: (view: "library") => void;
  setActiveSource: (sourceId: string) => void;
  setBrowserTab: (tab: StreamingTab) => void;
  setHideUnavailable: (hide: boolean) => void;
  setSelection: (selected: Set<string>) => void;
  setSelecting: (selecting: boolean) => void;
  closePlaylist: () => void;
  setSearchDraft: (draft: string) => void;
  clearSearch: () => void;
  dismissImportRun: () => void;
  setSignInError: (
    sourceId: string,
    challenge: string,
    error: CommandError,
  ) => void;
  clearSignInError: () => void;
  setYoutubeLinkOpen: (open: boolean) => void;
  rememberVideoItems: (items: VideoQueueItem[]) => void;
  getVideoItem: (id: string) => VideoQueueItem | undefined;
  rememberPreviewTracks: (tracks: StreamingTrack[]) => void;
  loadSession: (sourceId?: string) => Promise<void>;
  openSignIn: (sourceId?: string) => Promise<void>;
  cancelSignIn: (sourceId?: string) => Promise<void>;
  acceptSignIn: (session: StreamingSessionSnapshot) => void;
  signOut: (sourceId?: string, stopWork?: boolean) => Promise<void>;
  loadLiked: (more?: boolean) => Promise<void>;
  loadPlaylists: (more?: boolean) => Promise<void>;
  openPlaylist: (remoteId: string, more?: boolean) => Promise<void>;
  search: (query: string, more?: boolean) => Promise<void>;
  importPlaylist: () => Promise<void>;
  importTracks: (
    ids: string[],
    playlistId?: string | null,
    label?: string | null,
    sourceId?: string,
  ) => Promise<void>;
  resolveConflict: (action: LibraryDecisionAction) => Promise<void>;
  applyImportEvent: (event: StreamingImportEvent) => void;
  applyImportComplete: (event: StreamingImportCompleteEvent) => void;
  cancelImport: () => Promise<void>;
  recoverImportJob: () => Promise<void>;
}
export type CatalogStore = UseBoundStore<StoreApi<CatalogState>>;
const emptyBrowse = (): SourceBrowseState => ({
  session: null,
  sessionStatus: "idle",
  authStatus: "idle",
  liked: [],
  playlists: [],
  playlistDetail: null,
  playlistDetails: {},
  hideUnavailable: true,
  searchResults: [],
  browserTab: "playlists",
  searchQuery: "",
  searchDraft: "",
  likedStatus: "idle",
  likedLoadingMore: false,
  playlistsStatus: "idle",
  searchStatus: "idle",
  playlistDetailStatus: "idle",
  selectedPlaylistId: null,
  likedCursor: null,
  playlistsCursor: null,
  searchCursor: null,
  selected: new Set(),
  selecting: false,
  signInError: null,
  challenge: null,
  errors: {},
});
const message = (error: unknown) =>
  error instanceof Error
    ? error.message
    : typeof error === "object" && error !== null && "message" in error
      ? String(error.message)
      : String(error);
const terminal = (state?: StreamingImportTrackState) =>
  state && ["imported", "skipped", "failed"].includes(state.kind);
export function sourceAllows(
  source: SourceSnapshot | undefined,
  capability: string,
  session?: StreamingSessionSnapshot | null,
) {
  return (
    !!source?.installed_version &&
    source.enabled &&
    source.runtime_ready &&
    ![
      "not_installed",
      "disabled",
      "update_required",
      "unavailable",
      "cancelling",
    ].includes(source.state) &&
    !!source.manifest?.capabilities.includes(capability) &&
    (!source.manifest.requires_authentication.includes(
      capability === "track_details"
        ? "tracks.get"
        : capability === "playlist_tracks"
          ? "playlist.tracks"
          : capability === "preview" || capability === "import"
            ? "media.resolve"
            : capability,
    ) ||
      (!!session?.signed_in && !session.expired))
  );
}
export function createCatalogStore(
  backend: Backend = tauriBackend,
): CatalogStore {
  const generations = new Map<string, number>();
  const authTimers = new Map<string, ReturnType<typeof setTimeout>>();
  const generation = (id: string, operation: string) => {
    const key = `${id}:${operation}`;
    const next = (generations.get(key) ?? 0) + 1;
    generations.set(key, next);
    return () => generations.get(key) === next;
  };
  const invalidate = (id: string) => {
    for (const key of generations.keys())
      if (key.startsWith(`${id}:`))
        generations.set(key, (generations.get(key) ?? 0) + 1);
    clearTimeout(authTimers.get(id));
    authTimers.delete(id);
  };
  const browse = (id: string): SourceBrowseState =>
    Object.prototype.hasOwnProperty.call(store.getState().sourceStates, id)
      ? store.getState().sourceStates[id]
      : emptyBrowse();
  const patch = (id: string, update: Partial<SourceBrowseState>) =>
    store.setState((state) => {
      const next = { ...browse(id), ...update };
      return {
        sourceStates: Object.assign(
          Object.create(null) as Record<string, SourceBrowseState>,
          state.sourceStates,
          { [id]: next },
        ),
        ...(state.activeView === "streaming" && state.activeSourceId === id
          ? next
          : {}),
      };
    });
  const activePatch = (update: Partial<SourceBrowseState>) =>
    patch(store.getState().activeSourceId ?? "", update);
  const fail = (id: string, operation: string, error: unknown) => {
    const old = browse(id);
    patch(id, { errors: { ...old.errors, [operation]: message(error) } });
    if (
      typeof error === "object" &&
      error !== null &&
      "code" in error &&
      ["streaming_session_expired", "session_expired"].includes(
        String(error.code),
      )
    ) {
      invalidate(id);
      patch(id, {
        ...emptyBrowse(),
        sessionStatus: "ready",
        session: {
          source_id: id,
          signed_in: false,
          expired: true,
          display_name: null,
        },
      });
    }
  };
  const findTrack = (id: string, remote: string) => {
    const state = browse(id);
    return [
      ...state.liked,
      ...state.searchResults,
      ...Object.values(state.playlistDetails).flatMap(
        (detail) => detail.tracks,
      ),
    ].find((track) => track.remote_track_id === remote);
  };
  const count = (
    id: string,
    ids: string[],
    states: CatalogState["trackStates"],
  ) => {
    let imported = 0,
      skipped = 0;
    for (const remote of ids) {
      const kind = states[onlinePreviewQueueId(id, remote)]?.kind;
      if (kind === "imported") imported++;
      if (kind === "skipped" || kind === "failed") skipped++;
    }
    return { imported, skipped, completed: imported + skipped };
  };
  const applyProgress = (
    progress: StreamingImportProgress,
    jobId: string | null,
  ) => {
    const state = store.getState(),
      run = state.importRun;
    if (
      !run ||
      run.phase !== "running" ||
      run.jobId !== jobId ||
      progress.source_id !== run.sourceId
    )
      return;
    if (progress.status === "awaiting_decision") {
      store.setState({
        pendingConflict: progress.conflict,
        importFailures: progress.failed,
      });
      return;
    }
    const states = { ...state.trackStates };
    for (const remote of state.activeImportIds) {
      const key = onlinePreviewQueueId(run.sourceId, remote);
      if (terminal(states[key])) continue;
      const failure = progress.failed.find(
        (item) => item.remote_track_id === remote,
      );
      states[key] = failure
        ? failure.refusal
          ? { kind: "skipped", refusal: failure.refusal }
          : { kind: "failed", reason: failure.reason }
        : { kind: "imported", song_id: "" };
    }
    store.setState({
      trackStates: states,
      pendingConflict: null,
      importFailures: progress.failed,
      activeImportIds: [],
      importRun: {
        ...run,
        imported: progress.imported_song_ids.length,
        skipped: progress.failed.length,
        completed: progress.imported_song_ids.length + progress.failed.length,
        phase: "done",
        cancelling: false,
      },
    });
  };
  const failImport = (error: unknown, jobId: string | null) => {
    const state = store.getState(),
      run = state.importRun;
    if (!run || run.phase !== "running" || run.jobId !== jobId) return;
    const states = { ...state.trackStates };
    for (const remote of state.activeImportIds) {
      const key = onlinePreviewQueueId(run.sourceId, remote);
      if (!terminal(states[key]))
        states[key] = { kind: "failed", reason: "import_failed" };
    }
    store.setState({
      trackStates: states,
      activeImportIds: [],
      pendingConflict: null,
      importRun: {
        ...run,
        ...count(run.sourceId, state.activeImportIds, states),
        phase: "done",
        cancelling: false,
      },
    });
    notifyError(error);
  };
  const poll = async (
    id: string,
    challenge: AuthChallenge,
    current: () => boolean,
  ) => {
    if (!current() || browse(id).challenge?.challenge !== challenge.challenge)
      return;
    try {
      const result = await backend.catalog.pollStreamingSignIn(
        id,
        challenge.challenge,
      );
      if (!current()) return;
      if (result.state === "complete" && result.session) {
        store.getState().acceptSignIn({ ...result.session, source_id: id });
        return;
      }
      if (result.state === "pending") {
        authTimers.set(
          id,
          setTimeout(
            () => void poll(id, challenge, current),
            Math.max(
              250,
              result.poll_after_ms ?? challenge.poll_after_ms ?? 1000,
            ),
          ),
        );
      } else
        patch(id, {
          challenge: null,
          errors: { ...browse(id).errors, auth: result.state },
        });
    } catch (error) {
      if (current()) {
        patch(id, { challenge: null, authStatus: "idle" });
        fail(id, "auth", error);
      }
    }
  };
  const store: CatalogStore = create<CatalogState>((set, get) => ({
    ...emptyBrowse(),
    activeView: "library",
    activeSourceId: null,
    sources: [],
    sourceStates: Object.create(null) as Record<string, SourceBrowseState>,
    sourcesStatus: "idle",
    sourcesError: null,
    importFailures: [],
    pendingConflict: null,
    videoItems: {},
    previewItems: {},
    trackStates: {},
    importRun: null,
    activeImportIds: [],
    youtubeLinkOpen: false,
    applySources: (sources) => {
      for (const source of sources) get().applySource(source);
    },
    applySource: (source) => {
      const previous = get().sources.find(
        (item) => item.source_id === source.source_id,
      );
      if (
        previous &&
        (!source.installed_version ||
          (!source.enabled && previous.enabled) ||
          source.state === "unavailable" ||
          source.state === "update_required")
      ) {
        invalidate(source.source_id);
        patch(source.source_id, emptyBrowse());
      }
      set((state) => ({
        sources: state.sources.some(
          (item) => item.source_id === source.source_id,
        )
          ? state.sources.map((item) =>
              item.source_id === source.source_id ? source : item,
            )
          : [...state.sources, source],
      }));
    },
    loadSources: async (refresh = false) => {
      const current = generation("_host", "sources");
      set({ sourcesStatus: "loading", sourcesError: null });
      try {
        const sources = await (refresh
          ? backend.catalog.refreshStreamingSources()
          : backend.catalog.listStreamingSources());
        if (!current()) return;
        get().applySources(sources);
        set({ sourcesStatus: "ready" });
      } catch (error) {
        if (current())
          set({ sourcesStatus: "error", sourcesError: message(error) });
      }
    },
    setActiveView: () =>
      set({ activeView: "library", activeSourceId: null, ...emptyBrowse() }),
    setActiveSource: (activeSourceId) =>
      set({
        activeView: "streaming",
        activeSourceId,
        ...browse(activeSourceId),
      }),
    setBrowserTab: (browserTab) =>
      activePatch({ browserTab, selected: new Set(), selecting: false }),
    setHideUnavailable: (hideUnavailable) => activePatch({ hideUnavailable }),
    setSelection: (selected) => activePatch({ selected }),
    setSelecting: (selecting) =>
      activePatch({ selecting, selected: new Set() }),
    closePlaylist: () =>
      activePatch({
        playlistDetail: null,
        selectedPlaylistId: null,
        playlistDetailStatus: "idle",
        selected: new Set(),
        selecting: false,
      }),
    setSearchDraft: (searchDraft) => activePatch({ searchDraft }),
    clearSearch: () => {
      generation(get().activeSourceId ?? "", "search");
      activePatch({
        searchQuery: "",
        searchDraft: "",
        searchResults: [],
        searchCursor: null,
        searchStatus: "idle",
        browserTab: "playlists",
        selected: new Set(),
      });
    },
    dismissImportRun: () => {
      if (get().importRun?.phase !== "running")
        set({ importRun: null, importFailures: [] });
    },
    setSignInError: (id, challenge, signInError) => {
      const source = get().sources.find((item) => item.source_id === id);
      if (
        !source?.enabled ||
        !source.installed_version ||
        browse(id).challenge?.challenge !== challenge
      )
        return;
      generation(id, "auth");
      clearTimeout(authTimers.get(id));
      authTimers.delete(id);
      patch(id, { signInError, challenge: null, authStatus: "error" });
    },
    clearSignInError: () => activePatch({ signInError: null }),
    setYoutubeLinkOpen: (youtubeLinkOpen) => set({ youtubeLinkOpen }),
    rememberVideoItems: (items) =>
      set((state) => ({
        videoItems: {
          ...state.videoItems,
          ...Object.fromEntries(items.map((item) => [item.id, item])),
        },
      })),
    getVideoItem: (id) => get().videoItems[id],
    rememberPreviewTracks: (tracks) =>
      set((state) => ({
        previewItems: {
          ...state.previewItems,
          ...Object.fromEntries(
            tracks.map((track) => {
              const id = onlinePreviewQueueId(
                track.source_id,
                track.remote_track_id,
              );
              return [id, { ...track, id }];
            }),
          ),
        },
      })),
    loadSession: async (id = get().activeSourceId ?? "") => {
      const current = generation(id, "session");
      patch(id, { sessionStatus: "loading" });
      try {
        const session = await backend.catalog.getStreamingSession(id);
        if (current())
          patch(id, {
            session: { ...session, source_id: id },
            sessionStatus: "ready",
          });
      } catch (error) {
        if (current()) {
          patch(id, { sessionStatus: "error" });
          fail(id, "session", error);
        }
      }
    },
    openSignIn: async (id = get().activeSourceId ?? "") => {
      await get().cancelSignIn(id);
      if (browse(id).challenge) return;
      const current = generation(id, "auth");
      patch(id, {
        authStatus: "loading",
        signInError: null,
        errors: { ...browse(id).errors, auth: undefined },
      });
      try {
        const challenge = await backend.catalog.openStreamingSignIn(id);
        if (!current()) {
          await backend.catalog.cancelStreamingSignIn(id, challenge.challenge);
          return;
        }
        patch(id, { challenge, authStatus: "ready" });
        authTimers.set(
          id,
          setTimeout(
            () => void poll(id, challenge, current),
            Math.max(250, challenge.poll_after_ms ?? 1000),
          ),
        );
      } catch (error) {
        if (current()) {
          patch(id, { authStatus: "error" });
          fail(id, "auth", error);
        }
      }
    },
    cancelSignIn: async (id = get().activeSourceId ?? "") => {
      const current = generation(id, "auth");
      clearTimeout(authTimers.get(id));
      const challenge = browse(id).challenge;
      if (!challenge) {
        patch(id, { authStatus: "idle" });
        return;
      }
      try {
        await backend.catalog.cancelStreamingSignIn(id, challenge.challenge);
        if (
          !current() ||
          browse(id).challenge?.challenge !== challenge.challenge
        )
          return;
        patch(id, { challenge: null, authStatus: "idle" });
      } catch (error) {
        if (
          current() &&
          browse(id).challenge?.challenge === challenge.challenge
        )
          fail(id, "auth", error);
      }
    },
    acceptSignIn: (session) => {
      if (
        !browse(session.source_id).challenge &&
        browse(session.source_id).authStatus !== "loading"
      )
        return;
      if (
        !get().sources.some(
          (source) =>
            source.source_id === session.source_id &&
            source.enabled &&
            source.installed_version,
        )
      )
        return;
      invalidate(session.source_id);
      patch(session.source_id, {
        ...emptyBrowse(),
        session,
        sessionStatus: "ready",
      });
    },
    signOut: async (id = get().activeSourceId ?? "", stopWork = false) => {
      try {
        const session = await backend.catalog.signOutStreamingSource(
          id,
          stopWork,
        );
        invalidate(id);
        patch(id, {
          ...emptyBrowse(),
          session: { ...session, source_id: id },
          sessionStatus: "ready",
        });
      } catch (error) {
        fail(id, "auth", error);
        notifyError(error);
      }
    },
    loadLiked: async (more = false) => {
      const id = get().activeSourceId ?? "",
        old = browse(id);
      if (old.likedStatus === "loading" || (more && !old.likedCursor)) return;
      const current = generation(id, "liked");
      patch(id, { likedStatus: "loading", likedLoadingMore: more });
      try {
        const page = await backend.catalog.listStreamingLikedTracks(
          id,
          more ? old.likedCursor : null,
        );
        if (current())
          patch(id, {
            liked: more ? [...old.liked, ...page.tracks] : page.tracks,
            likedCursor: page.next_cursor,
            likedStatus: "ready",
            likedLoadingMore: false,
          });
      } catch (error) {
        if (current()) {
          patch(id, { likedStatus: "error", likedLoadingMore: false });
          fail(id, "liked", error);
        }
      }
    },
    loadPlaylists: async (more = false) => {
      const id = get().activeSourceId ?? "",
        old = browse(id);
      if (old.playlistsStatus === "loading" || (more && !old.playlistsCursor))
        return;
      const current = generation(id, "playlists");
      patch(id, { playlistsStatus: "loading" });
      try {
        const page = await backend.catalog.listStreamingPlaylists(
          id,
          more ? old.playlistsCursor : null,
        );
        if (current())
          patch(id, {
            playlists: more ? [...old.playlists, ...page.items] : page.items,
            playlistsCursor: page.next_cursor,
            playlistsStatus: "ready",
          });
      } catch (error) {
        if (current()) {
          patch(id, { playlistsStatus: "error" });
          fail(id, "playlists", error);
        }
      }
    },
    openPlaylist: async (remote, more = false) => {
      const id = get().activeSourceId ?? "",
        old = browse(id),
        cached = Object.prototype.hasOwnProperty.call(
          old.playlistDetails,
          remote,
        )
          ? old.playlistDetails[remote]
          : undefined;
      if (more && !cached?.next_cursor) return;
      patch(id, {
        selectedPlaylistId: remote,
        selected: old.selectedPlaylistId === remote ? old.selected : new Set(),
        selecting: old.selectedPlaylistId === remote && old.selecting,
        playlistDetail: cached ?? null,
        playlistDetailStatus: cached && !more ? "ready" : "loading",
      });
      if (cached && !more && old.playlistDetailStatus !== "error") return;
      const current = generation(id, `playlist:${remote}`);
      try {
        const page = await backend.catalog.getStreamingPlaylist(
          id,
          remote,
          more ? (cached?.next_cursor ?? null) : null,
        );
        if (!current()) return;
        const detail = {
          ...page,
          tracks: more
            ? [...(cached?.tracks ?? []), ...page.tracks]
            : page.tracks,
        };
        patch(id, {
          playlistDetails: { ...browse(id).playlistDetails, [remote]: detail },
          ...(browse(id).selectedPlaylistId === remote
            ? { playlistDetail: detail, playlistDetailStatus: "ready" }
            : {}),
        });
      } catch (error) {
        if (current()) {
          if (browse(id).selectedPlaylistId === remote)
            patch(id, { playlistDetailStatus: "error" });
          fail(id, "playlist", error);
        }
      }
    },
    search: async (query, more = false) => {
      const id = get().activeSourceId ?? "",
        old = browse(id),
        trimmed = query.trim();
      if (!trimmed) {
        get().clearSearch();
        return;
      }
      if (more && (!old.searchCursor || old.searchStatus === "loading")) return;
      const current = generation(id, "search");
      patch(id, {
        searchQuery: trimmed,
        searchStatus: "loading",
        searchResults: more ? old.searchResults : [],
        searchCursor: more ? old.searchCursor : null,
        browserTab: "results",
        selected: more ? old.selected : new Set(),
        selecting: more && old.selecting,
      });
      try {
        const page = await backend.catalog.searchStreamingSource(
          id,
          trimmed,
          more ? old.searchCursor : null,
        );
        if (current())
          patch(id, {
            searchResults: more
              ? [...old.searchResults, ...page.tracks]
              : page.tracks,
            searchCursor: page.next_cursor,
            searchStatus: "ready",
          });
      } catch (error) {
        if (current()) {
          patch(id, { searchStatus: "error" });
          fail(id, "search", error);
        }
      }
    },
    importPlaylist: async () => {
      const id = get().activeSourceId ?? "";
      const initial = browse(id).playlistDetail;
      if (!initial || get().importRun?.phase === "running") return;
      await get().importTracks([], initial.remote_id, initial.name, id);
    },
    importTracks: async (ids, playlist, label, boundSource) => {
      if (
        get().importRun?.phase === "running" ||
        (ids.length === 0 && !playlist)
      )
        return;
      const sourceId = boundSource ?? get().activeSourceId ?? "";
      const jobId = crypto.randomUUID();
      const trackStates = { ...get().trackStates };
      for (const id of ids)
        trackStates[onlinePreviewQueueId(sourceId, id)] = { kind: "queued" };
      set({
        trackStates,
        activeImportIds: ids,
        importFailures: [],
        importRun: {
          sourceId,
          jobId,
          phase: "running",
          total: ids.length,
          completed: 0,
          imported: 0,
          skipped: 0,
          label:
            label ??
            (ids.length === 1 ? findTrack(sourceId, ids[0])?.title : null) ??
            null,
          cancelling: false,
        },
      });
      try {
        applyProgress(
          await backend.catalog.startStreamingImport(
            sourceId,
            ids,
            playlist ?? null,
            jobId,
          ),
          jobId,
        );
      } catch (error) {
        failImport(error, jobId);
      }
    },
    resolveConflict: async (action) => {
      const jobId = get().importRun?.jobId ?? null;
      set({ pendingConflict: null });
      try {
        applyProgress(
          await backend.catalog.continueStreamingImport(action),
          jobId,
        );
      } catch (error) {
        failImport(error, jobId);
      }
    },
    applyImportComplete: (event) => {
      const run = get().importRun;
      if (
        !run ||
        run.sourceId !== event.source_id ||
        run.jobId !== event.job_id ||
        event.progress.source_id !== event.source_id ||
        event.progress.status !== "completed"
      )
        return;
      applyProgress(event.progress, event.job_id);
    },
    applyImportEvent: (event) => {
      const state = get(),
        run = state.importRun;
      if (
        !run ||
        run.phase !== "running" ||
        run.sourceId !== event.source_id ||
        run.jobId !== event.job_id
      )
        return;
      const activeImportIds = state.activeImportIds.includes(
        event.remote_track_id,
      )
        ? state.activeImportIds
        : [...state.activeImportIds, event.remote_track_id];
      const trackStates = {
        ...state.trackStates,
        [onlinePreviewQueueId(event.source_id, event.remote_track_id)]:
          event.state,
      };
      const known = findTrack(event.source_id, event.remote_track_id);
      const failure: StreamingImportFailure | null =
        event.state.kind === "skipped"
          ? {
              remote_track_id: event.remote_track_id,
              title: event.title || known?.title || "",
              artist: event.artist || known?.artist || "",
              reason: "refusal",
              refusal: event.state.refusal,
            }
          : event.state.kind === "failed"
            ? {
                remote_track_id: event.remote_track_id,
                title: event.title || known?.title || "",
                artist: event.artist || known?.artist || "",
                reason: event.state.reason,
                refusal: null,
              }
            : null;
      set({
        trackStates,
        activeImportIds,
        importFailures: failure
          ? [
              ...state.importFailures.filter(
                (item) => item.remote_track_id !== event.remote_track_id,
              ),
              failure,
            ]
          : state.importFailures,
        importRun: {
          ...run,
          ...count(run.sourceId, activeImportIds, trackStates),
          jobId: event.job_id,
          total: event.total,
          completed: event.completed,
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
        if (get().importRun?.jobId === run.jobId)
          set({ importRun: { ...run, cancelling: false } });
        notifyError(error);
      }
    },
    recoverImportJob: async () => {
      if (get().importRun) return;
      try {
        const snapshot = await backend.catalog.getStreamingImportStatus();
        if (!snapshot || snapshot.phase === "completed" || get().importRun)
          return;
        const trackStates = { ...get().trackStates };
        for (const track of snapshot.tracks)
          trackStates[
            onlinePreviewQueueId(snapshot.source_id, track.remote_track_id)
          ] = track.state;
        const ids = snapshot.tracks.map((track) => track.remote_track_id);
        set({
          trackStates,
          activeImportIds: ids,
          importRun: {
            sourceId: snapshot.source_id,
            jobId: snapshot.job_id,
            phase: "running",
            ...count(snapshot.source_id, ids, trackStates),
            total: snapshot.total,
            completed: snapshot.completed,
            label: null,
            cancelling: false,
          },
        });
        if (snapshot.phase === "awaiting_decision")
          applyProgress(
            await backend.catalog.continueStreamingImport("cancel"),
            snapshot.job_id,
          );
      } catch (error) {
        notifyError(error);
      }
    },
  }));
  return store;
}
export const useCatalogStore = createCatalogStore();
