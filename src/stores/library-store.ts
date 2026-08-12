import { create } from "zustand";
import { tauriBackend, type Backend } from "@/lib/backend";
import { createWebviewSyncChannel } from "@/runtime/webview-sync";
import { invalidateCoverArtUrl } from "@/lib/cover-art";
import { notifyError } from "@/lib/errors";
import { runImportWorkflow } from "@/runtime/import-workflow";
import type { AmbiguousCdgChoiceRequest } from "@/lib/import-cdg-selection";
import type {
  BatchSeparationProgress,
  ImportFailure,
  SeparationStatusSnapshot,
  Song,
  UploadStatusSnapshot,
} from "@/types/ipc";

function debounce<T extends (...args: never[]) => void>(
  fn: T,
  ms: number,
): T & { cancel: () => void } {
  let timer: ReturnType<typeof setTimeout> | null = null;
  const wrapped = ((...args: Parameters<T>) => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      fn(...args);
    }, ms);
  }) as T & { cancel: () => void };
  wrapped.cancel = () => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  };
  return wrapped;
}

interface LibraryState {
  songs: Song[];
  searchQuery: string;
  isImporting: boolean;
  importErrors: ImportFailure[];
  selectedSongIds: Set<string>;
  lastClickedSongId: string | null;
  separationStatuses: Record<string, SeparationStatusSnapshot>;
  uploadStatuses: Record<string, UploadStatusSnapshot>;
  filter: "all" | "separated";
  batchSeparation: BatchSeparationProgress | null;
  pendingImportCdgChoice: AmbiguousCdgChoiceRequest | null;

  loadLibrary: () => Promise<void>;
  importFiles: (paths: string[]) => Promise<void>;
  promptForCdgChoice: (
    request: AmbiguousCdgChoiceRequest,
  ) => Promise<string | null>;
  resolveCdgChoicePrompt: (audioPath: string | null) => void;
  setSearchQuery: (query: string) => void;
  searchSongs: (query: string) => Promise<void>;
  selectSong: (
    songId: string,
    event: { shiftKey: boolean; metaKey: boolean; ctrlKey: boolean },
    orderedHashes?: string[],
  ) => void;
  clearSelection: () => void;
  clearRangeSelectionAnchor: () => void;
  setFilter: (filter: "all" | "separated") => void;
  updateSongMetadata: (
    hash: string,
    title: string | null,
    artist: string | null,
  ) => Promise<boolean>;
  setSongsInstrumental: (
    songIds: string[],
    instrumental: boolean,
  ) => Promise<boolean>;
  setSongsLanguage: (
    songIds: string[],
    language: string | null,
  ) => Promise<boolean>;
  extractEmbeddedCoverArt: (songIds: string[]) => Promise<boolean>;
  updateSeparationStatus: (status: SeparationStatusSnapshot) => void;
  clearAllSeparationStatuses: () => void;
  updateUploadStatus: (status: UploadStatusSnapshot) => void;
  clearUploadStatus: (songId: string) => void;
  clearAllUploadStatuses: () => void;
  updateBatchProgress: (progress: BatchSeparationProgress) => void;
  clearBatchSeparation: () => void;
  clearImportErrors: () => void;
}

export function createLibraryStore(backend: Backend = tauriBackend) {
  const { library, librarySetup, maintenance, remoteRepository, separation } =
    backend;

  let pendingCdgChoiceResolver: ((audioPath: string | null) => void) | null =
    null;

  const librarySyncChannel = createWebviewSyncChannel<{ revision: number }>(
    "openkara.library",
  );
  let librarySyncRevision = 0;

  function publishLibraryInvalidation() {
    librarySyncRevision += 1;
    librarySyncChannel.publish({ revision: librarySyncRevision });
  }

  let searchGeneration = 0;

  const debouncedSearch = debounce(async (query: string) => {
    const gen = ++searchGeneration;
    try {
      const songs = await library.searchLibrary(query);
      if (gen !== searchGeneration) return;
      store.setState({ songs });
    } catch (e) {
      if (gen !== searchGeneration) return;
      notifyError(e);
    }
  }, 300);

  const store = create<LibraryState>((set, get) => ({
    songs: [],
    searchQuery: "",
    isImporting: false,
    importErrors: [],
    selectedSongIds: new Set<string>(),
    lastClickedSongId: null,
    separationStatuses: {},
    uploadStatuses: {},
    filter: "all",
    batchSeparation: null,
    pendingImportCdgChoice: null,

    loadLibrary: async () => {
      try {
        try {
          const activeLibrary = await librarySetup.getActiveLibrary();
          if (activeLibrary?.kind === "remote") {
            await remoteRepository.refreshRemoteRepository();
          }
        } catch {}

        const songs = await library.getLibrary();
        set({ songs });

        try {
          const statuses = await separation.getAllSeparationStatuses();
          const statusMap: Record<string, SeparationStatusSnapshot> = {};
          for (const s of statuses) {
            statusMap[s.song_id] = s;
          }
          set({ separationStatuses: statusMap });
        } catch {}

        try {
          const uploads = await remoteRepository.getAllUploadStatuses();
          const uploadMap: Record<string, UploadStatusSnapshot> = {};
          for (const s of uploads) {
            uploadMap[s.song_id] = s;
          }
          set({ uploadStatuses: uploadMap });
        } catch {}
      } catch (e) {
        notifyError(e);
      }
    },

    importFiles: async (paths) => {
      set({ isImporting: true, importErrors: [] });
      try {
        await runImportWorkflow({
          paths,
          api: {
            importSongs: library.importSongs,
            importLyricsFiles: backend.lyrics.importLyricsFiles,
            getLibrary: library.getLibrary,
          },
          promptForCdgChoice: get().promptForCdgChoice,
          notifyError,
          setImportErrors: (importErrors) => set({ importErrors }),
          setSongs: (songs) => set({ songs }),
          publishLibraryInvalidation,
        });
      } catch (e) {
        notifyError(e);
      } finally {
        set({ isImporting: false });
      }
    },

    promptForCdgChoice: async (request) => {
      set({ pendingImportCdgChoice: request });

      return new Promise((resolve) => {
        pendingCdgChoiceResolver = resolve;
      });
    },

    resolveCdgChoicePrompt: (audioPath) => {
      set({ pendingImportCdgChoice: null });
      pendingCdgChoiceResolver?.(audioPath);
      pendingCdgChoiceResolver = null;
    },

    setSearchQuery: (query) => {
      set({ searchQuery: query });
      if (query.trim()) {
        debouncedSearch(query);
      } else {
        debouncedSearch.cancel();
        searchGeneration++;
        get().loadLibrary();
      }
    },

    searchSongs: async (query) => {
      try {
        const songs = await library.searchLibrary(query);
        set({ songs });
      } catch (e) {
        notifyError(e);
      }
    },

    selectSong: (songId, event, orderedHashes) => {
      const { selectedSongIds, lastClickedSongId } = get();

      if (event.shiftKey && lastClickedSongId && orderedHashes) {
        const startIdx = orderedHashes.indexOf(lastClickedSongId);
        const endIdx = orderedHashes.indexOf(songId);
        if (startIdx !== -1 && endIdx !== -1) {
          const from = Math.min(startIdx, endIdx);
          const to = Math.max(startIdx, endIdx);
          const rangeIds = orderedHashes.slice(from, to + 1);
          const newSet = new Set(selectedSongIds);
          for (const id of rangeIds) {
            newSet.add(id);
          }
          set({ selectedSongIds: newSet });
        }
      } else if (event.metaKey || event.ctrlKey) {
        const newSet = new Set(selectedSongIds);
        if (newSet.has(songId)) {
          newSet.delete(songId);
        } else {
          newSet.add(songId);
        }
        set({ selectedSongIds: newSet, lastClickedSongId: songId });
      } else {
        set({
          selectedSongIds: new Set([songId]),
          lastClickedSongId: songId,
        });
      }
    },

    clearSelection: () =>
      set({ selectedSongIds: new Set(), lastClickedSongId: null }),

    clearRangeSelectionAnchor: () => set({ lastClickedSongId: null }),

    setFilter: (filter) => set({ filter }),

    updateSongMetadata: async (hash, title, artist) => {
      try {
        const updated = await library.updateSongMetadata(hash, title, artist);
        set((state) => ({
          songs: state.songs.map((s) =>
            s.hash === hash
              ? { ...s, title: updated.title, artist: updated.artist }
              : s,
          ),
        }));
        publishLibraryInvalidation();
        return true;
      } catch (e) {
        notifyError(e);
        return false;
      }
    },

    setSongsInstrumental: async (songIds, instrumental) => {
      try {
        const updatedSongs = await library.setSongsInstrumental(
          songIds,
          instrumental,
        );
        const updatedByHash = new Map(
          updatedSongs.map((song) => [song.hash, song]),
        );

        set((state) => ({
          songs: state.songs.map(
            (song) => updatedByHash.get(song.hash) ?? song,
          ),
        }));

        publishLibraryInvalidation();

        return true;
      } catch (e) {
        notifyError(e);
        return false;
      }
    },

    setSongsLanguage: async (songIds, language) => {
      try {
        const updatedSongs = await library.setSongsLanguage(songIds, language);
        const updatedByHash = new Map(
          updatedSongs.map((song) => [song.hash, song]),
        );

        set((state) => ({
          songs: state.songs.map(
            (song) => updatedByHash.get(song.hash) ?? song,
          ),
        }));

        publishLibraryInvalidation();

        return true;
      } catch (e) {
        notifyError(e);
        return false;
      }
    },

    extractEmbeddedCoverArt: async (songIds) => {
      try {
        const result = await maintenance.extractEmbeddedCoverArt(songIds);

        for (const song of result.updated_songs) {
          invalidateCoverArtUrl(song.hash);
        }

        if (result.updated_songs.length > 0) {
          const updatedByHash = new Map(
            result.updated_songs.map((song) => [song.hash, song]),
          );
          set((state) => ({
            songs: state.songs.map(
              (song) => updatedByHash.get(song.hash) ?? song,
            ),
          }));
          publishLibraryInvalidation();
        }

        for (const failure of result.failed) {
          notifyError(failure.error);
        }

        return result.updated_songs.length > 0;
      } catch (e) {
        notifyError(e);
        return false;
      }
    },

    updateSeparationStatus: (status) => {
      set((state) => ({
        separationStatuses: {
          ...state.separationStatuses,
          [status.song_id]: status,
        },
      }));
    },

    clearAllSeparationStatuses: () => set({ separationStatuses: {} }),

    updateUploadStatus: (status) => {
      set((state) => ({
        uploadStatuses: {
          ...state.uploadStatuses,
          [status.song_id]: status,
        },
      }));
    },

    clearUploadStatus: (songId) =>
      set((state) => {
        if (!(songId in state.uploadStatuses)) {
          return state;
        }

        const next = { ...state.uploadStatuses };
        delete next[songId];
        return { uploadStatuses: next };
      }),

    clearAllUploadStatuses: () => set({ uploadStatuses: {} }),

    updateBatchProgress: (progress) => set({ batchSeparation: progress }),

    clearBatchSeparation: () => set({ batchSeparation: null }),

    clearImportErrors: () => set({ importErrors: [] }),
  }));

  librarySyncChannel.subscribe(() => {
    void store.getState().loadLibrary();
  });

  return store;
}

export const useLibraryStore = createLibraryStore();
