import type {
  LibrarySession,
  LibrarySessionDependencies,
  LibrarySessionViews,
} from "./types";
import { createZustandLibrarySessionStores } from "./zustand-stores";

const LOCAL_LIBRARY_DIRECTORY_NAME = "OpenKara";

const DETACHED_VIEWS: LibrarySessionViews = {
  refreshRegistry: () => Promise.resolve(),
  refreshModelStatuses: () => Promise.resolve(),
};

export function createLibrarySession({
  backend,
  stores = createZustandLibrarySessionStores(),
  views = DETACHED_VIEWS,
}: LibrarySessionDependencies): LibrarySession {
  const { librarySetup, remoteRepository } = backend;

  const discardDerivedState = () => {
    stores.library.clearAllSeparationStatuses();
    stores.library.clearAllUploadStatuses();
    stores.library.clearSelection();
    stores.queue.clearQueue();
    stores.lyrics.clear();
  };

  const refreshViews = async () => {
    await views.refreshRegistry();
    await views.refreshModelStatuses();
  };

  const rebuildWorkingCopy = async () => {
    discardDerivedState();
    await stores.player.loadState();
    await stores.library.loadLibrary();
    await refreshViews();
  };

  return {
    createLocalLibrary: async (parentDirectory) => {
      await librarySetup.createLocalLibrary(
        `${parentDirectory}/${LOCAL_LIBRARY_DIRECTORY_NAME}`,
      );
      await views.refreshRegistry();
    },

    openLocalLibrary: async (directory) => {
      await librarySetup.registerLocalLibrary(directory);
      await views.refreshRegistry();
    },

    switchLibrary: async (libraryId) => {
      const registry = await librarySetup.switchLibrary(libraryId);
      const target = registry.libraries.find(
        (library) => library.id === libraryId,
      );

      if (target?.kind === "remote") {
        await remoteRepository.refreshRemoteRepository();
      }

      await rebuildWorkingCopy();
    },

    refreshRepository: async () => {
      await remoteRepository.refreshRemoteRepository();
      await rebuildWorkingCopy();
    },

    adoptRegistry: async (registry) => {
      if (registry.active_library_id) {
        await stores.library.loadLibrary();
      } else {
        stores.library.clearSongs();
        discardDerivedState();
        await stores.player.loadState();
      }

      await refreshViews();
    },
  };
}
