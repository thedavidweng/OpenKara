// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { createSettingsHarness } from "@/test-utils/settings-controller";
import type {
  AppSettings,
  IntegrityReport,
  LibraryRegistrySnapshot,
  ManagedAssetIssue,
  ModelStatusSnapshot,
  RegisteredLibrary,
  RuntimeBootstrapStatusSnapshot,
  SeparationStatusSnapshot,
} from "@/types/ipc";
import { describeLibrary } from "./settings-controller";

const localLibrary: RegisteredLibrary = {
  id: "local:/karaoke",
  kind: "local",
  display_name: "karaoke",
  root_path: "/karaoke",
};

const driveRepository: RegisteredLibrary = {
  id: "remote:drive-1",
  kind: "remote",
  display_name: "Drive",
  provider: "google_drive",
  account_id: "account-1",
  remote_root_locator: "root-1",
  remote_path_display: "Google Drive / OpenKara",
  connection_config: null,
  cached_db_path: null,
  remote_revision: "rev-1",
};

const dropboxRepository: RegisteredLibrary = {
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

const BASE_SETTINGS: AppSettings = {
  stem_mode: "four_stem",
  model_variant: "htdemucs",
  language: "en",
  hide_batch_separate: false,
  cover_art_backdrop: true,
  hide_upgrade_all: false,
  lyrics_font_step: 0,
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
};

function appSettings(patch: Partial<AppSettings> = {}): AppSettings {
  return { ...BASE_SETTINGS, ...patch };
}

function registryOf(
  activeLibraryId: string | null,
  libraries: RegisteredLibrary[],
): LibraryRegistrySnapshot {
  return { active_library_id: activeLibraryId, libraries };
}

function modelStatus(patch: Partial<ModelStatusSnapshot> = {}) {
  return {
    variant: "htdemucs",
    downloaded: false,
    legacy_install_present: false,
    file_size_bytes: null,
    installed_version: null,
    pinned_version: "model-v2.1.0",
    ...patch,
  } satisfies ModelStatusSnapshot;
}

function runtimeStatus(
  patch: Partial<RuntimeBootstrapStatusSnapshot> = {},
): RuntimeBootstrapStatusSnapshot {
  return {
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
    ...patch,
  };
}

const issue = (song_hash: string): ManagedAssetIssue => ({
  song_hash,
  asset_type: "primary_media",
  path: `media/${song_hash}.mp3`,
});

const emptyReport: IntegrityReport = {
  checked_local_songs: 0,
  skipped_remote_songs: 0,
  missing_primary_media: [],
  empty_primary_media: [],
  missing_optional_assets: [],
  empty_optional_assets: [],
  orphaned_managed_files: [],
};

const sampleReport: IntegrityReport = {
  ...emptyReport,
  checked_local_songs: 3,
  skipped_remote_songs: 1,
  missing_primary_media: [issue("hash-missing")],
  empty_primary_media: [issue("hash-empty")],
  missing_optional_assets: [
    { song_hash: "hash-optional", asset_type: "cdg", path: "media-g/a.cdg" },
  ],
  orphaned_managed_files: ["stems/orphan.wav"],
};

beforeEach(() => {
  vi.restoreAllMocks();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("describeLibrary", () => {
  test("returns the root path for a Local Working Copy", () => {
    expect(describeLibrary(localLibrary)).toBe("/karaoke");
  });

  test("names the Remote Repository and its remote path", () => {
    expect(describeLibrary(driveRepository)).toBe(
      "Drive · Google Drive / OpenKara",
    );
  });

  test("falls back to the remote locator when there is no display path", () => {
    expect(describeLibrary(dropboxRepository)).toBe(
      "Dropbox · /shared/openkara",
    );
  });
});

describe("initialize", () => {
  test("publishes the registry, preferences, model and runtime status", async () => {
    const harness = createSettingsHarness({
      overrides: {
        librarySetup: {
          getLibraryRegistry: async () =>
            registryOf(localLibrary.id, [localLibrary]),
        },
        settings: {
          getSettings: async () =>
            appSettings({ stem_mode: "two_stem", hide_batch_separate: true }),
          getModelStatus: async (variant) =>
            modelStatus({
              variant,
              downloaded: variant === "htdemucs",
              file_size_bytes: variant === "htdemucs" ? 123 : null,
            }),
          getRuntimeBootstrapStatus: async () => runtimeStatus(),
        },
      },
    });

    expect(harness.view().isInitializing).toBe(true);

    await harness.controller.initialize();

    const view = harness.view();
    expect(view.isInitializing).toBe(false);
    expect(view.library.activeLibraryId).toBe(localLibrary.id);
    expect(view.library.activeLibraryPath).toBe("/karaoke");
    expect(view.library.libraries).toHaveLength(1);
    expect(view.preferences.stemMode).toBe("two_stem");
    expect(view.preferences.hideBatchSeparate).toBe(true);
    expect(view.models.statuses.htdemucs?.downloaded).toBe(true);
    expect(view.models.statuses.htdemucs_ft?.downloaded).toBe(false);
    expect(view.runtime.status?.state).toBe("ready");
  });

  test("resolves the stored app language once, for ADR-0021", async () => {
    const harness = createSettingsHarness({
      overrides: {
        settings: { getSettings: async () => appSettings({ language: "ko" }) },
      },
    });

    await harness.controller.initialize();

    expect(harness.view().preferences.language).toBe("ko");
    expect(harness.preferencesStore.getState().language).toBe("ko");
  });

  test("falls back to the host locale only while no language is stored", async () => {
    const harness = createSettingsHarness({
      overrides: {
        settings: { getSettings: async () => appSettings({ language: null }) },
      },
    });

    await harness.controller.initialize();

    expect(harness.view().preferences.language).not.toBeNull();
    expect(harness.view().preferences.language.length).toBeGreaterThan(0);
    expect(harness.preferencesStore.getState().language).toBeNull();
  });

  test("reports a failed registry read and still applies preferences", async () => {
    const harness = createSettingsHarness({
      overrides: {
        librarySetup: {
          getLibraryRegistry: async () => {
            throw new Error("registry unavailable");
          },
        },
        settings: {
          getSettings: async () =>
            appSettings({ model_variant: "htdemucs_ft" }),
        },
      },
    });

    await harness.controller.initialize();

    expect(harness.notifyError).toHaveBeenCalledWith(expect.any(Error));
    expect(harness.view().preferences.modelVariant).toBe("htdemucs_ft");
    expect(harness.view().isInitializing).toBe(false);
  });

  test("reports both failures without leaving the view initializing", async () => {
    const harness = createSettingsHarness({
      overrides: {
        librarySetup: {
          getLibraryRegistry: async () => {
            throw new Error("registry failed");
          },
        },
        settings: {
          getSettings: async () => {
            throw new Error("settings failed");
          },
        },
      },
    });

    await harness.controller.initialize();

    expect(harness.notifyError).toHaveBeenCalledTimes(2);
    expect(harness.view().isInitializing).toBe(false);
  });
});

describe("library commands", () => {
  test("creating a Local Working Copy delegates to the library session", async () => {
    const harness = createSettingsHarness();
    harness.selectDirectory.mockResolvedValue("/music");

    await harness.controller.library.create("Create library");

    expect(harness.librarySession.calls).toEqual([
      { entry: "createLocalLibrary", parentDirectory: "/music" },
    ]);
  });

  test("creating without a chosen directory does nothing", async () => {
    const harness = createSettingsHarness();
    harness.selectDirectory.mockResolvedValue(null);

    await harness.controller.library.create("Create library");

    expect(harness.librarySession.calls).toEqual([]);
  });

  test("opening a Local Working Copy delegates to the library session", async () => {
    const harness = createSettingsHarness();
    harness.selectDirectory.mockResolvedValue("/existing");

    await harness.controller.library.open("Open library");

    expect(harness.librarySession.calls).toEqual([
      { entry: "openLocalLibrary", directory: "/existing" },
    ]);
  });

  test("a failed session entry surfaces as the library error", async () => {
    const harness = createSettingsHarness();
    harness.selectDirectory.mockResolvedValue("/music");
    harness.librarySession.failOn("createLocalLibrary", new Error("disk full"));

    await harness.controller.library.create("Create library");

    expect(harness.view().library.error).toBe("disk full");
  });

  test("the session's registry view refreshes the library slice", async () => {
    const harness = createSettingsHarness({
      overrides: {
        librarySetup: {
          getLibraryRegistry: async () =>
            registryOf("local:/music/OpenKara", [
              {
                id: "local:/music/OpenKara",
                kind: "local",
                display_name: "OpenKara",
                root_path: "/music/OpenKara",
              },
            ]),
        },
      },
    });
    harness.selectDirectory.mockResolvedValue("/music");

    await harness.controller.library.create("Create library");
    await harness.librarySession.views?.refreshRegistry();

    expect(harness.view().library.activeLibraryPath).toBe("/music/OpenKara");
  });

  test("activating a library delegates instead of calling the backend", async () => {
    const switchLibrary = vi.fn();
    const harness = createSettingsHarness({
      overrides: { librarySetup: { switchLibrary } },
    });

    await harness.controller.library.activate(driveRepository.id);

    expect(harness.librarySession.calls).toEqual([
      { entry: "switchLibrary", libraryId: driveRepository.id },
    ]);
    expect(switchLibrary).not.toHaveBeenCalled();
  });

  test("a successful activation reports ok to its caller", async () => {
    const harness = createSettingsHarness();

    const result = await harness.controller.library.activate(
      driveRepository.id,
    );

    expect(result).toEqual({ ok: true });
    expect(harness.view().library.error).toBeNull();
  });

  test("a failed activation reports its error to the caller", async () => {
    const harness = createSettingsHarness();
    harness.librarySession.failOn(
      "switchLibrary",
      new Error("endpoint unreachable"),
    );

    const result = await harness.controller.library.activate(
      driveRepository.id,
    );

    expect(result).toEqual({ ok: false, error: "endpoint unreachable" });
    expect(harness.view().library.error).toBe("endpoint unreachable");
  });

  test("refreshing an active Remote Repository refreshes in place", async () => {
    const harness = createSettingsHarness({
      overrides: {
        librarySetup: {
          getLibraryRegistry: async () =>
            registryOf(driveRepository.id, [localLibrary, driveRepository]),
        },
      },
    });
    await harness.controller.initialize();

    await harness.controller.library.refresh(driveRepository.id);

    expect(harness.librarySession.calls).toEqual([
      { entry: "refreshRepository" },
    ]);
  });

  test("refreshing an inactive Remote Repository switches to it first", async () => {
    const harness = createSettingsHarness({
      overrides: {
        librarySetup: {
          getLibraryRegistry: async () =>
            registryOf(localLibrary.id, [localLibrary, driveRepository]),
        },
      },
    });
    await harness.controller.initialize();

    await harness.controller.library.refresh(driveRepository.id);

    expect(harness.librarySession.calls).toEqual([
      { entry: "switchLibrary", libraryId: driveRepository.id },
    ]);
  });

  test("refreshing ignores libraries that are not Remote Repositories", async () => {
    const harness = createSettingsHarness({
      overrides: {
        librarySetup: {
          getLibraryRegistry: async () =>
            registryOf(localLibrary.id, [localLibrary]),
        },
      },
    });
    await harness.controller.initialize();

    await harness.controller.library.refresh(localLibrary.id);

    expect(harness.librarySession.calls).toEqual([]);
  });

  test("renaming trims the name and hands the registry to the session", async () => {
    const renamed = registryOf(localLibrary.id, [
      { ...localLibrary, display_name: "New Name" },
    ]);
    const renameLibrary = vi.fn(async () => renamed);
    const harness = createSettingsHarness({
      overrides: {
        librarySetup: {
          renameLibrary,
          getLibraryRegistry: async () =>
            registryOf(localLibrary.id, [localLibrary]),
        },
      },
    });
    await harness.controller.initialize();

    await harness.controller.library.rename(localLibrary.id, "  New Name  ");

    expect(renameLibrary).toHaveBeenCalledWith(localLibrary.id, "New Name");
    expect(harness.librarySession.calls).toEqual([
      { entry: "adoptRegistry", registry: renamed },
    ]);
  });

  test("renaming to the same or an empty name does nothing", async () => {
    const renameLibrary = vi.fn();
    const harness = createSettingsHarness({
      overrides: {
        librarySetup: {
          renameLibrary,
          getLibraryRegistry: async () =>
            registryOf(localLibrary.id, [localLibrary]),
        },
      },
    });
    await harness.controller.initialize();

    await harness.controller.library.rename(localLibrary.id, "   ");
    await harness.controller.library.rename(localLibrary.id, "karaoke");

    expect(renameLibrary).not.toHaveBeenCalled();
  });

  test("a failed rename surfaces as the library error", async () => {
    const harness = createSettingsHarness({
      overrides: {
        librarySetup: {
          renameLibrary: async () => {
            throw new Error("rename failed");
          },
          getLibraryRegistry: async () =>
            registryOf(localLibrary.id, [localLibrary]),
        },
      },
    });
    await harness.controller.initialize();

    await harness.controller.library.rename(localLibrary.id, "New Name");

    expect(harness.view().library.error).toBe("rename failed");
  });

  test("Disconnect Repository runs only after the prompt is accepted", async () => {
    const removeLibrary = vi.fn(async () => registryOf(null, []));
    const harness = createSettingsHarness({
      overrides: {
        librarySetup: {
          removeLibrary,
          getLibraryRegistry: async () =>
            registryOf(localLibrary.id, [localLibrary]),
        },
      },
    });
    await harness.controller.initialize();

    vi.spyOn(window, "confirm").mockReturnValue(false);
    await harness.controller.library.disconnect(localLibrary.id);
    expect(removeLibrary).not.toHaveBeenCalled();

    vi.spyOn(window, "confirm").mockReturnValue(true);
    await harness.controller.library.disconnect(localLibrary.id);
    expect(removeLibrary).toHaveBeenCalledWith(localLibrary.id);
    expect(harness.librarySession.calls).toEqual([
      { entry: "adoptRegistry", registry: registryOf(null, []) },
    ]);
  });

  test("Delete Repository names the provider-hosted content it removes", async () => {
    const deleteLibrary = vi.fn(async () => registryOf(null, []));
    const harness = createSettingsHarness({
      overrides: {
        librarySetup: {
          deleteLibrary,
          getLibraryRegistry: async () =>
            registryOf(driveRepository.id, [driveRepository]),
        },
      },
    });
    await harness.controller.initialize();
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);

    await harness.controller.library.delete(driveRepository.id, "Drive");

    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining(
        "delete the remote repository contents from Google Drive",
      ),
    );
    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining("Google Drive / OpenKara"),
    );
    expect(deleteLibrary).toHaveBeenCalledWith(driveRepository.id);
  });

  test("Delete Repository stops when the typed name does not match", async () => {
    const deleteLibrary = vi.fn(async () => registryOf(null, []));
    const harness = createSettingsHarness({
      overrides: {
        librarySetup: {
          deleteLibrary,
          getLibraryRegistry: async () =>
            registryOf(driveRepository.id, [driveRepository]),
        },
      },
    });
    await harness.controller.initialize();
    vi.spyOn(window, "confirm").mockReturnValue(true);

    await harness.controller.library.delete(driveRepository.id, "Wrong");

    expect(deleteLibrary).not.toHaveBeenCalled();
  });
});

describe("preferences", () => {
  test("choosing a language switches i18n before the backend confirms", async () => {
    const setLanguage = vi.fn(async () => appSettings({ language: "ja" }));
    const harness = createSettingsHarness({
      overrides: { settings: { setLanguage } },
    });

    await harness.controller.preferences.set({ language: "ja" });

    expect(harness.changeLanguage).toHaveBeenCalledWith("ja");
    expect(setLanguage).toHaveBeenCalledWith("ja");
    expect(harness.view().preferences.language).toBe("ja");
  });

  test("a failed language write is reported and keeps the chosen language", async () => {
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          setLanguage: async () => {
            throw new Error("lang fail");
          },
        },
      },
    });

    await harness.controller.preferences.set({ language: "fr" });

    expect(harness.notifyError).toHaveBeenCalledWith(
      expect.objectContaining({ message: "lang fail" }),
    );
    expect(harness.view().preferences.language).toBe("fr");
  });

  test("stem mode and execution provider are written through the backend", async () => {
    let stored = appSettings();
    const setStemMode = vi.fn(async (mode: string) => {
      stored = {
        ...stored,
        stem_mode: mode === "two_stem" ? "two_stem" : "four_stem",
      };
      return stored;
    });
    const setExecutionProvider = vi.fn(async () => {
      stored = {
        ...stored,
        execution_provider: "xnnpack",
        available_execution_providers: ["cpu", "xnnpack"],
        compatible_execution_providers: ["cpu", "xnnpack"],
      };
      return stored;
    });
    const harness = createSettingsHarness({
      overrides: { settings: { setStemMode, setExecutionProvider } },
    });

    await harness.controller.preferences.set({ stemMode: "two_stem" });
    await harness.controller.preferences.set({ executionProvider: "xnnpack" });

    expect(setStemMode).toHaveBeenCalledWith("two_stem");
    expect(setExecutionProvider).toHaveBeenCalledWith("xnnpack");
    expect(harness.view().preferences.stemMode).toBe("two_stem");
    expect(harness.view().preferences.executionProvider).toBe("xnnpack");
    expect(harness.view().preferences.availableExecutionProviders).toEqual([
      "cpu",
      "xnnpack",
    ]);
  });

  test("a failed stem mode write is reported", async () => {
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          setStemMode: async () => {
            throw new Error("stem fail");
          },
        },
      },
    });

    await harness.controller.preferences.set({ stemMode: "two_stem" });

    expect(harness.notifyError).toHaveBeenCalledWith(expect.any(Error));
  });

  test("interface toggles show immediately and are written through", async () => {
    let stored = appSettings();
    const setHideBatchSeparate = vi.fn(async (value: boolean) => {
      stored = { ...stored, hide_batch_separate: value };
      return stored;
    });
    const setHideUpgradeAll = vi.fn(async (value: boolean) => {
      stored = { ...stored, hide_upgrade_all: value };
      return stored;
    });
    const setCoverArtBackdrop = vi.fn(async (value: boolean) => {
      stored = { ...stored, cover_art_backdrop: value };
      return stored;
    });
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          setHideBatchSeparate,
          setHideUpgradeAll,
          setCoverArtBackdrop,
        },
      },
    });

    await harness.controller.preferences.set({ hideBatchSeparate: true });
    await harness.controller.preferences.set({ hideUpgradeAll: true });
    await harness.controller.preferences.set({ coverArtBackdrop: false });

    expect(setHideBatchSeparate).toHaveBeenCalledWith(true);
    expect(setHideUpgradeAll).toHaveBeenCalledWith(true);
    expect(setCoverArtBackdrop).toHaveBeenCalledWith(false);
    expect(harness.view().preferences.hideBatchSeparate).toBe(true);
    expect(harness.view().preferences.hideUpgradeAll).toBe(true);
    expect(harness.view().preferences.coverArtBackdrop).toBe(false);
  });

  test("the equaliser clamps gains to ±12 dB", async () => {
    const setEqGains = vi.fn(async () =>
      appSettings({ eq_gains_db: [12, -12, 0, 0, 0] }),
    );
    const harness = createSettingsHarness({
      overrides: { settings: { setEqGains } },
    });

    await harness.controller.preferences.set({ eqGainsDb: [20, -20, 0, 0, 0] });

    expect(setEqGains).toHaveBeenCalledWith([12, -12, 0, 0, 0]);
    expect(harness.view().preferences.eqGainsDb).toEqual([12, -12, 0, 0, 0]);
  });

  test("unchanged gains and crossfade durations are not written", async () => {
    const setEqGains = vi.fn(async () => appSettings());
    const setCrossfadeDurationMs = vi.fn(async () => appSettings());
    const harness = createSettingsHarness({
      overrides: { settings: { setEqGains, setCrossfadeDurationMs } },
    });

    await harness.controller.preferences.set({ eqGainsDb: [0, 0, 0, 0, 0] });
    await harness.controller.preferences.set({ crossfadeDurationMs: 3_000 });

    expect(setEqGains).not.toHaveBeenCalled();
    expect(setCrossfadeDurationMs).not.toHaveBeenCalled();
  });

  test("the crossfade duration is clamped and rounded", async () => {
    const setCrossfadeDurationMs = vi.fn(async () =>
      appSettings({ crossfade_duration_ms: 500 }),
    );
    const harness = createSettingsHarness({
      overrides: { settings: { setCrossfadeDurationMs } },
    });

    await harness.controller.preferences.set({ crossfadeDurationMs: 100 });

    expect(setCrossfadeDurationMs).toHaveBeenCalledWith(500);
    expect(harness.view().preferences.crossfadeDurationMs).toBe(500);
  });

  test("the equaliser and crossfade switches roll back with the store", async () => {
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          setEqEnabled: async () => {
            throw new Error("eq fail");
          },
          setCrossfadeEnabled: async () => {
            throw new Error("crossfade fail");
          },
        },
      },
    });

    await harness.controller.preferences.set({ eqEnabled: true });
    await harness.controller.preferences.set({ crossfadeEnabled: true });

    expect(harness.view().preferences.eqEnabled).toBe(false);
    expect(harness.view().preferences.crossfadeEnabled).toBe(false);
  });

  test("theme and update policy mirror the store after it settles", async () => {
    const harness = createSettingsHarness();

    await harness.controller.preferences.set({ themePreference: "light" });
    await harness.controller.preferences.set({ updatePolicy: "auto_download" });

    expect(harness.view().preferences.themePreference).toBe("light");
    expect(harness.view().preferences.updatePolicy).toBe("auto_download");
  });

  test("a failed theme write rolls the view back", async () => {
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          setThemePreference: async () => {
            throw new Error("theme fail");
          },
        },
      },
    });

    await harness.controller.preferences.set({ themePreference: "light" });

    expect(harness.view().preferences.themePreference).toBe("dark");
  });
});

describe("model variant", () => {
  test("selecting the fine-tuned model asks for confirmation first", async () => {
    const setModelVariant = vi.fn(async () => appSettings());
    const harness = createSettingsHarness({
      overrides: { settings: { setModelVariant } },
    });

    await harness.controller.preferences.selectModelVariant("htdemucs_ft");

    expect(harness.view().dialog).toBe("ft_warning");
    expect(setModelVariant).not.toHaveBeenCalled();
  });

  test("confirming the fine-tuned dialog applies the variant", async () => {
    const setModelVariant = vi.fn(async () =>
      appSettings({ model_variant: "htdemucs_ft" }),
    );
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          setModelVariant,
          getModelStatus: async (variant) =>
            modelStatus({ variant, downloaded: true }),
        },
      },
    });
    await harness.controller.initialize();

    await harness.controller.preferences.selectModelVariant("htdemucs_ft");
    await harness.controller.maintenance.confirmDialog();

    expect(harness.view().dialog).toBeNull();
    expect(setModelVariant).toHaveBeenCalledWith("htdemucs_ft");
    expect(harness.view().preferences.modelVariant).toBe("htdemucs_ft");
  });

  test("selecting a missing model downloads it before applying", async () => {
    const downloadModel = vi.fn(async () => ({
      state: "ready" as const,
      model_path: "/tmp/model",
      downloaded_bytes: null,
      total_bytes: null,
      error: null,
    }));
    const harness = createSettingsHarness({
      preferences: { modelVariant: "htdemucs_ft" },
      overrides: {
        settings: {
          downloadModel,
          getModelStatus: async (variant) =>
            modelStatus({ variant, downloaded: variant === "htdemucs_ft" }),
          setModelVariant: async () =>
            appSettings({ model_variant: "htdemucs" }),
        },
      },
    });
    await harness.controller.initialize();

    await harness.controller.preferences.selectModelVariant("htdemucs");

    expect(downloadModel).toHaveBeenCalledWith("htdemucs");
    expect(harness.stores.modelBootstrap.reload).toHaveBeenCalled();
    expect(harness.view().models.downloading).toBeNull();
    expect(harness.view().preferences.modelVariant).toBe("htdemucs");
  });

  test("a failed model download clears the downloading flag and reports", async () => {
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          downloadModel: async () => {
            throw new Error("download failed");
          },
          getModelStatus: async (variant) => modelStatus({ variant }),
        },
      },
    });
    await harness.controller.initialize();

    await harness.controller.maintenance.downloadModel("htdemucs_ft");

    expect(harness.view().models.downloading).toBeNull();
    expect(harness.notifyError).toHaveBeenCalledWith(expect.any(Error));
  });

  test("a second download of the same variant is ignored", async () => {
    let resolveDownload: () => void = () => {};
    const downloadModel = vi.fn(
      () =>
        new Promise<never>((_, reject) => {
          resolveDownload = () => reject(new Error("cancelled"));
        }),
    );
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          downloadModel,
          getModelStatus: async (variant) => modelStatus({ variant }),
        },
      },
    });

    const first = harness.controller.maintenance.downloadModel("htdemucs");
    await harness.controller.maintenance.downloadModel("htdemucs");
    resolveDownload();
    await first;

    expect(downloadModel).toHaveBeenCalledTimes(1);
  });

  test("deleting a model refreshes statuses and the bootstrap store", async () => {
    const deleteModel = vi.fn(async () => {});
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          deleteModel,
          getModelStatus: async (variant) => modelStatus({ variant }),
        },
      },
    });

    await harness.controller.maintenance.deleteModel("htdemucs");

    expect(deleteModel).toHaveBeenCalledWith("htdemucs");
    expect(harness.stores.modelBootstrap.reload).toHaveBeenCalled();
  });

  test("a failed model deletion is reported", async () => {
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          deleteModel: async () => {
            throw new Error("delete failed");
          },
        },
      },
    });

    await harness.controller.maintenance.deleteModel("htdemucs_ft");

    expect(harness.notifyError).toHaveBeenCalledWith(expect.any(Error));
  });

  test("a model update check records its report", async () => {
    const report = {
      generation: 4,
      release_id: "2026-08-01-001",
      models: [
        {
          variant: "htdemucs" as const,
          state: "update_available" as const,
          installed_version: "model-v2.1.0",
          available_version: "model-v2.2.0",
          available_bytes: 355_000_000,
        },
      ],
    };
    const harness = createSettingsHarness({
      overrides: { settings: { checkModelUpdates: async () => report } },
    });

    await harness.controller.maintenance.checkModelUpdates();

    expect(harness.view().models.update).toEqual({
      status: "checked",
      error: null,
      generation: 4,
      models: report.models,
    });
  });

  test("a failed model update check is recorded without a notification", async () => {
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          checkModelUpdates: async () => {
            throw new Error("offline");
          },
        },
      },
    });

    await harness.controller.maintenance.checkModelUpdates();

    expect(harness.view().models.update).toEqual({
      status: "failed",
      error: "offline",
      generation: null,
      models: [],
    });
    expect(harness.notifyError).not.toHaveBeenCalled();
  });
});

describe("runtime", () => {
  test.each(["installing", "probing", "activating"] as const)(
    "runtime work is refused while the runtime is %s",
    async (state) => {
      const checkRuntimeUpdates = vi.fn();
      const downloadRuntime = vi.fn();
      const harness = createSettingsHarness({
        runtimeStatus: runtimeStatus({ state }),
        overrides: { settings: { checkRuntimeUpdates, downloadRuntime } },
      });

      await harness.controller.maintenance.checkRuntimeUpdates();
      await harness.controller.maintenance.installRuntime();

      expect(checkRuntimeUpdates).not.toHaveBeenCalled();
      expect(downloadRuntime).not.toHaveBeenCalled();
    },
  );

  test("only one runtime action runs at a time", async () => {
    const downloadRuntime = vi.fn(() => new Promise<never>(() => {}));
    const harness = createSettingsHarness({
      overrides: { settings: { downloadRuntime } },
    });

    void harness.controller.maintenance.installRuntime();
    void harness.controller.maintenance.installRuntime();
    await Promise.resolve();
    await Promise.resolve();

    expect(downloadRuntime).toHaveBeenCalledTimes(1);
  });

  test("an update check caches the report and re-reads runtime status", async () => {
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
    const getRuntimeBootstrapStatus = vi.fn(async () =>
      runtimeStatus({ state: "update_available" }),
    );
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          checkRuntimeUpdates: async () => report,
          getRuntimeBootstrapStatus,
        },
      },
    });

    await harness.controller.maintenance.checkRuntimeUpdates();

    expect(harness.view().runtime.update).toEqual({
      status: "checked",
      error: null,
      report,
    });
    expect(getRuntimeBootstrapStatus).toHaveBeenCalled();
    expect(harness.view().runtime.status?.state).toBe("update_available");
  });

  test("a failed update check is recorded without a notification", async () => {
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          checkRuntimeUpdates: async () => {
            throw new Error("offline");
          },
        },
      },
    });

    await harness.controller.maintenance.checkRuntimeUpdates();

    expect(harness.view().runtime.update?.status).toBe("failed");
    expect(harness.view().runtime.update?.error).toBe("offline");
    expect(harness.notifyError).not.toHaveBeenCalled();
  });

  test("installing mirrors the returned status and drops a stale report", async () => {
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          checkRuntimeUpdates: async () => ({
            generation: 9,
            release_id: "2026-08-01-001",
            target_triple: "aarch64-apple-darwin",
            state: "not_installed" as const,
            installed_version: null,
            available_version: "v1.28.0",
            available_bytes: 42_000_000,
            restart_required: false,
          }),
          getRuntimeBootstrapStatus: async () =>
            runtimeStatus({ state: "missing" }),
          downloadRuntime: async () =>
            runtimeStatus({
              state: "candidate_ready_restart_required",
              candidate_version: "v1.28.0",
              restart_required: true,
            }),
        },
      },
    });

    await harness.controller.maintenance.checkRuntimeUpdates();
    expect(harness.view().runtime.update?.report?.state).toBe("not_installed");

    await harness.controller.maintenance.installRuntime();

    expect(harness.view().runtime.status?.state).toBe(
      "candidate_ready_restart_required",
    );
    expect(harness.view().runtime.status?.candidate_version).toBe("v1.28.0");
    expect(harness.view().runtime.update).toBeNull();
  });

  test("a failed install surfaces its phase and message", async () => {
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          downloadRuntime: async () => ({
            ...runtimeStatus({ state: "failed", active_artifact_id: null }),
            error: {
              code: "model_unavailable" as const,
              message: "LoadLibraryExW failed",
              retryable: true,
              fallback: "retry",
            },
            failure_phase: "probe" as const,
          }),
        },
      },
    });

    await harness.controller.maintenance.installRuntime();

    expect(harness.view().runtime.status?.state).toBe("failed");
    expect(harness.view().runtime.status?.failure_phase).toBe("probe");
    expect(harness.view().runtime.status?.error).toBe("LoadLibraryExW failed");
  });

  test("deleting the runtime releases the lock and closes the dialog", async () => {
    const deleteRuntime = vi.fn(async () => {
      throw new Error("permission denied");
    });
    const downloadRuntime = vi.fn(async () => runtimeStatus());
    const harness = createSettingsHarness({
      overrides: { settings: { deleteRuntime, downloadRuntime } },
    });

    await harness.controller.maintenance.openDialog("delete_runtime");
    await harness.controller.maintenance.confirmDialog();

    expect(deleteRuntime).toHaveBeenCalledOnce();
    expect(harness.notifyError).toHaveBeenCalledWith(expect.any(Error));
    expect(harness.view().dialog).toBeNull();

    await harness.controller.maintenance.installRuntime();
    expect(downloadRuntime).toHaveBeenCalledOnce();
  });

  test("restarting the app is reported when it fails", async () => {
    const harness = createSettingsHarness({
      overrides: {
        settings: {
          restartApp: async () => {
            throw new Error("restart fail");
          },
        },
      },
    });

    await harness.controller.maintenance.restartApp();

    expect(harness.notifyError).toHaveBeenCalledWith(expect.any(Error));
  });
});

describe("maintenance dialogs", () => {
  test("opening the delete-stems dialog reads the size estimate", async () => {
    const harness = createSettingsHarness({
      overrides: { maintenance: { estimateStemsSize: async () => 512 } },
    });

    await harness.controller.maintenance.openDialog("delete_stems");

    expect(harness.view().dialog).toBe("delete_stems");
    expect(harness.view().maintenance.stemsSize).toBe(512);
  });

  test("a failed estimate still opens the dialog", async () => {
    const harness = createSettingsHarness({
      overrides: {
        maintenance: {
          estimateStemsSize: async () => {
            throw new Error("disk error");
          },
          estimateDowngradeSavings: async () => {
            throw new Error("disk error");
          },
        },
      },
    });

    await harness.controller.maintenance.openDialog("delete_stems");
    expect(harness.view().maintenance.stemsSize).toBeNull();
    expect(harness.view().dialog).toBe("delete_stems");

    await harness.controller.maintenance.openDialog("downgrade_stems");
    expect(harness.view().maintenance.downgradeSavings).toBeNull();
    expect(harness.view().dialog).toBe("downgrade_stems");
  });

  test("confirming delete stems clears in-memory separation status", async () => {
    const deleteAllStems = vi.fn(async () => ({
      deleted_count: 2,
      freed_bytes: 10,
    }));
    const harness = createSettingsHarness({
      overrides: {
        maintenance: { deleteAllStems, estimateStemsSize: async () => 512 },
      },
    });

    await harness.controller.maintenance.openDialog("delete_stems");
    await harness.controller.maintenance.confirmDialog();

    expect(deleteAllStems).toHaveBeenCalledOnce();
    expect(
      harness.stores.library.clearAllSeparationStatuses,
    ).toHaveBeenCalled();
    expect(harness.view().dialog).toBeNull();
    expect(harness.view().maintenance.deletingStems).toBe(false);
  });

  test("a failed delete stems is reported and still settles the dialog", async () => {
    const harness = createSettingsHarness({
      overrides: {
        maintenance: {
          estimateStemsSize: async () => 1,
          deleteAllStems: async () => {
            throw new Error("delete failed");
          },
        },
      },
    });

    await harness.controller.maintenance.openDialog("delete_stems");
    await harness.controller.maintenance.confirmDialog();

    expect(harness.notifyError).toHaveBeenCalledWith(expect.any(Error));
    expect(harness.view().dialog).toBeNull();
    expect(harness.view().maintenance.deletingStems).toBe(false);
  });

  test("confirming a downgrade repopulates separation status", async () => {
    const statuses: SeparationStatusSnapshot[] = [
      {
        song_id: "song-1",
        state: "completed",
        percent: 100,
        cache_hit: false,
        vocals_path: "vocals.ogg",
        accomp_path: "accomp.ogg",
        drums_path: null,
        bass_path: null,
        other_path: null,
        model_variant: "htdemucs",
        error: null,
      },
    ];
    const downgradeAllToTwoStem = vi.fn(async () => ({
      downgraded_count: 1,
      freed_bytes: 4_096,
    }));
    const harness = createSettingsHarness({
      overrides: {
        maintenance: {
          downgradeAllToTwoStem,
          estimateDowngradeSavings: async () => 4_096,
        },
        separation: { getAllSeparationStatuses: async () => statuses },
      },
    });

    await harness.controller.maintenance.openDialog("downgrade_stems");
    expect(harness.view().maintenance.downgradeSavings).toBe(4_096);

    await harness.controller.maintenance.confirmDialog();

    expect(downgradeAllToTwoStem).toHaveBeenCalledOnce();
    expect(
      harness.stores.library.clearAllSeparationStatuses,
    ).toHaveBeenCalled();
    expect(harness.stores.library.updateSeparationStatus).toHaveBeenCalledWith(
      statuses[0],
    );
    expect(harness.view().dialog).toBeNull();
  });

  test("confirming delete lyrics clears the lyrics store", async () => {
    const deleteAllCachedLyrics = vi.fn(async () => 3);
    const harness = createSettingsHarness({
      overrides: { maintenance: { deleteAllCachedLyrics } },
    });

    await harness.controller.maintenance.openDialog("delete_lyrics");
    await harness.controller.maintenance.confirmDialog();

    expect(deleteAllCachedLyrics).toHaveBeenCalledOnce();
    expect(harness.stores.lyrics.clear).toHaveBeenCalled();
    expect(harness.view().dialog).toBeNull();
    expect(harness.view().maintenance.deletingLyrics).toBe(false);
  });

  test("opening a second dialog replaces the first and closing clears it", async () => {
    const harness = createSettingsHarness({
      overrides: {
        maintenance: {
          estimateStemsSize: async () => 100,
          estimateDowngradeSavings: async () => 200,
        },
      },
    });

    await harness.controller.maintenance.openDialog("delete_stems");
    expect(harness.view().dialog).toBe("delete_stems");

    await harness.controller.maintenance.openDialog("downgrade_stems");
    expect(harness.view().dialog).toBe("downgrade_stems");

    harness.controller.maintenance.closeDialog();
    expect(harness.view().dialog).toBeNull();
  });

  test("confirming with no dialog open does nothing", async () => {
    const harness = createSettingsHarness();

    await harness.controller.maintenance.confirmDialog();

    expect(harness.view().dialog).toBeNull();
    expect(harness.notifyError).not.toHaveBeenCalled();
  });
});

describe("library integrity", () => {
  test("a check selects the primary-media issues by default", async () => {
    const harness = createSettingsHarness({
      overrides: {
        library: { checkLibraryIntegrity: async () => sampleReport },
      },
    });

    await harness.controller.library.checkIntegrity();

    const integrity = harness.view().integrity;
    expect(integrity.report).toEqual(sampleReport);
    expect(integrity.checking).toBe(false);
    expect([...integrity.selection].sort()).toEqual([
      "hash-empty",
      "hash-missing",
    ]);
  });

  test("a failed check clears the report and reports the error", async () => {
    const harness = createSettingsHarness({
      overrides: {
        library: {
          checkLibraryIntegrity: async () => {
            throw new Error("DB locked");
          },
        },
      },
    });

    await harness.controller.library.checkIntegrity();

    expect(harness.view().integrity.report).toBeNull();
    expect(harness.view().integrity.selection.size).toBe(0);
    expect(harness.view().integrity.checking).toBe(false);
    expect(harness.notifyError).toHaveBeenCalledOnce();
  });

  test("a new check clears the previous report and skipped count", async () => {
    const harness = createSettingsHarness({
      overrides: {
        library: {
          checkLibraryIntegrity: async () => sampleReport,
          removeMissingLibraryEntries: async () => ({
            deleted_song_hashes: ["hash-missing"],
            skipped_song_hashes: ["hash-empty"],
          }),
        },
      },
    });

    await harness.controller.library.checkIntegrity();
    await harness.controller.library.cleanUpIntegrity();
    expect(harness.view().integrity.skippedCount).toBe(1);

    harness.backend.library.checkLibraryIntegrity = async () => emptyReport;
    await harness.controller.library.checkIntegrity();

    expect(harness.view().integrity.report).toEqual(emptyReport);
    expect(harness.view().integrity.selection.size).toBe(0);
    expect(harness.view().integrity.skippedCount).toBeNull();
  });

  test("toggling an entry adds and removes it from the selection", async () => {
    const harness = createSettingsHarness({
      overrides: {
        library: { checkLibraryIntegrity: async () => emptyReport },
      },
    });
    await harness.controller.library.checkIntegrity();

    harness.controller.library.toggleIntegrityEntry("hash-a");
    expect(harness.view().integrity.selection.has("hash-a")).toBe(true);

    harness.controller.library.toggleIntegrityEntry("hash-a");
    expect(harness.view().integrity.selection.has("hash-a")).toBe(false);
  });

  test("cleanup removes the selected entries and rebuilds the song list", async () => {
    const removeMissingLibraryEntries = vi.fn(async () => ({
      deleted_song_hashes: ["hash-missing", "hash-empty"],
      skipped_song_hashes: [],
    }));
    const harness = createSettingsHarness({
      overrides: {
        library: {
          checkLibraryIntegrity: async () => sampleReport,
          removeMissingLibraryEntries,
        },
      },
    });
    await harness.controller.library.checkIntegrity();
    harness.controller.library.toggleIntegrityEntry("hash-optional");

    await harness.controller.library.cleanUpIntegrity();

    expect(removeMissingLibraryEntries).toHaveBeenCalledWith([
      "hash-missing",
      "hash-empty",
      "hash-optional",
    ]);
    expect(harness.stores.queue.removeSongIds).toHaveBeenCalledWith([
      "hash-missing",
      "hash-empty",
    ]);
    expect(harness.stores.library.loadLibrary).toHaveBeenCalled();
    expect(harness.stores.player.loadState).toHaveBeenCalled();

    const integrity = harness.view().integrity;
    expect(integrity.report?.missing_primary_media).toHaveLength(0);
    expect(integrity.report?.empty_primary_media).toHaveLength(0);
    expect(integrity.report?.missing_optional_assets).toHaveLength(1);
    expect([...integrity.selection]).toEqual(["hash-optional"]);
    expect(integrity.cleaningUp).toBe(false);
    expect(harness.view().dialog).toBeNull();
  });

  test("cleanup clears the song selection only when it overlaps", async () => {
    const createCleanupHarness = (selectedSongIds: string[]) => {
      const harness = createSettingsHarness({
        overrides: {
          library: {
            checkLibraryIntegrity: async () => sampleReport,
            removeMissingLibraryEntries: async () => ({
              deleted_song_hashes: ["hash-missing"],
              skipped_song_hashes: [],
            }),
          },
        },
      });
      harness.stores.library.getSelectedSongIds.mockReturnValue(
        new Set(selectedSongIds),
      );
      return harness;
    };

    const untouched = createCleanupHarness(["other-hash"]);
    await untouched.controller.library.checkIntegrity();
    await untouched.controller.library.cleanUpIntegrity();
    expect(untouched.stores.library.clearSelection).not.toHaveBeenCalled();

    const overlapping = createCleanupHarness(["hash-missing"]);
    await overlapping.controller.library.checkIntegrity();
    await overlapping.controller.library.cleanUpIntegrity();
    expect(overlapping.stores.library.clearSelection).toHaveBeenCalled();
  });

  test("cleanup records how many entries the backend skipped", async () => {
    const harness = createSettingsHarness({
      overrides: {
        library: {
          checkLibraryIntegrity: async () => sampleReport,
          removeMissingLibraryEntries: async () => ({
            deleted_song_hashes: ["hash-missing"],
            skipped_song_hashes: ["a", "b"],
          }),
        },
      },
    });
    await harness.controller.library.checkIntegrity();

    await harness.controller.library.cleanUpIntegrity();

    expect(harness.view().integrity.skippedCount).toBe(2);
  });

  test("cleanup with an empty selection just closes the dialog", async () => {
    const removeMissingLibraryEntries = vi.fn();
    const harness = createSettingsHarness({
      overrides: { library: { removeMissingLibraryEntries } },
    });

    await harness.controller.maintenance.openDialog(
      "integrity_cleanup_confirm",
    );
    await harness.controller.maintenance.confirmDialog();

    expect(removeMissingLibraryEntries).not.toHaveBeenCalled();
    expect(harness.view().dialog).toBeNull();
  });

  test("a failed cleanup reloads the song list, reports, and closes", async () => {
    const harness = createSettingsHarness({
      overrides: {
        library: {
          checkLibraryIntegrity: async () => sampleReport,
          removeMissingLibraryEntries: async () => {
            throw new Error("DB error");
          },
        },
      },
    });
    await harness.controller.library.checkIntegrity();

    await harness.controller.library.cleanUpIntegrity();

    expect(harness.notifyError).toHaveBeenCalledOnce();
    expect(harness.stores.library.loadLibrary).toHaveBeenCalled();
    expect(harness.view().dialog).toBeNull();
    expect(harness.view().integrity.cleaningUp).toBe(false);
  });

  test("dismissing the report clears it with its selection", async () => {
    const harness = createSettingsHarness({
      overrides: {
        library: { checkLibraryIntegrity: async () => sampleReport },
      },
    });
    await harness.controller.library.checkIntegrity();

    harness.controller.library.dismissIntegrityReport();

    expect(harness.view().integrity.report).toBeNull();
    expect(harness.view().integrity.selection.size).toBe(0);
    expect(harness.view().integrity.skippedCount).toBeNull();
  });
});

describe("subscription", () => {
  test("subscribers see a new view object for every change", async () => {
    const harness = createSettingsHarness({
      overrides: { maintenance: { estimateStemsSize: async () => 1 } },
    });
    const seen: unknown[] = [];
    const unsubscribe = harness.controller.subscribe(() => {
      seen.push(harness.controller.getView());
    });

    await harness.controller.maintenance.openDialog("delete_stems");

    expect(seen.length).toBeGreaterThan(0);
    expect(new Set(seen).size).toBe(seen.length);
    unsubscribe();
  });

  test("a preference changed elsewhere reaches subscribed consumers", () => {
    const harness = createSettingsHarness();
    const unsubscribe = harness.controller.subscribe(() => {});

    harness.preferencesStore.getState().patchAppSettings({ lyricsFontStep: 2 });

    expect(harness.view().preferences.lyricsFontStep).toBe(2);
    unsubscribe();
  });
});
