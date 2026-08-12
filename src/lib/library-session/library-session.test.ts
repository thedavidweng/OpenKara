import { describe, expect, test } from "vitest";
import { createMockBackend } from "@/lib/backend/mock-backend";
import type { LibraryRegistrySnapshot, RegisteredLibrary } from "@/types/ipc";
import { createLibrarySession } from "./library-session";
import type { LibrarySessionStores, LibrarySessionViews } from "./types";

const localLibrary: RegisteredLibrary = {
  id: "local:/karaoke",
  kind: "local",
  display_name: "karaoke",
  root_path: "/karaoke",
};

const remoteRepository: RegisteredLibrary = {
  id: "remote:drive-1",
  kind: "remote",
  display_name: "Drive",
  provider: "google_drive",
  account_id: "account-1",
  remote_root_locator: "root-1",
  remote_path_display: "Google Drive / OpenKara",
  connection_config: null,
  cached_db_path: null,
  remote_revision: null,
};

const registryWith = (
  activeLibraryId: string | null,
): LibraryRegistrySnapshot => ({
  active_library_id: activeLibraryId,
  libraries: [localLibrary, remoteRepository],
});

const CLEAR_DERIVED_STATE = [
  "library.clearAllSeparationStatuses",
  "library.clearAllUploadStatuses",
  "library.clearSelection",
  "queue.clearQueue",
  "lyrics.clear",
];

const REFRESH_VIEWS = ["views.refreshRegistry", "views.refreshModelStatuses"];

interface HarnessOptions {
  switchResult?: LibraryRegistrySnapshot;
  rejectOn?: "switchLibrary" | "refreshRemoteRepository" | "createLocalLibrary";
  attachViews?: boolean;
}

function createHarness({
  switchResult = registryWith(localLibrary.id),
  rejectOn,
  attachViews = true,
}: HarnessOptions = {}) {
  const calls: string[] = [];

  const record = (label: string) => {
    calls.push(label);
    if (rejectOn && label.startsWith(`backend.${rejectOn}`)) {
      throw new Error(`${rejectOn} failed`);
    }
  };

  const backend = createMockBackend({
    overrides: {
      librarySetup: {
        switchLibrary: async (libraryId: string) => {
          record(`backend.switchLibrary(${libraryId})`);
          return switchResult;
        },
        createLocalLibrary: async (path: string) => {
          record(`backend.createLocalLibrary(${path})`);
        },
        registerLocalLibrary: async (path: string) => {
          record(`backend.registerLocalLibrary(${path})`);
        },
      },
      remoteRepository: {
        refreshRemoteRepository: async () => {
          record("backend.refreshRemoteRepository");
        },
      },
    },
  });

  const stores: LibrarySessionStores = {
    library: {
      clearSongs: () => record("library.clearSongs"),
      clearAllSeparationStatuses: () =>
        record("library.clearAllSeparationStatuses"),
      clearAllUploadStatuses: () => record("library.clearAllUploadStatuses"),
      clearSelection: () => record("library.clearSelection"),
      loadLibrary: async () => record("library.loadLibrary"),
    },
    queue: { clearQueue: () => record("queue.clearQueue") },
    lyrics: { clear: () => record("lyrics.clear") },
    player: { loadState: async () => record("player.loadState") },
  };

  const views: LibrarySessionViews = {
    refreshRegistry: async () => record("views.refreshRegistry"),
    refreshModelStatuses: async () => record("views.refreshModelStatuses"),
  };

  return {
    calls,
    session: createLibrarySession({
      backend,
      stores,
      views: attachViews ? views : undefined,
    }),
  };
}

describe("LibrarySession.switchLibrary", () => {
  test("rebuilds the working copy in a fixed order for a local library", async () => {
    const harness = createHarness();

    await harness.session.switchLibrary(localLibrary.id);

    expect(harness.calls).toEqual([
      `backend.switchLibrary(${localLibrary.id})`,
      ...CLEAR_DERIVED_STATE,
      "player.loadState",
      "library.loadLibrary",
      ...REFRESH_VIEWS,
    ]);
  });

  test("refreshes the remote repository before discarding derived state", async () => {
    const harness = createHarness({
      switchResult: registryWith(remoteRepository.id),
    });

    await harness.session.switchLibrary(remoteRepository.id);

    expect(harness.calls).toEqual([
      `backend.switchLibrary(${remoteRepository.id})`,
      "backend.refreshRemoteRepository",
      ...CLEAR_DERIVED_STATE,
      "player.loadState",
      "library.loadLibrary",
      ...REFRESH_VIEWS,
    ]);
  });

  test("leaves the working copy untouched when the switch itself fails", async () => {
    const harness = createHarness({ rejectOn: "switchLibrary" });

    await expect(
      harness.session.switchLibrary(localLibrary.id),
    ).rejects.toThrow("switchLibrary failed");
    expect(harness.calls).toEqual([
      `backend.switchLibrary(${localLibrary.id})`,
    ]);
  });

  test("stops before discarding derived state when Refresh Repository fails", async () => {
    const harness = createHarness({
      switchResult: registryWith(remoteRepository.id),
      rejectOn: "refreshRemoteRepository",
    });

    await expect(
      harness.session.switchLibrary(remoteRepository.id),
    ).rejects.toThrow("refreshRemoteRepository failed");
    expect(harness.calls).toEqual([
      `backend.switchLibrary(${remoteRepository.id})`,
      "backend.refreshRemoteRepository",
    ]);
  });

  test("runs without views when no view port is attached", async () => {
    const harness = createHarness({ attachViews: false });

    await harness.session.switchLibrary(localLibrary.id);

    expect(harness.calls).toEqual([
      `backend.switchLibrary(${localLibrary.id})`,
      ...CLEAR_DERIVED_STATE,
      "player.loadState",
      "library.loadLibrary",
    ]);
  });
});

describe("LibrarySession.refreshRepository", () => {
  test("refreshes and rebuilds without activating a library", async () => {
    const harness = createHarness();

    await harness.session.refreshRepository();

    expect(harness.calls).toEqual([
      "backend.refreshRemoteRepository",
      ...CLEAR_DERIVED_STATE,
      "player.loadState",
      "library.loadLibrary",
      ...REFRESH_VIEWS,
    ]);
  });

  test("stops at the failed refresh", async () => {
    const harness = createHarness({ rejectOn: "refreshRemoteRepository" });

    await expect(harness.session.refreshRepository()).rejects.toThrow(
      "refreshRemoteRepository failed",
    );
    expect(harness.calls).toEqual(["backend.refreshRemoteRepository"]);
  });
});

describe("LibrarySession.adoptRegistry", () => {
  test("reloads the song list when a library is still active", async () => {
    const harness = createHarness();

    await harness.session.adoptRegistry(registryWith(localLibrary.id));

    expect(harness.calls).toEqual(["library.loadLibrary", ...REFRESH_VIEWS]);
  });

  test("empties the working copy when no library is left active", async () => {
    const harness = createHarness();

    await harness.session.adoptRegistry({
      active_library_id: null,
      libraries: [],
    });

    expect(harness.calls).toEqual([
      "library.clearSongs",
      ...CLEAR_DERIVED_STATE,
      "player.loadState",
      ...REFRESH_VIEWS,
    ]);
  });
});

describe("LibrarySession local library registration", () => {
  test("creates the working copy inside the selected directory", async () => {
    const harness = createHarness();

    await harness.session.createLocalLibrary("/music");

    expect(harness.calls).toEqual([
      "backend.createLocalLibrary(/music/OpenKara)",
      "views.refreshRegistry",
    ]);
  });

  test("registers an existing directory verbatim", async () => {
    const harness = createHarness();

    await harness.session.openLocalLibrary("/existing/OpenKara");

    expect(harness.calls).toEqual([
      "backend.registerLocalLibrary(/existing/OpenKara)",
      "views.refreshRegistry",
    ]);
  });

  test("does not refresh the registry view when creation fails", async () => {
    const harness = createHarness({ rejectOn: "createLocalLibrary" });

    await expect(harness.session.createLocalLibrary("/music")).rejects.toThrow(
      "createLocalLibrary failed",
    );
    expect(harness.calls).toEqual([
      "backend.createLocalLibrary(/music/OpenKara)",
    ]);
  });
});
