import { useLibraryStore } from "@/stores/library-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";
import { useQueueStore } from "@/stores/queue-store";
import type { LibrarySessionStores } from "./types";

export function createZustandLibrarySessionStores(): LibrarySessionStores {
  return {
    library: {
      clearSongs: () =>
        useLibraryStore.setState({ songs: [], searchQuery: "" }),
      clearAllSeparationStatuses: () =>
        useLibraryStore.getState().clearAllSeparationStatuses(),
      clearAllUploadStatuses: () =>
        useLibraryStore.getState().clearAllUploadStatuses(),
      clearSelection: () => useLibraryStore.getState().clearSelection(),
      loadLibrary: () => useLibraryStore.getState().loadLibrary(),
    },
    queue: {
      clearQueue: () => useQueueStore.getState().clearQueue(),
    },
    lyrics: {
      clear: () => useLyricsStore.getState().clear(),
    },
    player: {
      loadState: () => usePlayerStore.getState().loadState(),
    },
  };
}
