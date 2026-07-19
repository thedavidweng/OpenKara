// @vitest-environment jsdom

import { beforeEach, describe, expect, test, vi } from "vitest";

vi.mock("@/stores/bootstrap-store", () => ({
  useBootstrapStore: {
    getState: () => ({
      loadStatus: () => Promise.resolve(),
    }),
  },
}));

vi.mock("@/stores/runtime-bootstrap-store", () => ({
  useRuntimeBootstrapStore: {
    getState: () => ({
      updateStatus: vi.fn(),
    }),
  },
}));

vi.mock("@/stores/settings-store", () => ({
  useSettingsStore: {
    getState: () => ({
      getAppSettingsSnapshot: () => ({
        hydrated: true,
        stemMode: "four_stem",
        modelVariant: "htdemucs_ft",
        language: "zh-CN",
        hideBatchSeparate: false,
        coverArtBackdrop: true,
        lyricsFontStep: 0,
        executionProvider: "xnnpack",
        availableExecutionProviders: ["cpu", "xnnpack"],
        eqEnabled: false,
        eqGainsDb: [0, 0, 0, 0, 0],
        crossfadeEnabled: false,
        crossfadeDurationMs: 3_000,
        librarySortMode: "recently_imported",
        themePreference: "dark",
      }),
    }),
  },
}));

import {
  createInitialSettingsOverlaySnapshot,
  createSettingsOverlayActions,
  type SettingsOverlayControllerDependencies,
  type SettingsOverlaySnapshot,
} from "./SettingsOverlay.state";
import type { AppSettingsSnapshot } from "@/stores/settings-store";

function createDependencies(): SettingsOverlayControllerDependencies {
  return {
    api: {
      createLibrary: vi.fn(),
      createLocalLibrary: vi.fn(),
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
      getLibraryRegistry: vi.fn(),
      getRuntimeBootstrapStatus: vi.fn(),
      getSettings: vi.fn(),
      getModelStatus: vi.fn(),
      openLibrary: vi.fn(),
      registerLocalLibrary: vi.fn(),
      restartApp: vi.fn(),
      switchLibrary: vi.fn(),
      refreshRemoteRepository: vi.fn(),
      renameLibrary: vi.fn(),
      removeLibrary: vi.fn(),
      deleteLibrary: vi.fn(),
      mirrorLocalLibraryToRemote: vi.fn(),
      reauthorizeRemoteLibrary: vi.fn(),
      setExecutionProvider: vi.fn(),
      setHideBatchSeparate: vi.fn(),
      setCoverArtBackdrop: vi.fn(),
      setLanguage: vi.fn(),
      setModelVariant: vi.fn(),
      setStemMode: vi.fn(),
      setEqEnabled: vi.fn(),
      setEqGains: vi.fn(),
      setCrossfadeEnabled: vi.fn(),
      setCrossfadeDurationMs: vi.fn(),
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
      loadLibrary: vi.fn(),
      updateSeparationStatus: vi.fn(),
    },
    queueStore: {
      clearQueue: vi.fn(),
      removeSongIds: vi.fn(),
    },
    playerStore: {
      loadState: vi.fn(),
    },
    lyricsStore: {
      clear: vi.fn(),
    },
    settingsStore: {
      getAppSettingsSnapshot: vi.fn(
        (): AppSettingsSnapshot => ({
          hydrated: true,
          stemMode: "four_stem",
          modelVariant: "htdemucs_ft",
          language: "zh-CN",
          hideBatchSeparate: false,
          coverArtBackdrop: true,
          lyricsFontStep: 0,
          executionProvider: "xnnpack",
          availableExecutionProviders: ["cpu", "xnnpack"],
          eqEnabled: false,
          eqGainsDb: [0, 0, 0, 0, 0],
          crossfadeEnabled: false,
          crossfadeDurationMs: 3_000,
          librarySortMode: "recently_imported",
          themePreference: "dark",
        }),
      ),
      hydrateAppSettings: vi.fn(),
      patchAppSettings: vi.fn(),
      setEqEnabled: vi.fn(),
      setEqGains: vi.fn(),
      setCrossfadeEnabled: vi.fn(),
      setCrossfadeDurationMs: vi.fn(),
      setThemePreference: vi.fn(),
    },
  };
}

function createHarness(overrides?: {
  initialSettings?: Partial<AppSettingsSnapshot>;
}) {
  let snapshot: SettingsOverlaySnapshot = createInitialSettingsOverlaySnapshot(
    overrides?.initialSettings
      ? {
          hydrated: true,
          stemMode: "four_stem",
          modelVariant: "htdemucs_ft",
          language: "zh-CN",
          hideBatchSeparate: false,
          coverArtBackdrop: true,
          lyricsFontStep: 0,
          executionProvider: "xnnpack",
          availableExecutionProviders: ["cpu", "xnnpack"],
          eqEnabled: false,
          eqGainsDb: [0, 0, 0, 0, 0],
          crossfadeEnabled: false,
          crossfadeDurationMs: 3_000,
          librarySortMode: "recently_imported",
          themePreference: "dark",
          ...overrides.initialSettings,
        }
      : undefined,
  );

  const dependencies = createDependencies();

  const actions = createSettingsOverlayActions(dependencies, {
    getSnapshot: () => snapshot,
    setSnapshot: (updater) => {
      snapshot = updater(snapshot);
    },
  });

  return {
    actions,
    dependencies,
    getSnapshot: () => snapshot,
    setSnapshot: (next: SettingsOverlaySnapshot) => {
      snapshot = next;
    },
  };
}

describe("createInitialSettingsOverlaySnapshot", () => {
  test("returns the correct initial shape with default settings", () => {
    const snapshot = createInitialSettingsOverlaySnapshot();

    expect(snapshot).toEqual({
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
        language: "zh-CN",
        hideBatchSeparate: false,
        coverArtBackdrop: true,
        executionProvider: "xnnpack",
        availableExecutionProviders: ["cpu", "xnnpack"],
        eqEnabled: false,
        eqGainsDb: [0, 0, 0, 0, 0],
        crossfadeEnabled: false,
        crossfadeDurationMs: 3_000,
        librarySortMode: "recently_imported",
        themePreference: "dark",
        integrityReport: null,
        integritySelection: expect.any(Set),
        integritySkippedCount: null,
      },
      meta: {
        isInitializing: true,
        dangerDialog: null,
        stemsSize: null,
        downgradeSavings: null,
        deletingStemsInProgress: false,
        deletingLyricsInProgress: false,
        downgradingInProgress: false,
        integrityCheckInProgress: false,
        integrityCleanupInProgress: false,
      },
    });
  });

  test("defaults language to 'en' when initialSettings.language is null", () => {
    const snapshot = createInitialSettingsOverlaySnapshot({
      hydrated: true,
      stemMode: "two_stem",
      modelVariant: "htdemucs",
      language: null,
      hideBatchSeparate: false,
      coverArtBackdrop: false,
      lyricsFontStep: 0,
      executionProvider: "cpu",
      availableExecutionProviders: ["cpu"],
      eqEnabled: false,
      eqGainsDb: [0, 0, 0, 0, 0],
      crossfadeEnabled: false,
      crossfadeDurationMs: 3_000,
      librarySortMode: "recently_imported",
      themePreference: "dark",
    });

    expect(snapshot.state.language).toBe("en");
    expect(snapshot.state.stemMode).toBe("two_stem");
    expect(snapshot.state.modelVariant).toBe("htdemucs");
    expect(snapshot.state.coverArtBackdrop).toBe(false);
  });

  test("uses provided initialSettings values", () => {
    const snapshot = createInitialSettingsOverlaySnapshot({
      hydrated: true,
      stemMode: "four_stem",
      modelVariant: "htdemucs_ft",
      language: "ko",
      hideBatchSeparate: true,
      coverArtBackdrop: true,
      lyricsFontStep: 3,
      executionProvider: "xnnpack",
      availableExecutionProviders: ["cpu", "xnnpack"],
      eqEnabled: false,
      eqGainsDb: [0, 0, 0, 0, 0],
      crossfadeEnabled: false,
      crossfadeDurationMs: 3_000,
      librarySortMode: "recently_imported",
      themePreference: "dark",
    });

    expect(snapshot.state.language).toBe("ko");
    expect(snapshot.state.hideBatchSeparate).toBe(true);
  });
});

describe("createSettingsOverlayActions - initialize", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  test("fetches registry and settings, patches state on success", async () => {
    const harness = createHarness();

    vi.mocked(harness.dependencies.api.getLibraryRegistry).mockResolvedValue({
      active_library_id: "local:/music",
      libraries: [
        {
          id: "local:/music",
          kind: "local",
          display_name: "Music",
          root_path: "/music",
        },
      ],
    });
    vi.mocked(harness.dependencies.api.getSettings).mockResolvedValue({
      stem_mode: "two_stem",
      model_variant: "htdemucs",
      language: "en",
      hide_batch_separate: true,
      cover_art_backdrop: false,
      lyrics_font_step: 1,
      execution_provider: "cpu",
      available_execution_providers: ["cpu"],
      eq_enabled: false,
      eq_gains_db: [0, 0, 0, 0, 0],
      crossfade_enabled: false,
      crossfade_duration_ms: 3_000,
      library_sort_mode: "recently_imported",
      theme_preference: "dark",
    });
    vi.mocked(harness.dependencies.api.getModelStatus)
      .mockResolvedValueOnce({
        variant: "htdemucs",
        downloaded: true,
        legacy_install_present: false,
        file_size: 100,
      })
      .mockResolvedValueOnce({
        variant: "htdemucs_ft",
        downloaded: false,
        legacy_install_present: false,
        file_size: null,
      });
    vi.mocked(
      harness.dependencies.api.getRuntimeBootstrapStatus,
    ).mockResolvedValue({
      state: "ready",
      version: "1.0.0",
      runtime_path: "/tmp/runtime",
      downloaded_bytes: null,
      total_bytes: null,
      error: null,
    });

    await harness.actions.initialize();

    expect(
      harness.dependencies.settingsStore.hydrateAppSettings,
    ).toHaveBeenCalledWith({
      stem_mode: "two_stem",
      model_variant: "htdemucs",
      language: "en",
      hide_batch_separate: true,
      cover_art_backdrop: false,
      lyrics_font_step: 1,
      execution_provider: "cpu",
      available_execution_providers: ["cpu"],
      eq_enabled: false,
      eq_gains_db: [0, 0, 0, 0, 0],
      crossfade_enabled: false,
      crossfade_duration_ms: 3_000,
      library_sort_mode: "recently_imported",
      theme_preference: "dark",
    });
    expect(harness.getSnapshot().state.libraryPath).toBe("/music");
    expect(harness.getSnapshot().state.stemMode).toBe("two_stem");
    expect(harness.getSnapshot().state.modelVariant).toBe("htdemucs");
    expect(harness.getSnapshot().state.language).toBe("en");
    expect(harness.getSnapshot().state.hideBatchSeparate).toBe(true);
    expect(harness.getSnapshot().meta.isInitializing).toBe(false);
  });

  test("handles rejected registry promise gracefully", async () => {
    const harness = createHarness();

    vi.mocked(harness.dependencies.api.getLibraryRegistry).mockRejectedValue(
      new Error("registry unavailable"),
    );
    vi.mocked(harness.dependencies.api.getSettings).mockResolvedValue({
      stem_mode: "four_stem",
      model_variant: "htdemucs_ft",
      language: "ja",
      hide_batch_separate: false,
      cover_art_backdrop: true,
      lyrics_font_step: 0,
      execution_provider: "xnnpack",
      available_execution_providers: ["cpu", "xnnpack"],
      eq_enabled: false,
      eq_gains_db: [0, 0, 0, 0, 0],
      crossfade_enabled: false,
      crossfade_duration_ms: 3_000,
      library_sort_mode: "recently_imported",
      theme_preference: "dark",
    });
    vi.mocked(harness.dependencies.api.getModelStatus)
      .mockResolvedValueOnce({
        variant: "htdemucs",
        downloaded: true,
        legacy_install_present: false,
        file_size: 100,
      })
      .mockResolvedValueOnce({
        variant: "htdemucs_ft",
        downloaded: true,
        legacy_install_present: false,
        file_size: 200,
      });
    vi.mocked(
      harness.dependencies.api.getRuntimeBootstrapStatus,
    ).mockResolvedValue({
      state: "ready",
      version: "1.0.0",
      runtime_path: "/tmp/runtime",
      downloaded_bytes: null,
      total_bytes: null,
      error: null,
    });

    await harness.actions.initialize();

    expect(harness.dependencies.notifyError).toHaveBeenCalledWith(
      expect.any(Error),
    );
    // Settings still get applied even though registry failed
    expect(harness.getSnapshot().state.modelVariant).toBe("htdemucs_ft");
    expect(harness.getSnapshot().meta.isInitializing).toBe(false);
  });

  test("handles rejected settings promise gracefully", async () => {
    const harness = createHarness();

    vi.mocked(harness.dependencies.api.getLibraryRegistry).mockResolvedValue({
      active_library_id: null,
      libraries: [],
    });
    vi.mocked(harness.dependencies.api.getSettings).mockRejectedValue(
      new Error("settings unavailable"),
    );
    vi.mocked(harness.dependencies.api.getModelStatus)
      .mockResolvedValueOnce({
        variant: "htdemucs",
        downloaded: false,
        legacy_install_present: false,
        file_size: null,
      })
      .mockResolvedValueOnce({
        variant: "htdemucs_ft",
        downloaded: false,
        legacy_install_present: false,
        file_size: null,
      });
    vi.mocked(
      harness.dependencies.api.getRuntimeBootstrapStatus,
    ).mockResolvedValue({
      state: "ready",
      version: "1.0.0",
      runtime_path: "/tmp/runtime",
      downloaded_bytes: null,
      total_bytes: null,
      error: null,
    });

    await harness.actions.initialize();

    expect(harness.dependencies.notifyError).toHaveBeenCalledWith(
      expect.any(Error),
    );
    // Settings store hydration should not have been called
    expect(
      harness.dependencies.settingsStore.hydrateAppSettings,
    ).not.toHaveBeenCalled();
    expect(harness.getSnapshot().meta.isInitializing).toBe(false);
  });

  test("handles both registry and settings rejected", async () => {
    const harness = createHarness();

    vi.mocked(harness.dependencies.api.getLibraryRegistry).mockRejectedValue(
      new Error("registry failed"),
    );
    vi.mocked(harness.dependencies.api.getSettings).mockRejectedValue(
      new Error("settings failed"),
    );
    vi.mocked(harness.dependencies.api.getModelStatus)
      .mockResolvedValueOnce({
        variant: "htdemucs",
        downloaded: false,
        legacy_install_present: false,
        file_size: null,
      })
      .mockResolvedValueOnce({
        variant: "htdemucs_ft",
        downloaded: false,
        legacy_install_present: false,
        file_size: null,
      });
    vi.mocked(
      harness.dependencies.api.getRuntimeBootstrapStatus,
    ).mockResolvedValue({
      state: "ready",
      version: "1.0.0",
      runtime_path: "/tmp/runtime",
      downloaded_bytes: null,
      total_bytes: null,
      error: null,
    });

    await harness.actions.initialize();

    expect(harness.dependencies.notifyError).toHaveBeenCalledTimes(2);
    expect(harness.getSnapshot().meta.isInitializing).toBe(false);
  });
});
