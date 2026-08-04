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
        hideUpgradeAll: false,
        lyricsFontStep: 0,
        executionProvider: "xnnpack",
        availableExecutionProviders: ["cpu", "xnnpack"],
        compatibleExecutionProviders: ["cpu", "xnnpack"],
        eqEnabled: false,
        eqGainsDb: [0, 0, 0, 0, 0],
        crossfadeEnabled: false,
        crossfadeDurationMs: 3_000,
        librarySortMode: "recently_imported",
        themePreference: "dark",
        updatePolicy: "notify",
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
import type { RuntimeStatusView } from "./settings-overlay.types";

function createDependencies(): SettingsOverlayControllerDependencies {
  return {
    api: {
      createLibrary: vi.fn(),
      createLocalLibrary: vi.fn(),
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
      reauthorizeRemoteRepository: vi.fn(),
      setExecutionProvider: vi.fn(),
      setHideBatchSeparate: vi.fn(),
      setCoverArtBackdrop: vi.fn(),
      setHideUpgradeAll: vi.fn(),
      setLanguage: vi.fn(),
      setModelVariant: vi.fn(),
      setStemMode: vi.fn(),
      setEqEnabled: vi.fn(),
      setEqGains: vi.fn(),
      setCrossfadeEnabled: vi.fn(),
      setCrossfadeDurationMs: vi.fn(),
      setThemePreference: vi.fn(),
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
          hideUpgradeAll: false,
          lyricsFontStep: 0,
          executionProvider: "xnnpack",
          availableExecutionProviders: ["cpu", "xnnpack"],
          compatibleExecutionProviders: ["cpu", "xnnpack"],
          eqEnabled: false,
          eqGainsDb: [0, 0, 0, 0, 0],
          crossfadeEnabled: false,
          crossfadeDurationMs: 3_000,
          librarySortMode: "recently_imported",
          themePreference: "dark",
          updatePolicy: "notify",
        }),
      ),
      hydrateAppSettings: vi.fn(),
      patchAppSettings: vi.fn(),
      setEqEnabled: vi.fn(),
      setEqGains: vi.fn(),
      setCrossfadeEnabled: vi.fn(),
      setCrossfadeDurationMs: vi.fn(),
      setThemePreference: vi.fn(),
      setUpdatePolicy: vi.fn(),
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
          hideUpgradeAll: false,
          lyricsFontStep: 0,
          executionProvider: "xnnpack",
          availableExecutionProviders: ["cpu", "xnnpack"],
          compatibleExecutionProviders: ["cpu", "xnnpack"],
          eqEnabled: false,
          eqGainsDb: [0, 0, 0, 0, 0],
          crossfadeEnabled: false,
          crossfadeDurationMs: 3_000,
          librarySortMode: "recently_imported",
          themePreference: "dark",
          updatePolicy: "notify",
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
        modelUpdate: null,
        runtimeStatus: null,
        runtimeUpdate: null,
        language: "zh-CN",
        hideBatchSeparate: false,
        coverArtBackdrop: true,
        hideUpgradeAll: false,
        executionProvider: "xnnpack",
        availableExecutionProviders: ["cpu", "xnnpack"],
        compatibleExecutionProviders: ["cpu", "xnnpack"],
        eqEnabled: false,
        eqGainsDb: [0, 0, 0, 0, 0],
        crossfadeEnabled: false,
        crossfadeDurationMs: 3_000,
        librarySortMode: "recently_imported",
        themePreference: "dark",
        updatePolicy: "notify",
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
      hideUpgradeAll: false,
      lyricsFontStep: 0,
      executionProvider: "cpu",
      availableExecutionProviders: ["cpu"],
      compatibleExecutionProviders: ["cpu"],
      eqEnabled: false,
      eqGainsDb: [0, 0, 0, 0, 0],
      crossfadeEnabled: false,
      crossfadeDurationMs: 3_000,
      librarySortMode: "recently_imported",
      themePreference: "dark",
      updatePolicy: "notify",
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
      hideUpgradeAll: true,
      lyricsFontStep: 3,
      executionProvider: "xnnpack",
      availableExecutionProviders: ["cpu", "xnnpack"],
      compatibleExecutionProviders: ["cpu", "xnnpack"],
      eqEnabled: false,
      eqGainsDb: [0, 0, 0, 0, 0],
      crossfadeEnabled: false,
      crossfadeDurationMs: 3_000,
      librarySortMode: "recently_imported",
      themePreference: "dark",
      updatePolicy: "notify",
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
      hide_upgrade_all: true,
      lyrics_font_step: 1,
      execution_provider: "cpu",
      available_execution_providers: ["cpu"],
      compatible_execution_providers: ["cpu"],
      eq_enabled: false,
      eq_gains_db: [0, 0, 0, 0, 0],
      crossfade_enabled: false,
      crossfade_duration_ms: 3_000,
      library_sort_mode: "recently_imported",
      theme_preference: "dark",
      update_policy: "notify",
    });
    vi.mocked(harness.dependencies.api.getModelStatus)
      .mockResolvedValueOnce({
        variant: "htdemucs",
        downloaded: true,
        legacy_install_present: false,
        file_size_bytes: 100,
        installed_version: null,
        pinned_version: "model-v2.1.0",
      })
      .mockResolvedValueOnce({
        variant: "htdemucs_ft",
        downloaded: false,
        legacy_install_present: false,
        file_size_bytes: null,
        installed_version: null,
        pinned_version: "model-v2.1.0",
      });
    vi.mocked(
      harness.dependencies.api.getRuntimeBootstrapStatus,
    ).mockResolvedValue({
      state: "ready",
      version: "1.0.0",
      runtime_path: "/tmp/runtime",
      downloaded_bytes: null,
      total_bytes: null,
      active_artifact_id: "rt-1.0.0",
      target_triple: "aarch64-apple-darwin",
      candidate_version: null,
      restart_required: false,
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
      hide_upgrade_all: true,
      lyrics_font_step: 1,
      execution_provider: "cpu",
      available_execution_providers: ["cpu"],
      compatible_execution_providers: ["cpu"],
      eq_enabled: false,
      eq_gains_db: [0, 0, 0, 0, 0],
      crossfade_enabled: false,
      crossfade_duration_ms: 3_000,
      library_sort_mode: "recently_imported",
      theme_preference: "dark",
      update_policy: "notify",
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
      hide_upgrade_all: false,
      lyrics_font_step: 0,
      execution_provider: "xnnpack",
      available_execution_providers: ["cpu", "xnnpack"],
      compatible_execution_providers: ["cpu", "xnnpack"],
      eq_enabled: false,
      eq_gains_db: [0, 0, 0, 0, 0],
      crossfade_enabled: false,
      crossfade_duration_ms: 3_000,
      library_sort_mode: "recently_imported",
      theme_preference: "dark",
      update_policy: "notify",
    });
    vi.mocked(harness.dependencies.api.getModelStatus)
      .mockResolvedValueOnce({
        variant: "htdemucs",
        downloaded: true,
        legacy_install_present: false,
        file_size_bytes: 100,
        installed_version: null,
        pinned_version: "model-v2.1.0",
      })
      .mockResolvedValueOnce({
        variant: "htdemucs_ft",
        downloaded: true,
        legacy_install_present: false,
        file_size_bytes: 200,
        installed_version: null,
        pinned_version: "model-v2.1.0",
      });
    vi.mocked(
      harness.dependencies.api.getRuntimeBootstrapStatus,
    ).mockResolvedValue({
      state: "ready",
      version: "1.0.0",
      runtime_path: "/tmp/runtime",
      downloaded_bytes: null,
      total_bytes: null,
      active_artifact_id: "rt-1.0.0",
      target_triple: "aarch64-apple-darwin",
      candidate_version: null,
      restart_required: false,
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
        file_size_bytes: null,
        installed_version: null,
        pinned_version: "model-v2.1.0",
      })
      .mockResolvedValueOnce({
        variant: "htdemucs_ft",
        downloaded: false,
        legacy_install_present: false,
        file_size_bytes: null,
        installed_version: null,
        pinned_version: "model-v2.1.0",
      });
    vi.mocked(
      harness.dependencies.api.getRuntimeBootstrapStatus,
    ).mockResolvedValue({
      state: "ready",
      version: "1.0.0",
      runtime_path: "/tmp/runtime",
      downloaded_bytes: null,
      total_bytes: null,
      active_artifact_id: "rt-1.0.0",
      target_triple: "aarch64-apple-darwin",
      candidate_version: null,
      restart_required: false,
      error: null,
    });

    await harness.actions.initialize();

    expect(harness.dependencies.notifyError).toHaveBeenCalledWith(
      expect.any(Error),
    );
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
        file_size_bytes: null,
        installed_version: null,
        pinned_version: "model-v2.1.0",
      })
      .mockResolvedValueOnce({
        variant: "htdemucs_ft",
        downloaded: false,
        legacy_install_present: false,
        file_size_bytes: null,
        installed_version: null,
        pinned_version: "model-v2.1.0",
      });
    vi.mocked(
      harness.dependencies.api.getRuntimeBootstrapStatus,
    ).mockResolvedValue({
      state: "ready",
      version: "1.0.0",
      runtime_path: "/tmp/runtime",
      downloaded_bytes: null,
      total_bytes: null,
      active_artifact_id: "rt-1.0.0",
      target_triple: "aarch64-apple-darwin",
      candidate_version: null,
      restart_required: false,
      error: null,
    });

    await harness.actions.initialize();

    expect(harness.dependencies.notifyError).toHaveBeenCalledTimes(2);
    expect(harness.getSnapshot().meta.isInitializing).toBe(false);
  });
});

describe("createSettingsOverlayActions - runtime updates", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  function runtimeStatusWithState(
    state: RuntimeStatusView["state"],
  ): RuntimeStatusView {
    return {
      state,
      version: "v1.27.1",
      runtime_path: "/tmp/runtime",
      active_artifact_id: "rt-1.27.1",
      target_triple: "aarch64-apple-darwin",
      candidate_version: null,
      restart_required: false,
      error: null,
    };
  }

  test.each(["installing", "probing", "activating"] as const)(
    "checkRuntimeUpdates and updateRuntime return early while runtime is %s",
    async (state) => {
      const harness = createHarness();
      harness.setSnapshot({
        ...harness.getSnapshot(),
        state: {
          ...harness.getSnapshot().state,
          runtimeStatus: runtimeStatusWithState(state),
        },
      });

      await harness.actions.checkRuntimeUpdates();
      expect(
        harness.dependencies.api.checkRuntimeUpdates,
      ).not.toHaveBeenCalled();

      await harness.actions.updateRuntime();
      expect(harness.dependencies.api.downloadRuntime).not.toHaveBeenCalled();
    },
  );

  test("checkRuntimeUpdates and updateRuntime run normally when runtime is not busy", async () => {
    const harness = createHarness();
    harness.setSnapshot({
      ...harness.getSnapshot(),
      state: {
        ...harness.getSnapshot().state,
        runtimeStatus: runtimeStatusWithState("ready"),
      },
    });

    vi.mocked(harness.dependencies.api.checkRuntimeUpdates).mockResolvedValue({
      generation: 8,
      release_id: "2026-08-01-002",
      target_triple: "aarch64-apple-darwin",
      state: "not_installed",
      installed_version: null,
      available_version: "v1.28.0",
      available_bytes: 42_000_000,
      restart_required: false,
    });
    vi.mocked(
      harness.dependencies.api.getRuntimeBootstrapStatus,
    ).mockResolvedValue({
      state: "ready",
      version: "v1.27.1",
      runtime_path: "/tmp/runtime",
      downloaded_bytes: null,
      total_bytes: null,
      active_artifact_id: "rt-1.27.1",
      target_triple: "aarch64-apple-darwin",
      candidate_version: null,
      restart_required: false,
      error: null,
    });

    await harness.actions.checkRuntimeUpdates();
    expect(harness.dependencies.api.checkRuntimeUpdates).toHaveBeenCalledOnce();

    vi.mocked(harness.dependencies.api.downloadRuntime).mockResolvedValue({
      state: "ready",
      version: "v1.28.0",
      runtime_path: "/tmp/runtime",
      downloaded_bytes: null,
      total_bytes: null,
      active_artifact_id: "rt-1.28.0",
      target_triple: "aarch64-apple-darwin",
      candidate_version: null,
      restart_required: false,
      error: null,
    });

    await harness.actions.updateRuntime();
    expect(harness.dependencies.api.downloadRuntime).toHaveBeenCalledOnce();
  });

  test("checkRuntimeUpdates caches the report and refreshes runtime status", async () => {
    const harness = createHarness();

    const report = {
      generation: 7,
      release_id: "2026-08-01-001",
      target_triple: "aarch64-apple-darwin",
      state: "update_available" as const,
      installed_version: "v1.27.1",
      available_version: "v1.28.0",
      available_bytes: 42_000_000,
      restart_required: true,
    };
    vi.mocked(harness.dependencies.api.checkRuntimeUpdates).mockResolvedValue(
      report,
    );
    vi.mocked(
      harness.dependencies.api.getRuntimeBootstrapStatus,
    ).mockResolvedValue({
      state: "update_available",
      version: "v1.27.1",
      runtime_path: "/tmp/runtime",
      downloaded_bytes: null,
      total_bytes: null,
      active_artifact_id: "rt-1.27.1",
      target_triple: "aarch64-apple-darwin",
      candidate_version: null,
      restart_required: false,
      error: null,
    });

    await harness.actions.checkRuntimeUpdates();

    expect(harness.dependencies.api.checkRuntimeUpdates).toHaveBeenCalledOnce();
    expect(
      harness.dependencies.api.getRuntimeBootstrapStatus,
    ).toHaveBeenCalled();
    const runtimeUpdate = harness.getSnapshot().state.runtimeUpdate;
    expect(runtimeUpdate?.status).toBe("checked");
    expect(runtimeUpdate?.report).toEqual(report);
  });

  test("checkRuntimeUpdates records a failure without notifying", async () => {
    const harness = createHarness();

    vi.mocked(harness.dependencies.api.checkRuntimeUpdates).mockRejectedValue(
      new Error("offline"),
    );

    await harness.actions.checkRuntimeUpdates();

    const runtimeUpdate = harness.getSnapshot().state.runtimeUpdate;
    expect(runtimeUpdate?.status).toBe("failed");
    expect(runtimeUpdate?.error).toBe("offline");
    expect(harness.dependencies.notifyError).not.toHaveBeenCalled();
  });

  test("updateRuntime stages the candidate and mirrors the returned status", async () => {
    const harness = createHarness();

    vi.mocked(harness.dependencies.api.downloadRuntime).mockResolvedValue({
      state: "candidate_ready_restart_required",
      version: "v1.27.1",
      runtime_path: "/tmp/runtime",
      downloaded_bytes: null,
      total_bytes: null,
      active_artifact_id: "rt-1.27.1",
      target_triple: "aarch64-apple-darwin",
      candidate_version: "v1.28.0",
      restart_required: true,
      error: null,
    });

    await harness.actions.updateRuntime();

    expect(harness.dependencies.api.downloadRuntime).toHaveBeenCalledOnce();
    const runtimeStatus = harness.getSnapshot().state.runtimeStatus;
    expect(runtimeStatus?.state).toBe("candidate_ready_restart_required");
    expect(runtimeStatus?.candidate_version).toBe("v1.28.0");
    expect(runtimeStatus?.restart_required).toBe(true);
    expect(harness.getSnapshot().state.runtimeUpdate).toBeNull();
  });

  test("downloadRuntime discards the stale check report after installing", async () => {
    const harness = createHarness();

    vi.mocked(harness.dependencies.api.checkRuntimeUpdates).mockResolvedValue({
      generation: 9,
      release_id: "2026-08-01-001",
      target_triple: "aarch64-apple-darwin",
      state: "not_installed",
      installed_version: null,
      available_version: "v1.27.1",
      available_bytes: 42_000_000,
      restart_required: false,
    });
    vi.mocked(
      harness.dependencies.api.getRuntimeBootstrapStatus,
    ).mockResolvedValue({
      state: "missing",
      version: "v1.27.1",
      runtime_path: "",
      downloaded_bytes: null,
      total_bytes: null,
      active_artifact_id: null,
      target_triple: "aarch64-apple-darwin",
      candidate_version: null,
      restart_required: false,
      error: null,
    });

    await harness.actions.checkRuntimeUpdates();
    expect(harness.getSnapshot().state.runtimeUpdate?.report?.state).toBe(
      "not_installed",
    );

    vi.mocked(harness.dependencies.api.downloadRuntime).mockResolvedValue({
      state: "ready",
      version: "v1.27.1",
      runtime_path: "/tmp/runtime",
      downloaded_bytes: null,
      total_bytes: null,
      active_artifact_id: "rt-1.27.1",
      target_triple: "aarch64-apple-darwin",
      candidate_version: null,
      restart_required: false,
      error: null,
    });

    await harness.actions.downloadRuntime();

    expect(harness.getSnapshot().state.runtimeStatus?.state).toBe("ready");
    expect(harness.getSnapshot().state.runtimeUpdate).toBeNull();
  });

  test("setUpdatePolicy routes through the settings store and mirrors the result", async () => {
    const harness = createHarness();

    vi.mocked(
      harness.dependencies.settingsStore.getAppSettingsSnapshot,
    ).mockReturnValue({
      hydrated: true,
      stemMode: "four_stem",
      modelVariant: "htdemucs_ft",
      language: "zh-CN",
      hideBatchSeparate: false,
      coverArtBackdrop: true,
      hideUpgradeAll: false,
      lyricsFontStep: 0,
      executionProvider: "xnnpack",
      availableExecutionProviders: ["cpu", "xnnpack"],
      compatibleExecutionProviders: ["cpu", "xnnpack"],
      eqEnabled: false,
      eqGainsDb: [0, 0, 0, 0, 0],
      crossfadeEnabled: false,
      crossfadeDurationMs: 3_000,
      librarySortMode: "recently_imported",
      themePreference: "dark",
      updatePolicy: "auto_download",
    });

    await harness.actions.setUpdatePolicy("auto_download");

    expect(
      harness.dependencies.settingsStore.setUpdatePolicy,
    ).toHaveBeenCalledWith("auto_download");
    expect(harness.getSnapshot().state.updatePolicy).toBe("auto_download");
  });
});
