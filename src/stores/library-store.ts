import { create } from "zustand";
import * as api from "@/lib/tauri";
import type {
  ImportFailure,
  SeparationStatusSnapshot,
  Song,
} from "@/types/ipc";

interface LibraryState {
  songs: Song[];
  searchQuery: string;
  isImporting: boolean;
  importErrors: ImportFailure[];
  selectedSongId: string | null;
  separationStatuses: Record<string, SeparationStatusSnapshot>;
  filter: "all" | "separated";

  loadLibrary: () => Promise<void>;
  importFiles: (paths: string[]) => Promise<void>;
  setSearchQuery: (query: string) => void;
  searchSongs: (query: string) => Promise<void>;
  setSelectedSongId: (id: string | null) => void;
  setFilter: (filter: "all" | "separated") => void;
  updateSeparationStatus: (status: SeparationStatusSnapshot) => void;
  clearImportErrors: () => void;
}

export const useLibraryStore = create<LibraryState>((set, get) => ({
  songs: [],
  searchQuery: "",
  isImporting: false,
  importErrors: [],
  selectedSongId: null,
  separationStatuses: {},
  filter: "all",

  loadLibrary: async () => {
    const songs = await api.getLibrary();
    set({ songs });
  },

  importFiles: async (paths) => {
    set({ isImporting: true, importErrors: [] });
    try {
      const result = await api.importSongs(paths);
      if (result.failed.length > 0) {
        set({ importErrors: result.failed });
      }
      // Reload full library to get consistent state
      const songs = await api.getLibrary();
      set({ songs });
    } finally {
      set({ isImporting: false });
    }
  },

  setSearchQuery: (query) => {
    set({ searchQuery: query });
    if (query.trim()) {
      get().searchSongs(query);
    } else {
      get().loadLibrary();
    }
  },

  searchSongs: async (query) => {
    const songs = await api.searchLibrary(query);
    set({ songs });
  },

  setSelectedSongId: (id) => set({ selectedSongId: id }),

  setFilter: (filter) => set({ filter }),

  updateSeparationStatus: (status) => {
    set((state) => ({
      separationStatuses: {
        ...state.separationStatuses,
        [status.song_id]: status,
      },
    }));
  },

  clearImportErrors: () => set({ importErrors: [] }),
}));
