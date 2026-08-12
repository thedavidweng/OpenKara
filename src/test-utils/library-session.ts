import type {
  LibrarySession,
  LibrarySessionViews,
} from "@/lib/library-session";
import type { LibraryRegistrySnapshot } from "@/types/ipc";

export type LibrarySessionEntry = keyof LibrarySession;

export type LibrarySessionCall =
  | { entry: "createLocalLibrary"; parentDirectory: string }
  | { entry: "openLocalLibrary"; directory: string }
  | { entry: "switchLibrary"; libraryId: string }
  | { entry: "refreshRepository" }
  | { entry: "adoptRegistry"; registry: LibraryRegistrySnapshot };

export interface RecordingLibrarySession {
  createLibrarySession: (views: LibrarySessionViews) => LibrarySession;
  session: LibrarySession;
  calls: LibrarySessionCall[];
  views: LibrarySessionViews | null;
  failOn(entry: LibrarySessionEntry, error: unknown): void;
}

/**
 * A `LibrarySession` that records the entries a caller reaches for instead of
 * running them, so call-site tests can assert delegation rather than the
 * side-effect sequence the session owns. The views the caller wires up are
 * captured so tests can drive them directly.
 */
export function createRecordingLibrarySession(): RecordingLibrarySession {
  const calls: LibrarySessionCall[] = [];
  const failures = new Map<LibrarySessionEntry, unknown>();

  const record = async (call: LibrarySessionCall) => {
    calls.push(call);
    if (failures.has(call.entry)) {
      throw failures.get(call.entry);
    }
  };

  const recording: RecordingLibrarySession = {
    calls,
    views: null,
    failOn: (entry, error) => {
      failures.set(entry, error);
    },
    createLibrarySession: (views) => {
      recording.views = views;
      return recording.session;
    },
    session: {
      createLocalLibrary: (parentDirectory) =>
        record({ entry: "createLocalLibrary", parentDirectory }),
      openLocalLibrary: (directory) =>
        record({ entry: "openLocalLibrary", directory }),
      switchLibrary: (libraryId) =>
        record({ entry: "switchLibrary", libraryId }),
      refreshRepository: () => record({ entry: "refreshRepository" }),
      adoptRegistry: (registry) => record({ entry: "adoptRegistry", registry }),
    },
  };

  return recording;
}
