// @vitest-environment jsdom

import { beforeEach, describe, expect, test, vi } from "vitest";
import type {
  ExecutionProvider,
  LibraryRegistrySnapshot,
  RegisteredLibrary,
  ThemePreference,
  UpdatePolicy,
} from "@/types/ipc";
import type {
  SettingsActionContext,
  SettingsOverlaySnapshot,
} from "./settings-overlay.types";

vi.mock("@/lib/errors", () => ({
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : String(error),
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: {
    setState: vi.fn(),
  },
}));

import {
  createLibrarySettingsActions,
  describeLibrary,
} from "./settings-overlay.library-actions";

const localLibrary: RegisteredLibrary = {
  id: "local:/karaoke",
  kind: "local",
  display_name: "karaoke",
  root_path: "/karaoke",
};

const remoteLibraryWithDisplay: RegisteredLibrary = {
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

const remoteLibraryWithoutDisplay: RegisteredLibrary = {
  id: "remote:drive-2",
  kind: "remote",
  display_name: "Dropbox",
  provider: "dropbox",
  account_id: "account-2",
  remote_root_locator: "/shared/openkara",
  remote_path_display: "",
  connection_config: null,
  cached_db_path: null,
  remote_revision: null,
};

const emptyRegistry: LibraryRegistrySnapshot = {
  active_library_id: null,
  libraries: [],
};

function createAppSettings() {
  return {
    stem_mode: "four_stem" as const,
    model_variant: "htdemucs_ft" as const,
    language: "en",
    hide_batch_separate: false,
    cover_art_backdrop: false,
    lyrics_font_step: 0,
    execution_provider: "xnnpack" as const,
    available_execution_providers: ["cpu", "xnnpack"] as const,
    eq_enabled: false,
    eq_gains_db: [0, 0, 0, 0, 0],
    crossfade_enabled: false,
    crossfade_duration_ms: 3_000,
    library_sort_mode: "recently_imported" as const,
  };
}

function createHarness(overrides?: {
  libraries?: RegisteredLibrary[];
  activeLibraryId?: string | null;
}) {
  const libraries = overrides?.libraries ?? [localLibrary];
  const activeLibraryId = overrides?.activeLibraryId ?? localLibrary.id;

  let snapshot: SettingsOverlaySnapshot = {
    state: {
      libraryPath: null,
      libraryError: null,
      libraryRegistry: null,
      libraries,
      activeLibraryId,
      stemMode: "four_stem",
      modelVariant: "htdemucs_ft",
      modelStatuses: {},
      downloadingModel: null,
      modelUpdate: null,
      runtimeStatus: null,
      runtimeUpdate: null,
      language: "en",
      hideBatchSeparate: false,
      coverArtBackdrop: false,
      executionProvider: "xnnpack",
      availableExecutionProviders: ["cpu", "xnnpack"],
      eqEnabled: false,
      eqGainsDb: [0, 0, 0, 0, 0],
      crossfadeEnabled: false,
      crossfadeDurationMs: 3_000,
      librarySortMode: "recently_imported",
      themePreference: "dark",
      updatePolicy: "notify",
      integrityReport: null,
      integritySelection: new Set(),
      integritySkippedCount: null,
    },
    meta: {
      isInitializing: false,
      dangerDialog: null,
      stemsSize: null,
      downgradeSavings: null,
      deletingStemsInProgress: false,
      deletingLyricsInProgress: false,
      downgradingInProgress: false,
      integrityCheckInProgress: false,
      integrityCleanupInProgress: false,
    },
  };

  const dependencies = {
    api: {
      createLocalLibrary: vi.fn().mockResolvedValue(undefined),
      registerLocalLibrary: vi.fn().mockResolvedValue(undefined),
      switchLibrary: vi.fn(),
      refreshRemoteRepository: vi.fn().mockResolvedValue(undefined),
      getLibraryRegistry: vi.fn().mockResolvedValue(emptyRegistry),
      renameLibrary: vi.fn(),
      removeLibrary: vi.fn(),
      deleteLibrary: vi.fn(),
      setLanguage: vi.fn(),
      restartApp: vi.fn().mockResolvedValue(undefined),
      setStemMode: vi.fn(),
      setThemePreference: vi.fn(),
      setExecutionProvider: vi.fn(),
      setHideBatchSeparate: vi.fn(),
      setCoverArtBackdrop: vi.fn(),
      createLibrary: vi.fn(),
      deleteAllCachedLyrics: vi.fn(),
      deleteAllStems: vi.fn(),
      checkModelUpdates: vi.fn(),
      checkRuntimeUpdates: vi.fn(),
      deleteModel: vi.fn(),
      deleteRuntime: vi.fn(),
      downloadModel: vi.fn(),
      downloadRuntime: vi.fn(),
      downgradeAllToTwoStem: vi.fn(),
      estimateDowngradeSavings: vi.fn(),
      estimateStemsSize: vi.fn(),
      getAllSeparationStatuses: vi.fn(),
      getLibraryPath: vi.fn(),
      getRuntimeBootstrapStatus: vi.fn(),
      getSettings: vi.fn(),
      getModelStatus: vi.fn(),
      openLibrary: vi.fn(),
      mirrorLocalLibraryToRemote: vi.fn(),
      reauthorizeRemoteLibrary: vi.fn(),
      setModelVariant: vi.fn(),
      setEqEnabled: vi.fn(),
      setEqGains: vi.fn(),
      setCrossfadeEnabled: vi.fn(),
      setCrossfadeDurationMs: vi.fn(),
      setUpdatePolicy: vi.fn(),
      checkLibraryIntegrity: vi.fn(),
      removeMissingLibraryEntries: vi.fn(),
    },
    notifyError: vi.fn(),
    openDirectory: vi.fn(),
    changeLanguage: vi.fn(),
    libraryStore: {
      clearAllSeparationStatuses: vi.fn(),
      clearAllUploadStatuses: vi.fn(),
      clearSelection: vi.fn(),
      loadLibrary: vi.fn().mockResolvedValue(undefined),
      updateSeparationStatus: vi.fn(),
    },
    queueStore: { clearQueue: vi.fn(), removeSongIds: vi.fn() },
    playerStore: { loadState: vi.fn().mockResolvedValue(undefined) },
    lyricsStore: { clear: vi.fn() },
    settingsStore: {
      getAppSettingsSnapshot: vi.fn(() => ({
        hydrated: true,
        stemMode: "four_stem" as const,
        modelVariant: "htdemucs_ft" as const,
        language: "en",
        hideBatchSeparate: false,
        coverArtBackdrop: false,
        lyricsFontStep: 0,
        executionProvider: "xnnpack" as const,
        availableExecutionProviders: ["cpu", "xnnpack"] as ExecutionProvider[],
        eqEnabled: false,
        eqGainsDb: [0, 0, 0, 0, 0] as [number, number, number, number, number],
        crossfadeEnabled: false,
        crossfadeDurationMs: 3_000,
        librarySortMode: "recently_imported" as const,
        themePreference: "dark" as ThemePreference,
        updatePolicy: "notify" as UpdatePolicy,
      })),
      hydrateAppSettings: vi.fn(),
      patchAppSettings: vi.fn(),
      setEqEnabled: vi.fn().mockResolvedValue(undefined),
      setEqGains: vi.fn().mockResolvedValue(undefined),
      setCrossfadeEnabled: vi.fn(),
      setCrossfadeDurationMs: vi.fn(),
      setThemePreference: vi.fn(),
      setUpdatePolicy: vi.fn(),
    },
  };

  const patchState = vi.fn((patch: Record<string, unknown>) => {
    snapshot = {
      ...snapshot,
      state: { ...snapshot.state, ...patch },
    };
  });

  const refreshLibraryRegistry = vi.fn().mockResolvedValue(undefined);
  const refreshModelStatuses = vi.fn().mockResolvedValue(undefined);
  const selectSingleDirectory = vi.fn();

  const context: SettingsActionContext = {
    dependencies,
    controls: {
      getSnapshot: () => snapshot,
      setSnapshot: (updater) => {
        snapshot = updater(snapshot);
      },
    },
    patchState,
    patchMeta: vi.fn(),
    refreshLibraryRegistry,
    refreshModelStatuses,
    applyModelVariant: vi.fn(),
    selectSingleDirectory,
    closeDialog: vi.fn(),
  };

  const actions = createLibrarySettingsActions(context);

  return {
    actions,
    context,
    dependencies,
    patchState,
    refreshLibraryRegistry,
    refreshModelStatuses,
    selectSingleDirectory,
    getSnapshot: () => snapshot,
  };
}

describe("describeLibrary", () => {
  test("returns root_path for a local library", () => {
    expect(describeLibrary(localLibrary)).toBe("/karaoke");
  });

  test("returns display_name with remote_path_display for a remote library", () => {
    expect(describeLibrary(remoteLibraryWithDisplay)).toBe(
      "Drive · Google Drive / OpenKara",
    );
  });

  test("falls back to remote_root_locator when remote_path_display is empty", () => {
    expect(describeLibrary(remoteLibraryWithoutDisplay)).toBe(
      "Dropbox · /shared/openkara",
    );
  });
});

describe("createLibrarySettingsActions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("createLibrary", () => {
    test("no-ops when selectSingleDirectory returns null", async () => {
      const harness = createHarness();
      harness.selectSingleDirectory.mockResolvedValue(null);

      await harness.actions.createLibrary("Create");

      expect(
        harness.dependencies.api.createLocalLibrary,
      ).not.toHaveBeenCalled();
      expect(harness.refreshLibraryRegistry).not.toHaveBeenCalled();
    });

    test("creates a local library at selected path + /OpenKara on success", async () => {
      const harness = createHarness();
      harness.selectSingleDirectory.mockResolvedValue("/music");

      await harness.actions.createLibrary("Create");

      expect(harness.patchState).toHaveBeenCalledWith({ libraryError: null });
      expect(harness.dependencies.api.createLocalLibrary).toHaveBeenCalledWith(
        "/music/OpenKara",
      );
      expect(harness.refreshLibraryRegistry).toHaveBeenCalled();
    });

    test("patches libraryError on failure", async () => {
      const harness = createHarness();
      harness.selectSingleDirectory.mockResolvedValue("/music");
      harness.dependencies.api.createLocalLibrary.mockRejectedValue(
        new Error("disk full"),
      );

      await harness.actions.createLibrary("Create");

      expect(harness.patchState).toHaveBeenCalledWith({
        libraryError: "disk full",
      });
    });
  });

  describe("openLibrary", () => {
    test("no-ops when selectSingleDirectory returns null", async () => {
      const harness = createHarness();
      harness.selectSingleDirectory.mockResolvedValue(null);

      await harness.actions.openLibrary("Open");

      expect(
        harness.dependencies.api.registerLocalLibrary,
      ).not.toHaveBeenCalled();
    });

    test("registers a local library at selected path", async () => {
      const harness = createHarness();
      harness.selectSingleDirectory.mockResolvedValue("/existing");

      await harness.actions.openLibrary("Open");

      expect(harness.patchState).toHaveBeenCalledWith({ libraryError: null });
      expect(
        harness.dependencies.api.registerLocalLibrary,
      ).toHaveBeenCalledWith("/existing");
      expect(harness.refreshLibraryRegistry).toHaveBeenCalled();
    });

    test("patches libraryError on failure", async () => {
      const harness = createHarness();
      harness.selectSingleDirectory.mockResolvedValue("/existing");
      harness.dependencies.api.registerLocalLibrary.mockRejectedValue(
        new Error("not found"),
      );

      await harness.actions.openLibrary("Open");

      expect(harness.patchState).toHaveBeenCalledWith({
        libraryError: "not found",
      });
    });
  });

  describe("switchLibrary", () => {
    test("calls switchLibrary API and applies side effects on success", async () => {
      const harness = createHarness();
      const registry: LibraryRegistrySnapshot = {
        active_library_id: "local:/other",
        libraries: [localLibrary],
      };
      harness.dependencies.api.switchLibrary.mockResolvedValue(registry);

      await harness.actions.switchLibrary("local:/other");

      expect(harness.patchState).toHaveBeenCalledWith({ libraryError: null });
      expect(harness.dependencies.api.switchLibrary).toHaveBeenCalledWith(
        "local:/other",
      );
      expect(
        harness.dependencies.libraryStore.clearAllSeparationStatuses,
      ).toHaveBeenCalled();
      expect(
        harness.dependencies.libraryStore.clearAllUploadStatuses,
      ).toHaveBeenCalled();
      expect(
        harness.dependencies.libraryStore.clearSelection,
      ).toHaveBeenCalled();
      expect(harness.dependencies.queueStore.clearQueue).toHaveBeenCalled();
      expect(harness.dependencies.lyricsStore.clear).toHaveBeenCalled();
      expect(harness.dependencies.playerStore.loadState).toHaveBeenCalled();
      expect(harness.dependencies.libraryStore.loadLibrary).toHaveBeenCalled();
      expect(harness.refreshLibraryRegistry).toHaveBeenCalled();
      expect(harness.refreshModelStatuses).toHaveBeenCalled();
    });

    test("refreshes remote repository when switching to a remote library", async () => {
      const harness = createHarness({
        libraries: [localLibrary, remoteLibraryWithDisplay],
        activeLibraryId: localLibrary.id,
      });
      const registry: LibraryRegistrySnapshot = {
        active_library_id: remoteLibraryWithDisplay.id,
        libraries: [localLibrary, remoteLibraryWithDisplay],
      };
      harness.dependencies.api.switchLibrary.mockResolvedValue(registry);

      await harness.actions.switchLibrary(remoteLibraryWithDisplay.id);

      expect(
        harness.dependencies.api.refreshRemoteRepository,
      ).toHaveBeenCalled();
    });

    test("patches libraryError on failure", async () => {
      const harness = createHarness();
      harness.dependencies.api.switchLibrary.mockRejectedValue(
        new Error("switch failed"),
      );

      await harness.actions.switchLibrary("local:/missing");

      expect(harness.patchState).toHaveBeenCalledWith({
        libraryError: "switch failed",
      });
    });
  });

  describe("renameLibrary", () => {
    test("no-ops when the trimmed name is empty", async () => {
      const harness = createHarness();

      await harness.actions.renameLibrary(localLibrary.id, "   ");

      expect(harness.dependencies.api.renameLibrary).not.toHaveBeenCalled();
    });

    test("no-ops when the name is unchanged", async () => {
      const harness = createHarness();

      await harness.actions.renameLibrary(localLibrary.id, "karaoke");

      expect(harness.dependencies.api.renameLibrary).not.toHaveBeenCalled();
    });

    test("calls renameLibrary API with trimmed name and refreshes state", async () => {
      const harness = createHarness();
      const registry: LibraryRegistrySnapshot = {
        active_library_id: localLibrary.id,
        libraries: [{ ...localLibrary, display_name: "New Name" }],
      };
      harness.dependencies.api.renameLibrary.mockResolvedValue(registry);

      await harness.actions.renameLibrary(localLibrary.id, "  New Name  ");

      expect(harness.dependencies.api.renameLibrary).toHaveBeenCalledWith(
        localLibrary.id,
        "New Name",
      );
      expect(harness.refreshLibraryRegistry).toHaveBeenCalled();
      expect(harness.refreshModelStatuses).toHaveBeenCalled();
    });

    test("patches libraryError on failure", async () => {
      const harness = createHarness();
      harness.dependencies.api.renameLibrary.mockRejectedValue(
        new Error("rename failed"),
      );

      await harness.actions.renameLibrary(localLibrary.id, "New Name");

      expect(harness.patchState).toHaveBeenCalledWith({
        libraryError: "rename failed",
      });
    });
  });

  describe("removeLibrary", () => {
    test("no-ops when window.confirm returns false", async () => {
      const harness = createHarness();
      vi.spyOn(window, "confirm").mockReturnValue(false);

      await harness.actions.removeLibrary(localLibrary.id);

      expect(harness.dependencies.api.removeLibrary).not.toHaveBeenCalled();
    });

    test("calls removeLibrary API when confirmed", async () => {
      const harness = createHarness();
      vi.spyOn(window, "confirm").mockReturnValue(true);
      const registry: LibraryRegistrySnapshot = {
        active_library_id: null,
        libraries: [],
      };
      harness.dependencies.api.removeLibrary.mockResolvedValue(registry);

      await harness.actions.removeLibrary(localLibrary.id);

      expect(window.confirm).toHaveBeenCalled();
      expect(harness.dependencies.api.removeLibrary).toHaveBeenCalledWith(
        localLibrary.id,
      );
      expect(harness.refreshLibraryRegistry).toHaveBeenCalled();
    });

    test("patches libraryError on failure", async () => {
      const harness = createHarness();
      vi.spyOn(window, "confirm").mockReturnValue(true);
      harness.dependencies.api.removeLibrary.mockRejectedValue(
        new Error("remove failed"),
      );

      await harness.actions.removeLibrary(localLibrary.id);

      expect(harness.patchState).toHaveBeenCalledWith({
        libraryError: "remove failed",
      });
    });
  });

  describe("deleteLibrary", () => {
    test("no-ops when window.confirm returns false", async () => {
      const harness = createHarness();
      vi.spyOn(window, "confirm").mockReturnValue(false);

      await harness.actions.deleteLibrary(localLibrary.id, "karaoke");

      expect(harness.dependencies.api.deleteLibrary).not.toHaveBeenCalled();
    });

    test("no-ops when confirmationName does not match display_name", async () => {
      const harness = createHarness();
      vi.spyOn(window, "confirm").mockReturnValue(true);

      await harness.actions.deleteLibrary(localLibrary.id, "Wrong Name");

      expect(harness.dependencies.api.deleteLibrary).not.toHaveBeenCalled();
    });

    test("calls deleteLibrary API when confirmed with correct name", async () => {
      const harness = createHarness();
      vi.spyOn(window, "confirm").mockReturnValue(true);
      const registry: LibraryRegistrySnapshot = {
        active_library_id: null,
        libraries: [],
      };
      harness.dependencies.api.deleteLibrary.mockResolvedValue(registry);

      await harness.actions.deleteLibrary(localLibrary.id, "karaoke");

      expect(harness.dependencies.api.deleteLibrary).toHaveBeenCalledWith(
        localLibrary.id,
      );
      expect(harness.refreshLibraryRegistry).toHaveBeenCalled();
    });

    test("patches libraryError on failure", async () => {
      const harness = createHarness();
      vi.spyOn(window, "confirm").mockReturnValue(true);
      harness.dependencies.api.deleteLibrary.mockRejectedValue(
        new Error("delete failed"),
      );

      await harness.actions.deleteLibrary(localLibrary.id, "karaoke");

      expect(harness.patchState).toHaveBeenCalledWith({
        libraryError: "delete failed",
      });
    });
  });

  describe("setLanguage", () => {
    test("patches state, updates settings store, calls changeLanguage, and hydrates", async () => {
      const harness = createHarness();
      const appSettings = createAppSettings();
      harness.dependencies.api.setLanguage.mockResolvedValue(appSettings);

      await harness.actions.setLanguage("ja");

      expect(harness.patchState).toHaveBeenCalledWith({ language: "ja" });
      expect(
        harness.dependencies.settingsStore.patchAppSettings,
      ).toHaveBeenCalledWith({ language: "ja" });
      expect(harness.dependencies.changeLanguage).toHaveBeenCalledWith("ja");
      expect(harness.dependencies.api.setLanguage).toHaveBeenCalledWith("ja");
      expect(
        harness.dependencies.settingsStore.hydrateAppSettings,
      ).toHaveBeenCalledWith(appSettings);
    });

    test("calls notifyError on failure", async () => {
      const harness = createHarness();
      harness.dependencies.api.setLanguage.mockRejectedValue(
        new Error("lang fail"),
      );

      await harness.actions.setLanguage("fr");

      expect(harness.dependencies.notifyError).toHaveBeenCalledWith(
        expect.objectContaining({ message: "lang fail" }),
      );
    });
  });

  describe("restartApp", () => {
    test("calls api.restartApp", async () => {
      const harness = createHarness();

      await harness.actions.restartApp();

      expect(harness.dependencies.api.restartApp).toHaveBeenCalledOnce();
    });

    test("calls notifyError on failure", async () => {
      const harness = createHarness();
      harness.dependencies.api.restartApp.mockRejectedValue(
        new Error("restart fail"),
      );

      await harness.actions.restartApp();

      expect(harness.dependencies.notifyError).toHaveBeenCalled();
    });
  });

  describe("setStemMode", () => {
    test("calls api.setStemMode and hydrates settings", async () => {
      const harness = createHarness();
      const appSettings = {
        ...createAppSettings(),
        stem_mode: "two_stem" as const,
      };
      harness.dependencies.api.setStemMode.mockResolvedValue(appSettings);

      await harness.actions.setStemMode("two_stem");

      expect(harness.dependencies.api.setStemMode).toHaveBeenCalledWith(
        "two_stem",
      );
      expect(
        harness.dependencies.settingsStore.hydrateAppSettings,
      ).toHaveBeenCalledWith(appSettings);
      expect(harness.patchState).toHaveBeenCalledWith({
        stemMode: "two_stem",
      });
    });

    test("calls notifyError on failure", async () => {
      const harness = createHarness();
      harness.dependencies.api.setStemMode.mockRejectedValue(
        new Error("stem fail"),
      );

      await harness.actions.setStemMode("four_stem");

      expect(harness.dependencies.notifyError).toHaveBeenCalled();
    });
  });

  describe("setExecutionProvider", () => {
    test("calls api.setExecutionProvider and hydrates settings", async () => {
      const harness = createHarness();
      const appSettings = {
        ...createAppSettings(),
        execution_provider: "xnnpack" as const,
      };
      harness.dependencies.api.setExecutionProvider.mockResolvedValue(
        appSettings,
      );

      await harness.actions.setExecutionProvider("xnnpack");

      expect(
        harness.dependencies.api.setExecutionProvider,
      ).toHaveBeenCalledWith("xnnpack");
      expect(
        harness.dependencies.settingsStore.hydrateAppSettings,
      ).toHaveBeenCalledWith(appSettings);
      expect(harness.patchState).toHaveBeenCalledWith({
        executionProvider: "xnnpack",
      });
    });

    test("calls notifyError on failure", async () => {
      const harness = createHarness();
      harness.dependencies.api.setExecutionProvider.mockRejectedValue(
        new Error("provider fail"),
      );

      await harness.actions.setExecutionProvider("xnnpack");

      expect(harness.dependencies.notifyError).toHaveBeenCalled();
    });
  });

  describe("toggleHideBatchSeparate", () => {
    test("patches state, updates settings store, and calls api", async () => {
      const harness = createHarness();
      const appSettings = {
        ...createAppSettings(),
        hide_batch_separate: true,
      };
      harness.dependencies.api.setHideBatchSeparate.mockResolvedValue(
        appSettings,
      );

      await harness.actions.toggleHideBatchSeparate(true);

      expect(harness.patchState).toHaveBeenCalledWith({
        hideBatchSeparate: true,
      });
      expect(
        harness.dependencies.settingsStore.patchAppSettings,
      ).toHaveBeenCalledWith({ hideBatchSeparate: true });
      expect(
        harness.dependencies.api.setHideBatchSeparate,
      ).toHaveBeenCalledWith(true);
      expect(
        harness.dependencies.settingsStore.hydrateAppSettings,
      ).toHaveBeenCalledWith(appSettings);
    });

    test("calls notifyError on failure", async () => {
      const harness = createHarness();
      harness.dependencies.api.setHideBatchSeparate.mockRejectedValue(
        new Error("batch fail"),
      );

      await harness.actions.toggleHideBatchSeparate(false);

      expect(harness.dependencies.notifyError).toHaveBeenCalled();
    });
  });

  describe("toggleCoverArtBackdrop", () => {
    test("patches state, updates settings store, and calls api", async () => {
      const harness = createHarness();
      const appSettings = {
        ...createAppSettings(),
        cover_art_backdrop: true,
      };
      harness.dependencies.api.setCoverArtBackdrop.mockResolvedValue(
        appSettings,
      );

      await harness.actions.toggleCoverArtBackdrop(true);

      expect(harness.patchState).toHaveBeenCalledWith({
        coverArtBackdrop: true,
      });
      expect(
        harness.dependencies.settingsStore.patchAppSettings,
      ).toHaveBeenCalledWith({ coverArtBackdrop: true });
      expect(harness.dependencies.api.setCoverArtBackdrop).toHaveBeenCalledWith(
        true,
      );
      expect(
        harness.dependencies.settingsStore.hydrateAppSettings,
      ).toHaveBeenCalledWith(appSettings);
    });

    test("calls notifyError on failure", async () => {
      const harness = createHarness();
      harness.dependencies.api.setCoverArtBackdrop.mockRejectedValue(
        new Error("backdrop fail"),
      );

      await harness.actions.toggleCoverArtBackdrop(false);

      expect(harness.dependencies.notifyError).toHaveBeenCalled();
    });
  });

  describe("setEqEnabled", () => {
    test("patches state and delegates mutation ownership to the settings store", async () => {
      const harness = createHarness();
      harness.dependencies.settingsStore.getAppSettingsSnapshot.mockReturnValue(
        {
          ...harness.dependencies.settingsStore.getAppSettingsSnapshot(),
          eqEnabled: true,
        },
      );

      await harness.actions.setEqEnabled(true);

      expect(harness.patchState).toHaveBeenCalledWith({ eqEnabled: true });
      expect(
        harness.dependencies.settingsStore.setEqEnabled,
      ).toHaveBeenCalledWith(true);
      expect(harness.dependencies.api.setEqEnabled).not.toHaveBeenCalled();
      expect(
        harness.dependencies.settingsStore.getAppSettingsSnapshot,
      ).toHaveBeenCalled();
    });

    test("uses the settings-store rollback value after a failed mutation", async () => {
      const harness = createHarness();

      await harness.actions.setEqEnabled(true);

      expect(
        harness.dependencies.settingsStore.setEqEnabled,
      ).toHaveBeenCalledWith(true);
      expect(harness.patchState).toHaveBeenCalledWith({ eqEnabled: false });
      expect(harness.dependencies.notifyError).not.toHaveBeenCalled();
    });
  });

  describe("setEqGains", () => {
    test("patches state and delegates mutation ownership to the settings store", async () => {
      const harness = createHarness();
      const gains: [number, number, number, number, number] = [0, 0, 6, 0, 0];
      harness.dependencies.settingsStore.getAppSettingsSnapshot.mockReturnValue(
        {
          ...harness.dependencies.settingsStore.getAppSettingsSnapshot(),
          eqGainsDb: gains,
        },
      );

      await harness.actions.setEqGains(gains);

      expect(harness.patchState).toHaveBeenCalledWith({
        eqGainsDb: gains,
      });
      expect(
        harness.dependencies.settingsStore.setEqGains,
      ).toHaveBeenCalledWith(gains);
      expect(harness.dependencies.api.setEqGains).not.toHaveBeenCalled();
    });

    test("clamps each gain to ±12 dB", async () => {
      const harness = createHarness();
      await harness.actions.setEqGains([20, -20, 0, 0, 0]);

      expect(
        harness.dependencies.settingsStore.setEqGains,
      ).toHaveBeenCalledWith([12, -12, 0, 0, 0]);
    });

    test("skips api call when values are unchanged", async () => {
      const harness = createHarness();

      await harness.actions.setEqGains([0, 0, 0, 0, 0]);

      expect(
        harness.dependencies.settingsStore.setEqGains,
      ).not.toHaveBeenCalled();
    });

    test("uses the settings-store rollback value after a failed mutation", async () => {
      const harness = createHarness();

      await harness.actions.setEqGains([1, 3, 0, 0, 0]);

      expect(
        harness.dependencies.settingsStore.setEqGains,
      ).toHaveBeenCalledWith([1, 3, 0, 0, 0]);
      expect(harness.patchState).toHaveBeenCalledWith({
        eqGainsDb: [0, 0, 0, 0, 0],
      });
      expect(harness.dependencies.notifyError).not.toHaveBeenCalled();
    });
  });

  describe("resetEqGains", () => {
    test("patches state to flat and delegates to the settings store", async () => {
      const harness = createHarness();
      const flat = [0, 0, 0, 0, 0] as [number, number, number, number, number];

      await harness.actions.resetEqGains();

      expect(harness.patchState).toHaveBeenCalledWith({ eqGainsDb: flat });
      expect(
        harness.dependencies.settingsStore.setEqGains,
      ).toHaveBeenCalledWith(flat);
      expect(harness.dependencies.api.setEqGains).not.toHaveBeenCalled();
    });

    test("uses the settings-store rollback value after a failed reset", async () => {
      const harness = createHarness();
      const previous = [6, 0, 0, 0, 0] as [
        number,
        number,
        number,
        number,
        number,
      ];
      // Seed the snapshot with non-flat gains so rollback is observable.
      harness.context.controls.setSnapshot((s) => ({
        ...s,
        state: { ...s.state, eqGainsDb: previous },
      }));
      harness.dependencies.settingsStore.getAppSettingsSnapshot.mockReturnValue(
        {
          ...harness.dependencies.settingsStore.getAppSettingsSnapshot(),
          eqGainsDb: previous,
        },
      );

      await harness.actions.resetEqGains();

      expect(
        harness.dependencies.settingsStore.setEqGains,
      ).toHaveBeenCalledWith([0, 0, 0, 0, 0]);
      expect(harness.patchState).toHaveBeenCalledWith({ eqGainsDb: previous });
      expect(harness.dependencies.notifyError).not.toHaveBeenCalled();
    });
  });

  describe("setCrossfadeEnabled", () => {
    test("patches state and delegates mutation ownership to the settings store", async () => {
      const harness = createHarness();
      harness.dependencies.settingsStore.getAppSettingsSnapshot.mockReturnValue(
        {
          ...harness.dependencies.settingsStore.getAppSettingsSnapshot(),
          crossfadeEnabled: true,
        },
      );

      await harness.actions.setCrossfadeEnabled(true);

      expect(harness.patchState).toHaveBeenCalledWith({
        crossfadeEnabled: true,
      });
      expect(
        harness.dependencies.settingsStore.setCrossfadeEnabled,
      ).toHaveBeenCalledWith(true);
      expect(
        harness.dependencies.settingsStore.getAppSettingsSnapshot,
      ).toHaveBeenCalled();
    });

    test("uses the settings-store rollback value after a failed mutation", async () => {
      const harness = createHarness();

      await harness.actions.setCrossfadeEnabled(true);

      expect(
        harness.dependencies.settingsStore.setCrossfadeEnabled,
      ).toHaveBeenCalledWith(true);
      expect(harness.patchState).toHaveBeenCalledWith({
        crossfadeEnabled: false,
      });
      expect(harness.dependencies.notifyError).not.toHaveBeenCalled();
    });
  });

  describe("setCrossfadeDurationMs", () => {
    test("patches state and delegates mutation ownership to the settings store", async () => {
      const harness = createHarness();
      harness.dependencies.settingsStore.getAppSettingsSnapshot.mockReturnValue(
        {
          ...harness.dependencies.settingsStore.getAppSettingsSnapshot(),
          crossfadeDurationMs: 5_000,
        },
      );

      await harness.actions.setCrossfadeDurationMs(5_000);

      expect(harness.patchState).toHaveBeenCalledWith({
        crossfadeDurationMs: 5_000,
      });
      expect(
        harness.dependencies.settingsStore.setCrossfadeDurationMs,
      ).toHaveBeenCalledWith(5_000);
    });

    test("skips api call when value is unchanged", async () => {
      const harness = createHarness();
      // Default snapshot has crossfadeDurationMs: 3_000
      await harness.actions.setCrossfadeDurationMs(3_000);

      expect(
        harness.dependencies.settingsStore.setCrossfadeDurationMs,
      ).not.toHaveBeenCalled();
    });

    test("clamps duration to [500, 10000]", async () => {
      const harness = createHarness();
      harness.dependencies.settingsStore.getAppSettingsSnapshot.mockReturnValue(
        {
          ...harness.dependencies.settingsStore.getAppSettingsSnapshot(),
          crossfadeDurationMs: 500,
        },
      );

      await harness.actions.setCrossfadeDurationMs(100);

      expect(
        harness.dependencies.settingsStore.setCrossfadeDurationMs,
      ).toHaveBeenCalledWith(500);
    });
  });

  describe("setThemePreference", () => {
    test("patches overlay state and delegates to settings store", async () => {
      const harness = createHarness();
      harness.dependencies.settingsStore.setThemePreference.mockResolvedValue(
        undefined,
      );
      const baseSnapshot =
        harness.dependencies.settingsStore.getAppSettingsSnapshot();
      harness.dependencies.settingsStore.getAppSettingsSnapshot.mockReturnValue(
        {
          ...baseSnapshot,
          themePreference: "light",
        },
      );

      await harness.actions.setThemePreference("light");

      expect(harness.patchState).toHaveBeenCalledWith({
        themePreference: "light",
      });
      expect(
        harness.dependencies.settingsStore.setThemePreference,
      ).toHaveBeenCalledWith("light");
    });

    test("mirrors the final store snapshot after the store action resolves", async () => {
      const harness = createHarness();
      harness.dependencies.settingsStore.setThemePreference.mockResolvedValue(
        undefined,
      );
      const baseSnapshot =
        harness.dependencies.settingsStore.getAppSettingsSnapshot();
      harness.dependencies.settingsStore.getAppSettingsSnapshot.mockReturnValue(
        {
          ...baseSnapshot,
          themePreference: "dark",
        },
      );

      await harness.actions.setThemePreference("light");

      expect(harness.patchState).toHaveBeenLastCalledWith({
        themePreference: "dark",
      });
    });
  });
});
