import type { LibrarySetupBackend } from "@/lib/backend/types";
import type { LibraryRegistrySnapshot, RegisteredLibrary } from "@/types/ipc";
import type { InvokeCommand } from "./invoke";

export function createLibrarySetupCommands(
  invoke: InvokeCommand,
): LibrarySetupBackend {
  return {
    getLibraryPath: () => invoke<string | null>("get_library_path"),

    getLibraryRegistry: () =>
      invoke<LibraryRegistrySnapshot>("get_library_registry"),

    getActiveLibrary: () =>
      invoke<RegisteredLibrary | null>("get_active_library"),

    createLocalLibrary: (path) => invoke<void>("create_library", { path }),

    registerLocalLibrary: (path) => invoke<void>("open_library", { path }),

    switchLibrary: (libraryId) =>
      invoke<LibraryRegistrySnapshot>("switch_library", { libraryId }),

    removeLibrary: (libraryId) =>
      invoke<LibraryRegistrySnapshot>("remove_library", { libraryId }),

    renameLibrary: (libraryId, displayName) =>
      invoke<LibraryRegistrySnapshot>("rename_library", {
        libraryId,
        displayName,
      }),

    deleteLibrary: (libraryId) =>
      invoke<LibraryRegistrySnapshot>("delete_library", { libraryId }),
  };
}
