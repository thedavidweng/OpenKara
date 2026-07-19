// @vitest-environment jsdom

import { beforeEach, describe, expect, test, vi } from "vitest";
import type {
  IntegrityCleanupResult,
  IntegrityReport,
  ManagedAssetIssue,
} from "@/types/ipc";
import type {
  SettingsActionContext,
  SettingsOverlaySnapshot,
  SettingsOverlayState,
} from "./settings-overlay.types";

vi.mock("@/lib/errors", () => ({
  getErrorMessage: (error: unknown) =>
    error instanceof Error ? error.message : String(error),
}));

vi.mock("@/stores/library-store", () => ({
  useLibraryStore: {
    getState: vi.fn(() => ({ selectedSongIds: new Set<string>() })),
    setState: vi.fn(),
  },
}));

import { createIntegritySettingsActions } from "./settings-overlay.integrity-actions";

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const emptyReport: IntegrityReport = {
  checked_local_songs: 0,
  skipped_remote_songs: 0,
  missing_primary_media: [],
  empty_primary_media: [],
  missing_optional_assets: [],
  empty_optional_assets: [],
  orphaned_managed_files: [],
};

const sampleIssue: ManagedAssetIssue = {
  song_hash: "hash-abc12345",
  asset_type: "primary_media",
  path: "media/abc.mp3",
};

const sampleReport: IntegrityReport = {
  checked_local_songs: 3,
  skipped_remote_songs: 1,
  missing_primary_media: [sampleIssue],
  empty_primary_media: [
    {
      song_hash: "hash-empty1234",
      asset_type: "primary_media",
      path: "media/empty.mp3",
    },
  ],
  missing_optional_assets: [
    {
      song_hash: "hash-opt1234567",
      asset_type: "cdg",
      path: "media-g/abc.cdg",
    },
  ],
  empty_optional_assets: [],
  orphaned_managed_files: ["stems/orphan.wav"],
};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

function createHarness(
  initialSnapshot?: Partial<Omit<SettingsOverlaySnapshot, "state">> & {
    state?: Partial<SettingsOverlayState>;
  },
) {
  let snapshot: SettingsOverlaySnapshot = {
    state: {
      libraryPath: null,
      libraryError: null,
      libraryRegistry: null,
      libraries: [],
      activeLibraryId: null,
      stemMode: "four_stem",
      modelVariant: "htdemucs_ft",
      modelStatuses: {},
      downloadingModel: null,
      runtimeStatus: null,
      language: "en",
      hideBatchSeparate: false,
      coverArtBackdrop: false,
      executionProvider: "xnnpack",
      availableExecutionProviders: ["cpu", "xnnpack"],
      eqEnabled: false,
      eqGainsDb: [0, 0, 0, 0, 0],
      librarySortMode: "recently_imported",
      themePreference: "dark",
      integrityReport: null,
      integritySelection: new Set<string>(),
      integritySkippedCount: null,
      ...initialSnapshot?.state,
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
      ...initialSnapshot?.meta,
    },
  };

  const dependencies = {
    api: {
      createLocalLibrary: vi.fn(),
      registerLocalLibrary: vi.fn(),
      switchLibrary: vi.fn(),
      refreshRemoteRepository: vi.fn(),
      getLibraryRegistry: vi.fn(),
      renameLibrary: vi.fn(),
      removeLibrary: vi.fn(),
      deleteLibrary: vi.fn(),
      setLanguage: vi.fn(),
      restartApp: vi.fn(),
      setStemMode: vi.fn(),
      setExecutionProvider: vi.fn(),
      setHideBatchSeparate: vi.fn(),
      setCoverArtBackdrop: vi.fn(),
      createLibrary: vi.fn(),
      deleteAllCachedLyrics: vi.fn(),
      deleteAllStems: vi.fn(),
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
      setThemePreference: vi.fn(),
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
      getAppSettingsSnapshot: vi.fn(),
      hydrateAppSettings: vi.fn(),
      patchAppSettings: vi.fn(),
      setEqEnabled: vi.fn(),
      setEqGains: vi.fn(),
      setThemePreference: vi.fn(),
    },
  };

  const patchState = vi.fn((patch: Record<string, unknown>) => {
    snapshot = {
      ...snapshot,
      state: { ...snapshot.state, ...patch },
    };
  });
  const patchMeta = vi.fn((patch: Record<string, unknown>) => {
    snapshot = {
      ...snapshot,
      meta: { ...snapshot.meta, ...patch },
    };
  });

  const closeDialog = vi.fn(() => {
    snapshot = {
      ...snapshot,
      meta: { ...snapshot.meta, dangerDialog: null },
    };
  });

  const context: SettingsActionContext = {
    dependencies,
    controls: {
      getSnapshot: () => snapshot,
      setSnapshot: (updater) => {
        snapshot = updater(snapshot);
      },
    },
    patchState,
    patchMeta,
    refreshLibraryRegistry: vi.fn().mockResolvedValue(undefined),
    refreshModelStatuses: vi.fn().mockResolvedValue(undefined),
    applyModelVariant: vi.fn(),
    selectSingleDirectory: vi.fn(),
    closeDialog,
  };

  const actions = createIntegritySettingsActions(context);

  return { actions, context, dependencies, snapshot: () => snapshot };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

beforeEach(() => {
  vi.clearAllMocks();
});

describe("createIntegritySettingsActions", () => {
  describe("checkLibraryIntegrity", () => {
    test("sets in-progress flag, fetches report, and selects default hashes", async () => {
      const harness = createHarness();
      harness.dependencies.api.checkLibraryIntegrity.mockResolvedValue(
        sampleReport,
      );

      await harness.actions.checkLibraryIntegrity();

      expect(
        harness.dependencies.api.checkLibraryIntegrity,
      ).toHaveBeenCalledOnce();
      expect(harness.dependencies.notifyError).not.toHaveBeenCalled();

      // Verify the report was stored
      const finalSnapshot = harness.snapshot();
      expect(finalSnapshot.state.integrityReport).toEqual(sampleReport);
      // Default selection includes missing + empty primary
      expect(finalSnapshot.state.integritySelection.has("hash-abc12345")).toBe(
        true,
      );
      expect(finalSnapshot.state.integritySelection.has("hash-empty1234")).toBe(
        true,
      );
      // Optional issues are NOT in default selection
      expect(
        finalSnapshot.state.integritySelection.has("hash-opt1234567"),
      ).toBe(false);
    });

    test("clears previous report and skipped count at start", async () => {
      const harness = createHarness({
        state: {
          integrityReport: sampleReport,
          integritySelection: new Set(["hash-abc12345"]),
          integritySkippedCount: 5,
        },
      });
      harness.dependencies.api.checkLibraryIntegrity.mockResolvedValue(
        emptyReport,
      );

      await harness.actions.checkLibraryIntegrity();

      const finalSnapshot = harness.snapshot();
      expect(finalSnapshot.state.integrityReport).toEqual(emptyReport);
      expect(finalSnapshot.state.integritySelection.size).toBe(0);
      expect(finalSnapshot.state.integritySkippedCount).toBeNull();
    });

    test("notifies error and clears report when API throws", async () => {
      const harness = createHarness();
      harness.dependencies.api.checkLibraryIntegrity.mockRejectedValue(
        new Error("DB locked"),
      );

      await harness.actions.checkLibraryIntegrity();

      expect(harness.dependencies.notifyError).toHaveBeenCalledOnce();
      const finalSnapshot = harness.snapshot();
      expect(finalSnapshot.state.integrityReport).toBeNull();
      expect(finalSnapshot.state.integritySelection.size).toBe(0);
    });

    test("resets in-progress flag in finally block on success", async () => {
      const harness = createHarness();
      harness.dependencies.api.checkLibraryIntegrity.mockResolvedValue(
        emptyReport,
      );

      await harness.actions.checkLibraryIntegrity();

      const finalSnapshot = harness.snapshot();
      expect(finalSnapshot.meta.integrityCheckInProgress).toBe(false);
    });

    test("resets in-progress flag in finally block on error", async () => {
      const harness = createHarness();
      harness.dependencies.api.checkLibraryIntegrity.mockRejectedValue(
        new Error("fail"),
      );

      await harness.actions.checkLibraryIntegrity();

      const finalSnapshot = harness.snapshot();
      expect(finalSnapshot.meta.integrityCheckInProgress).toBe(false);
    });
  });

  describe("toggleIntegritySelection", () => {
    test("adds hash to selection when not present", () => {
      const harness = createHarness({
        state: { integritySelection: new Set<string>() },
      });

      harness.actions.toggleIntegritySelection("hash-abc12345");

      const finalSnapshot = harness.snapshot();
      expect(finalSnapshot.state.integritySelection.has("hash-abc12345")).toBe(
        true,
      );
    });

    test("removes hash from selection when present", () => {
      const harness = createHarness({
        state: {
          integritySelection: new Set(["hash-abc12345", "hash-other12"]),
        },
      });

      harness.actions.toggleIntegritySelection("hash-abc12345");

      const finalSnapshot = harness.snapshot();
      expect(finalSnapshot.state.integritySelection.has("hash-abc12345")).toBe(
        false,
      );
      expect(finalSnapshot.state.integritySelection.has("hash-other12")).toBe(
        true,
      );
    });
  });

  describe("openIntegrityCleanupConfirmDialog", () => {
    test("sets dangerDialog to integrity_cleanup_confirm", () => {
      const harness = createHarness();

      harness.actions.openIntegrityCleanupConfirmDialog();

      const finalSnapshot = harness.snapshot();
      expect(finalSnapshot.meta.dangerDialog).toBe("integrity_cleanup_confirm");
    });
  });

  describe("confirmIntegrityCleanup", () => {
    test("closes dialog immediately when selection is empty", async () => {
      const harness = createHarness({
        state: { integritySelection: new Set<string>() },
      });

      await harness.actions.confirmIntegrityCleanup();

      expect(
        harness.dependencies.api.removeMissingLibraryEntries,
      ).not.toHaveBeenCalled();
      // closeDialog was called (dangerDialog reset)
      expect(harness.snapshot().meta.dangerDialog).toBeNull();
    });

    test("calls removeMissingLibraryEntries with selected hashes", async () => {
      const harness = createHarness({
        state: {
          integrityReport: sampleReport,
          integritySelection: new Set(["hash-abc12345", "hash-empty1234"]),
        },
      });
      const result: IntegrityCleanupResult = {
        deleted_song_hashes: ["hash-abc12345", "hash-empty1234"],
        skipped_song_hashes: [],
      };
      harness.dependencies.api.removeMissingLibraryEntries.mockResolvedValue(
        result,
      );

      await harness.actions.confirmIntegrityCleanup();

      expect(
        harness.dependencies.api.removeMissingLibraryEntries,
      ).toHaveBeenCalledWith(["hash-abc12345", "hash-empty1234"]);
      expect(
        harness.dependencies.queueStore.removeSongIds,
      ).toHaveBeenCalledWith(["hash-abc12345", "hash-empty1234"]);
      expect(harness.dependencies.libraryStore.loadLibrary).toHaveBeenCalled();
      expect(harness.dependencies.playerStore.loadState).toHaveBeenCalled();
    });

    test("clears library selection when deleted songs overlap", async () => {
      const harness = createHarness({
        state: {
          integrityReport: sampleReport,
          integritySelection: new Set(["hash-abc12345"]),
        },
      });
      const result: IntegrityCleanupResult = {
        deleted_song_hashes: ["hash-abc12345"],
        skipped_song_hashes: [],
      };
      harness.dependencies.api.removeMissingLibraryEntries.mockResolvedValue(
        result,
      );
      // Override the mock to return a selection containing the deleted hash
      const { useLibraryStore } = await import("@/stores/library-store");
      vi.mocked(useLibraryStore.getState).mockReturnValueOnce({
        selectedSongIds: new Set(["hash-abc12345", "other"]),
      } as never);

      await harness.actions.confirmIntegrityCleanup();

      expect(
        harness.dependencies.libraryStore.clearSelection,
      ).toHaveBeenCalled();
    });

    test("does not clear library selection when no overlap", async () => {
      const harness = createHarness({
        state: {
          integrityReport: sampleReport,
          integritySelection: new Set(["hash-abc12345"]),
        },
      });
      const result: IntegrityCleanupResult = {
        deleted_song_hashes: ["hash-abc12345"],
        skipped_song_hashes: [],
      };
      harness.dependencies.api.removeMissingLibraryEntries.mockResolvedValue(
        result,
      );
      const { useLibraryStore } = await import("@/stores/library-store");
      vi.mocked(useLibraryStore.getState).mockReturnValueOnce({
        selectedSongIds: new Set(["other-hash"]),
      } as never);

      await harness.actions.confirmIntegrityCleanup();

      expect(
        harness.dependencies.libraryStore.clearSelection,
      ).not.toHaveBeenCalled();
    });

    test("updates report by removing deleted entries and recomputes selection", async () => {
      const harness = createHarness({
        state: {
          integrityReport: sampleReport,
          integritySelection: new Set([
            "hash-abc12345",
            "hash-empty1234",
            "hash-opt1234567",
          ]),
        },
      });
      const result: IntegrityCleanupResult = {
        deleted_song_hashes: ["hash-abc12345", "hash-empty1234"],
        skipped_song_hashes: [],
      };
      harness.dependencies.api.removeMissingLibraryEntries.mockResolvedValue(
        result,
      );

      await harness.actions.confirmIntegrityCleanup();

      const finalSnapshot = harness.snapshot();
      // Deleted hashes removed from report
      expect(
        finalSnapshot.state.integrityReport!.missing_primary_media,
      ).toHaveLength(0);
      expect(
        finalSnapshot.state.integrityReport!.empty_primary_media,
      ).toHaveLength(0);
      // Optional issue remains
      expect(
        finalSnapshot.state.integrityReport!.missing_optional_assets,
      ).toHaveLength(1);
      // Selection only retains non-deleted hashes
      expect(finalSnapshot.state.integritySelection.has("hash-abc12345")).toBe(
        false,
      );
      expect(finalSnapshot.state.integritySelection.has("hash-empty1234")).toBe(
        false,
      );
      expect(
        finalSnapshot.state.integritySelection.has("hash-opt1234567"),
      ).toBe(true);
    });

    test("sets skipped count when entries were skipped", async () => {
      const harness = createHarness({
        state: {
          integrityReport: sampleReport,
          integritySelection: new Set(["hash-abc12345"]),
        },
      });
      const result: IntegrityCleanupResult = {
        deleted_song_hashes: ["hash-abc12345"],
        skipped_song_hashes: ["hash-skipped1", "hash-skipped2"],
      };
      harness.dependencies.api.removeMissingLibraryEntries.mockResolvedValue(
        result,
      );

      await harness.actions.confirmIntegrityCleanup();

      const finalSnapshot = harness.snapshot();
      expect(finalSnapshot.state.integritySkippedCount).toBe(2);
    });

    test("clears skipped count when no entries were skipped", async () => {
      const harness = createHarness({
        state: {
          integrityReport: sampleReport,
          integritySelection: new Set(["hash-abc12345"]),
          integritySkippedCount: 3,
        },
      });
      const result: IntegrityCleanupResult = {
        deleted_song_hashes: ["hash-abc12345"],
        skipped_song_hashes: [],
      };
      harness.dependencies.api.removeMissingLibraryEntries.mockResolvedValue(
        result,
      );

      await harness.actions.confirmIntegrityCleanup();

      const finalSnapshot = harness.snapshot();
      expect(finalSnapshot.state.integritySkippedCount).toBeNull();
    });

    test("closes dialog on success", async () => {
      const harness = createHarness({
        state: {
          integrityReport: sampleReport,
          integritySelection: new Set(["hash-abc12345"]),
        },
      });
      harness.dependencies.api.removeMissingLibraryEntries.mockResolvedValue({
        deleted_song_hashes: ["hash-abc12345"],
        skipped_song_hashes: [],
      });

      await harness.actions.confirmIntegrityCleanup();

      expect(harness.snapshot().meta.dangerDialog).toBeNull();
    });

    test("sets cleanup in-progress flag and resets in finally", async () => {
      const harness = createHarness({
        state: {
          integrityReport: sampleReport,
          integritySelection: new Set(["hash-abc12345"]),
        },
      });
      harness.dependencies.api.removeMissingLibraryEntries.mockResolvedValue({
        deleted_song_hashes: ["hash-abc12345"],
        skipped_song_hashes: [],
      });

      await harness.actions.confirmIntegrityCleanup();

      expect(harness.snapshot().meta.integrityCleanupInProgress).toBe(false);
    });

    test("notifies error, reloads library, and closes dialog on API failure", async () => {
      const harness = createHarness({
        state: {
          integrityReport: sampleReport,
          integritySelection: new Set(["hash-abc12345"]),
        },
      });
      harness.dependencies.api.removeMissingLibraryEntries.mockRejectedValue(
        new Error("DB error"),
      );

      await harness.actions.confirmIntegrityCleanup();

      expect(harness.dependencies.notifyError).toHaveBeenCalledOnce();
      expect(harness.dependencies.libraryStore.loadLibrary).toHaveBeenCalled();
      expect(harness.snapshot().meta.dangerDialog).toBeNull();
      expect(harness.snapshot().meta.integrityCleanupInProgress).toBe(false);
    });

    test("filters empty_optional_assets entries for deleted songs", async () => {
      const reportWithEmptyOptional: IntegrityReport = {
        ...sampleReport,
        empty_optional_assets: [
          {
            song_hash: "hash-abc12345",
            asset_type: "stem_vocals",
            path: "stems/abc.wav",
          },
          {
            song_hash: "hash-opt1234567",
            asset_type: "stem_drums",
            path: "stems/opt.wav",
          },
        ],
      };
      const harness = createHarness({
        state: {
          integrityReport: reportWithEmptyOptional,
          integritySelection: new Set(["hash-abc12345"]),
        },
      });
      harness.dependencies.api.removeMissingLibraryEntries.mockResolvedValue({
        deleted_song_hashes: ["hash-abc12345"],
        skipped_song_hashes: [],
      });

      await harness.actions.confirmIntegrityCleanup();

      const finalSnapshot = harness.snapshot();
      const emptyOptional =
        finalSnapshot.state.integrityReport!.empty_optional_assets;
      // Deleted hash removed; non-deleted hash retained.
      expect(emptyOptional).toHaveLength(1);
      expect(emptyOptional[0].song_hash).toBe("hash-opt1234567");
    });

    test("does not call removeSongIds when no songs were deleted", async () => {
      const harness = createHarness({
        state: {
          integrityReport: sampleReport,
          integritySelection: new Set(["hash-abc12345"]),
        },
      });
      harness.dependencies.api.removeMissingLibraryEntries.mockResolvedValue({
        deleted_song_hashes: [],
        skipped_song_hashes: ["hash-abc12345"],
      });

      await harness.actions.confirmIntegrityCleanup();

      expect(
        harness.dependencies.queueStore.removeSongIds,
      ).not.toHaveBeenCalled();
    });
  });

  describe("closeIntegrityReport", () => {
    test("clears report, selection, and skipped count", () => {
      const harness = createHarness({
        state: {
          integrityReport: sampleReport,
          integritySelection: new Set(["hash-abc12345"]),
          integritySkippedCount: 5,
        },
      });

      harness.actions.closeIntegrityReport();

      const finalSnapshot = harness.snapshot();
      expect(finalSnapshot.state.integrityReport).toBeNull();
      expect(finalSnapshot.state.integritySelection.size).toBe(0);
      expect(finalSnapshot.state.integritySkippedCount).toBeNull();
    });
  });
});
