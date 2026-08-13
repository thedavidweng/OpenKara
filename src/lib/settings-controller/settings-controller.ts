import { getErrorMessage, notifyError as reportError } from "@/lib/errors";
import i18next, { resolveAppLanguage } from "@/lib/i18n";
import type { AppSettingsSnapshot } from "@/stores/settings-store";
import type {
  AppSettings,
  IntegrityReport,
  LibraryRegistrySnapshot,
  ModelStatusSnapshot,
  ModelVariant,
  RegisteredLibrary,
  RuntimeBootstrapStatusSnapshot,
} from "@/types/ipc";
import type {
  LibraryCommandResult,
  ModelStatusView,
  SettingsController,
  SettingsControllerDependencies,
  SettingsDialog,
  SettingsLibraryView,
  SettingsMaintenanceView,
  SettingsModelsView,
  SettingsIntegrityView,
  SettingsPreferencePatch,
  SettingsPreferencesView,
  SettingsRuntimeView,
  RuntimeStatusView,
  SettingsView,
} from "./types";
import { createZustandSettingsStores } from "./zustand-stores";

const EQ_GAIN_LIMIT_DB = 12;
const CROSSFADE_MIN_MS = 500;
const CROSSFADE_MAX_MS = 10_000;
const RUNTIME_BUSY_STATES = ["installing", "probing", "activating"];

export function describeLibrary(library: RegisteredLibrary): string {
  if (library.kind === "local") {
    return library.root_path;
  }

  return `${library.display_name} · ${
    library.remote_path_display || library.remote_root_locator
  }`;
}

function remoteProviderDisplayName(library: RegisteredLibrary): string {
  if (library.kind !== "remote") {
    return i18next.t("settings.library.thisLibrary");
  }

  return library.provider === "google_drive"
    ? i18next.t("setup.remoteProvider.googleDrive.title")
    : library.provider === "dropbox"
      ? i18next.t("setup.remoteProvider.dropbox.title")
      : i18next.t("setup.remoteProvider.webdav.title");
}

function toPreferencesView(
  snapshot: AppSettingsSnapshot,
): SettingsPreferencesView {
  return { ...snapshot, language: resolveAppLanguage(snapshot.language) };
}

function toRuntimeStatusView(
  status: RuntimeBootstrapStatusSnapshot | null,
): RuntimeStatusView | null {
  if (!status) {
    return null;
  }

  return {
    state: status.state,
    version: status.version,
    runtime_path: status.runtime_path,
    active_artifact_id: status.active_artifact_id,
    target_triple: status.target_triple,
    candidate_version: status.candidate_version,
    restart_required: status.restart_required,
    error: status.error?.message ?? null,
    failure_phase: status.failure_phase ?? null,
  };
}

function toModelStatusView(status: ModelStatusSnapshot): ModelStatusView {
  return {
    downloaded: status.downloaded,
    legacy_install_present: status.legacy_install_present,
    file_size_bytes: status.file_size_bytes,
    installed_version: status.installed_version,
    pinned_version: status.pinned_version,
  };
}

function shallowEqual(left: object | null, right: object | null): boolean {
  if (left === right) return true;
  if (!left || !right) return false;

  const leftKeys = Object.keys(left);
  if (leftKeys.length !== Object.keys(right).length) return false;

  return leftKeys.every(
    (key) => Reflect.get(left, key) === Reflect.get(right, key),
  );
}

function defaultIntegritySelection(report: IntegrityReport): Set<string> {
  const hashes = new Set<string>();
  for (const issue of report.missing_primary_media) {
    hashes.add(issue.song_hash);
  }
  for (const issue of report.empty_primary_media) {
    hashes.add(issue.song_hash);
  }
  return hashes;
}

function clampEqGains(
  gainsDb: [number, number, number, number, number],
): [number, number, number, number, number] {
  const clamp = (gain: number) =>
    Math.max(-EQ_GAIN_LIMIT_DB, Math.min(EQ_GAIN_LIMIT_DB, gain));
  return [
    clamp(gainsDb[0]),
    clamp(gainsDb[1]),
    clamp(gainsDb[2]),
    clamp(gainsDb[3]),
    clamp(gainsDb[4]),
  ];
}

export function createSettingsController({
  backend,
  createLibrarySession,
  selectDirectory,
  stores = createZustandSettingsStores(),
  notifyError = reportError,
  changeLanguage = (language: string) => i18next.changeLanguage(language),
}: SettingsControllerDependencies): SettingsController {
  const listeners = new Set<() => void>();

  let view: SettingsView = {
    isInitializing: true,
    dialog: null,
    library: {
      registry: null,
      libraries: [],
      activeLibraryId: null,
      activeLibraryPath: null,
      error: null,
    },
    preferences: toPreferencesView(stores.preferences.getSnapshot()),
    models: {
      statuses: {},
      statusesError: null,
      downloading: null,
      update: null,
    },
    runtime: {
      status: toRuntimeStatusView(stores.runtimeStatus.getStatus()),
      statusError: null,
      update: null,
    },
    integrity: {
      report: null,
      selection: new Set(),
      skippedCount: null,
      checking: false,
      cleaningUp: false,
    },
    maintenance: {
      stemsSize: null,
      downgradeSavings: null,
      deletingStems: false,
      deletingLyrics: false,
      downgrading: false,
    },
  };

  const commit = (next: SettingsView) => {
    view = next;
    for (const listener of [...listeners]) {
      listener();
    }
  };

  const patchLibrary = (patch: Partial<SettingsLibraryView>) =>
    commit({ ...view, library: { ...view.library, ...patch } });
  const patchModels = (patch: Partial<SettingsModelsView>) =>
    commit({ ...view, models: { ...view.models, ...patch } });
  const patchRuntime = (patch: Partial<SettingsRuntimeView>) =>
    commit({ ...view, runtime: { ...view.runtime, ...patch } });
  const patchIntegrity = (patch: Partial<SettingsIntegrityView>) =>
    commit({ ...view, integrity: { ...view.integrity, ...patch } });
  const patchMaintenance = (patch: Partial<SettingsMaintenanceView>) =>
    commit({ ...view, maintenance: { ...view.maintenance, ...patch } });
  const setDialog = (dialog: SettingsDialog | null) =>
    commit({ ...view, dialog });

  const syncStores = () => {
    const preferences = toPreferencesView(stores.preferences.getSnapshot());
    const status = toRuntimeStatusView(stores.runtimeStatus.getStatus());

    if (
      shallowEqual(preferences, view.preferences) &&
      shallowEqual(status, view.runtime.status)
    ) {
      return;
    }

    commit({
      ...view,
      preferences,
      runtime: { ...view.runtime, status },
    });
  };

  let detachStores: (() => void) | null = null;

  const subscribe = (listener: () => void) => {
    listeners.add(listener);

    if (!detachStores) {
      const detachPreferences = stores.preferences.subscribe(syncStores);
      const detachRuntime = stores.runtimeStatus.subscribe(syncStores);
      detachStores = () => {
        detachPreferences();
        detachRuntime();
      };
    }

    return () => {
      listeners.delete(listener);
      if (listeners.size === 0) {
        detachStores?.();
        detachStores = null;
      }
    };
  };

  const applyRegistry = (registry: LibraryRegistrySnapshot) => {
    const active = registry.libraries.find(
      (library) => library.id === registry.active_library_id,
    );

    patchLibrary({
      registry,
      libraries: registry.libraries,
      activeLibraryId: registry.active_library_id,
      activeLibraryPath: active ? describeLibrary(active) : null,
      error: null,
    });
  };

  const refreshRegistry = async () => {
    try {
      applyRegistry(await backend.librarySetup.getLibraryRegistry());
    } catch (error) {
      notifyError(error);
    }
  };

  const refreshModelStatuses = async () => {
    try {
      const [standard, fineTuned] = await Promise.all([
        backend.settings.getModelStatus("htdemucs"),
        backend.settings.getModelStatus("htdemucs_ft"),
      ]);

      patchModels({
        statuses: {
          htdemucs: toModelStatusView(standard),
          htdemucs_ft: toModelStatusView(fineTuned),
        },
        statusesError: null,
      });
    } catch (error) {
      patchModels({ statusesError: getErrorMessage(error) });
    }
  };

  const refreshRuntimeStatus = async () => {
    try {
      stores.runtimeStatus.updateStatus(
        await backend.settings.getRuntimeBootstrapStatus(),
      );
      patchRuntime({ statusError: null });
      syncStores();
    } catch (error) {
      patchRuntime({ statusError: getErrorMessage(error) });
    }
  };

  const librarySession = createLibrarySession({
    refreshRegistry,
    refreshModelStatuses,
  });

  const runLibraryWork = async (
    work: () => Promise<void>,
  ): Promise<LibraryCommandResult> => {
    patchLibrary({ error: null });

    try {
      await work();
      return { ok: true };
    } catch (error: unknown) {
      const message = getErrorMessage(error);
      patchLibrary({ error: message });
      return { ok: false, error: message };
    }
  };

  const findLibrary = (libraryId: string) =>
    view.library.libraries.find((library) => library.id === libraryId);

  const applyModelVariant = async (variant: ModelVariant) => {
    try {
      if (view.models.downloading === variant) {
        return;
      }

      if (!view.models.statuses[variant]?.downloaded) {
        patchModels({ downloading: variant });
        await backend.settings.downloadModel(variant);
        await refreshModelStatuses();
        stores.modelBootstrap.reload();
        patchModels({ downloading: null });
      }

      stores.preferences.hydrate(
        await backend.settings.setModelVariant(variant),
      );
      syncStores();
    } catch (error) {
      patchModels({ downloading: null });
      notifyError(error);
    }
  };

  const writeBackendPreference = async (write: () => Promise<AppSettings>) => {
    try {
      stores.preferences.hydrate(await write());
    } catch (error) {
      notifyError(error);
    } finally {
      syncStores();
    }
  };

  const writeOptimisticPreference = async (
    patch: Partial<AppSettingsSnapshot>,
    write: () => Promise<AppSettings>,
  ) => {
    stores.preferences.patch(patch);
    syncStores();
    await writeBackendPreference(write);
  };

  const writeStorePreference = async (write: () => Promise<void>) => {
    await write();
    syncStores();
  };

  const setPreferences = async (patch: SettingsPreferencePatch) => {
    const {
      language,
      stemMode,
      executionProvider,
      hideBatchSeparate,
      coverArtBackdrop,
      hideUpgradeAll,
      eqEnabled,
      eqGainsDb,
      crossfadeEnabled,
      crossfadeDurationMs,
      themePreference,
      updatePolicy,
    } = patch;

    if (language !== undefined) {
      stores.preferences.patch({ language });
      syncStores();
      await Promise.resolve(changeLanguage(language));
      await writeBackendPreference(() =>
        backend.settings.setLanguage(language),
      );
    }

    if (stemMode !== undefined) {
      await writeBackendPreference(() =>
        backend.settings.setStemMode(stemMode),
      );
    }

    if (executionProvider !== undefined) {
      await writeBackendPreference(() =>
        backend.settings.setExecutionProvider(executionProvider),
      );
    }

    if (hideBatchSeparate !== undefined) {
      await writeOptimisticPreference({ hideBatchSeparate }, () =>
        backend.settings.setHideBatchSeparate(hideBatchSeparate),
      );
    }

    if (coverArtBackdrop !== undefined) {
      await writeOptimisticPreference({ coverArtBackdrop }, () =>
        backend.settings.setCoverArtBackdrop(coverArtBackdrop),
      );
    }

    if (hideUpgradeAll !== undefined) {
      await writeOptimisticPreference({ hideUpgradeAll }, () =>
        backend.settings.setHideUpgradeAll(hideUpgradeAll),
      );
    }

    if (eqEnabled !== undefined) {
      await writeStorePreference(() =>
        stores.preferences.setEqEnabled(eqEnabled),
      );
    }

    if (eqGainsDb !== undefined) {
      const clamped = clampEqGains(eqGainsDb);
      if (
        clamped.some((gain, band) => gain !== view.preferences.eqGainsDb[band])
      ) {
        await writeStorePreference(() =>
          stores.preferences.setEqGains(clamped),
        );
      }
    }

    if (crossfadeEnabled !== undefined) {
      await writeStorePreference(() =>
        stores.preferences.setCrossfadeEnabled(crossfadeEnabled),
      );
    }

    if (crossfadeDurationMs !== undefined) {
      const clamped = Math.max(
        CROSSFADE_MIN_MS,
        Math.min(CROSSFADE_MAX_MS, Math.round(crossfadeDurationMs)),
      );
      if (clamped !== view.preferences.crossfadeDurationMs) {
        await writeStorePreference(() =>
          stores.preferences.setCrossfadeDurationMs(clamped),
        );
      }
    }

    if (themePreference !== undefined) {
      await writeStorePreference(() =>
        stores.preferences.setThemePreference(themePreference),
      );
    }

    if (updatePolicy !== undefined) {
      await writeStorePreference(() =>
        stores.preferences.setUpdatePolicy(updatePolicy),
      );
    }
  };

  let runtimeActionInFlight = false;

  const withRuntimeLock = async (action: () => Promise<void>) => {
    const runtimeState = view.runtime.status?.state;
    if (
      runtimeActionInFlight ||
      (runtimeState !== undefined && RUNTIME_BUSY_STATES.includes(runtimeState))
    ) {
      return;
    }

    runtimeActionInFlight = true;
    try {
      await action();
    } finally {
      runtimeActionInFlight = false;
    }
  };

  const installRuntime = () =>
    withRuntimeLock(async () => {
      try {
        stores.runtimeStatus.updateStatus(
          await backend.settings.downloadRuntime(),
        );
        syncStores();
        patchRuntime({ update: null });
        await refreshModelStatuses();
      } catch (error) {
        notifyError(error);
      }
    });

  const checkRuntimeUpdates = () =>
    withRuntimeLock(async () => {
      patchRuntime({
        update: { status: "checking", error: null, report: null },
      });

      try {
        const report = await backend.settings.checkRuntimeUpdates();
        patchRuntime({ update: { status: "checked", error: null, report } });
        await refreshRuntimeStatus();
      } catch (error) {
        patchRuntime({
          update: {
            status: "failed",
            error: getErrorMessage(error),
            report: null,
          },
        });
      }
    });

  const deleteRuntime = () =>
    withRuntimeLock(async () => {
      try {
        await backend.settings.deleteRuntime();
        await refreshRuntimeStatus();
        await refreshModelStatuses();
      } catch (error) {
        notifyError(error);
      }
    });

  const cleanUpIntegrity = async () => {
    const { report, selection } = view.integrity;
    const selectedHashes = [...selection];

    if (selectedHashes.length === 0) {
      setDialog(null);
      return;
    }

    patchIntegrity({ cleaningUp: true });
    patchLibrary({ error: null });

    try {
      const result =
        await backend.library.removeMissingLibraryEntries(selectedHashes);
      const deleted = new Set(result.deleted_song_hashes);

      if (result.deleted_song_hashes.length > 0) {
        stores.queue.removeSongIds(result.deleted_song_hashes);

        const affectsSelection = [...stores.library.getSelectedSongIds()].some(
          (songId) => deleted.has(songId),
        );
        if (affectsSelection) {
          stores.library.clearSelection();
        }
      }

      await stores.library.loadLibrary();
      await stores.player.loadState();

      patchIntegrity({
        selection: new Set(selectedHashes.filter((hash) => !deleted.has(hash))),
        skippedCount:
          result.skipped_song_hashes.length > 0
            ? result.skipped_song_hashes.length
            : null,
      });

      if (report) {
        const keep = (issue: { song_hash: string }) =>
          !deleted.has(issue.song_hash);

        patchIntegrity({
          report: {
            ...report,
            missing_primary_media: report.missing_primary_media.filter(keep),
            empty_primary_media: report.empty_primary_media.filter(keep),
            missing_optional_assets:
              report.missing_optional_assets.filter(keep),
            empty_optional_assets: report.empty_optional_assets.filter(keep),
          },
        });
      }
    } catch (error: unknown) {
      await stores.library.loadLibrary();
      notifyError(error);
    } finally {
      setDialog(null);
      patchIntegrity({ cleaningUp: false });
    }
  };

  const confirmDeleteStems = async () => {
    patchMaintenance({ deletingStems: true });

    try {
      await backend.maintenance.deleteAllStems();
      stores.library.clearAllSeparationStatuses();
    } catch (error) {
      notifyError(error);
    } finally {
      patchMaintenance({ deletingStems: false });
      setDialog(null);
    }
  };

  const confirmDowngrade = async () => {
    patchMaintenance({ downgrading: true });

    try {
      await backend.maintenance.downgradeAllToTwoStem();
      const statuses = await backend.separation.getAllSeparationStatuses();

      stores.library.clearAllSeparationStatuses();
      for (const status of statuses) {
        stores.library.updateSeparationStatus(status);
      }
    } catch (error) {
      notifyError(error);
    } finally {
      patchMaintenance({ downgrading: false });
      setDialog(null);
    }
  };

  const confirmDeleteLyrics = async () => {
    patchMaintenance({ deletingLyrics: true });

    try {
      await backend.maintenance.deleteAllCachedLyrics();
      stores.lyrics.clear();
    } catch (error) {
      notifyError(error);
    } finally {
      patchMaintenance({ deletingLyrics: false });
      setDialog(null);
    }
  };

  const estimate = async (read: () => Promise<number>) => {
    try {
      return await read();
    } catch {
      return null;
    }
  };

  return {
    getView: () => view,
    subscribe,

    initialize: async () => {
      commit({ ...view, isInitializing: true });

      const [registryResult, settingsResult] = await Promise.allSettled([
        backend.librarySetup.getLibraryRegistry(),
        backend.settings.getSettings(),
      ]);

      if (registryResult.status === "fulfilled") {
        applyRegistry(registryResult.value);
      } else {
        notifyError(registryResult.reason);
      }

      if (settingsResult.status === "fulfilled") {
        stores.preferences.hydrate(settingsResult.value);
        syncStores();
      } else {
        notifyError(settingsResult.reason);
      }

      commit({ ...view, isInitializing: false });

      await Promise.all([refreshRuntimeStatus(), refreshModelStatuses()]);
    },

    library: {
      create: (dialogTitle) =>
        runLibraryWork(async () => {
          const directory = await selectDirectory(dialogTitle);
          if (directory) {
            await librarySession.createLocalLibrary(directory);
          }
        }),

      open: (dialogTitle) =>
        runLibraryWork(async () => {
          const directory = await selectDirectory(dialogTitle);
          if (directory) {
            await librarySession.openLocalLibrary(directory);
          }
        }),

      activate: (libraryId) =>
        runLibraryWork(() => librarySession.switchLibrary(libraryId)),

      refresh: (libraryId) =>
        runLibraryWork(async () => {
          if (findLibrary(libraryId)?.kind !== "remote") {
            return;
          }

          if (view.library.activeLibraryId === libraryId) {
            await librarySession.refreshRepository();
          } else {
            await librarySession.switchLibrary(libraryId);
          }
        }),

      rename: (libraryId, displayName) =>
        runLibraryWork(async () => {
          const trimmed = displayName.trim();
          if (
            !trimmed ||
            trimmed === (findLibrary(libraryId)?.display_name ?? "")
          ) {
            return;
          }

          await librarySession.adoptRegistry(
            await backend.librarySetup.renameLibrary(libraryId, trimmed),
          );
        }),

      disconnect: (libraryId) =>
        runLibraryWork(async () => {
          const library = findLibrary(libraryId);
          const appName = i18next.t("app.name");
          const message =
            library?.kind === "remote"
              ? i18next.t("settings.library.confirmDisconnectRemote", {
                  displayName: library.display_name,
                  appName,
                  provider: remoteProviderDisplayName(library),
                })
              : i18next.t("settings.library.confirmDisconnectLocal", {
                  appName,
                });

          if (!window.confirm(message)) {
            return;
          }

          await librarySession.adoptRegistry(
            await backend.librarySetup.removeLibrary(libraryId),
          );
        }),

      delete: (libraryId, confirmationName) =>
        runLibraryWork(async () => {
          const library = findLibrary(libraryId);
          const displayName =
            library?.display_name ?? i18next.t("settings.library.thisLibrary");
          const message =
            library?.kind === "remote"
              ? i18next.t("settings.library.confirmDeleteRemote", {
                  displayName,
                  provider: remoteProviderDisplayName(library),
                  path: describeLibrary(library),
                })
              : i18next.t("settings.library.confirmDeleteLocal", {
                  displayName,
                  appName: i18next.t("app.name"),
                });

          if (confirmationName !== displayName || !window.confirm(message)) {
            return;
          }

          await librarySession.adoptRegistry(
            await backend.librarySetup.deleteLibrary(libraryId),
          );
        }),

      checkIntegrity: async () => {
        patchIntegrity({ checking: true, skippedCount: null });
        patchLibrary({ error: null });

        try {
          const report = await backend.library.checkLibraryIntegrity();
          patchIntegrity({
            report,
            selection: defaultIntegritySelection(report),
          });
        } catch (error: unknown) {
          patchIntegrity({ report: null, selection: new Set() });
          notifyError(error);
        } finally {
          patchIntegrity({ checking: false });
        }
      },

      toggleIntegrityEntry: (songHash) => {
        const selection = new Set(view.integrity.selection);
        if (selection.has(songHash)) {
          selection.delete(songHash);
        } else {
          selection.add(songHash);
        }
        patchIntegrity({ selection });
      },

      cleanUpIntegrity,

      dismissIntegrityReport: () => {
        patchIntegrity({
          report: null,
          selection: new Set(),
          skippedCount: null,
        });
      },
    },

    preferences: {
      set: setPreferences,

      selectModelVariant: async (variant) => {
        if (
          variant === "htdemucs_ft" &&
          view.preferences.modelVariant !== "htdemucs_ft"
        ) {
          setDialog("ft_warning");
          return;
        }

        await applyModelVariant(variant);
      },
    },

    maintenance: {
      openDialog: async (dialog) => {
        if (dialog === "delete_stems") {
          patchMaintenance({
            stemsSize: await estimate(() =>
              backend.maintenance.estimateStemsSize(),
            ),
          });
        } else if (dialog === "downgrade_stems") {
          patchMaintenance({
            downgradeSavings: await estimate(() =>
              backend.maintenance.estimateDowngradeSavings(),
            ),
          });
        }

        setDialog(dialog);
      },

      confirmDialog: async () => {
        switch (view.dialog) {
          case "delete_stems":
            await confirmDeleteStems();
            return;
          case "downgrade_stems":
            await confirmDowngrade();
            return;
          case "delete_lyrics":
            await confirmDeleteLyrics();
            return;
          case "delete_runtime":
            await deleteRuntime();
            setDialog(null);
            return;
          case "ft_warning":
            setDialog(null);
            await applyModelVariant("htdemucs_ft");
            return;
          case "integrity_cleanup_confirm":
            await cleanUpIntegrity();
            return;
          case null:
            return;
        }
      },

      closeDialog: () => setDialog(null),

      restartApp: async () => {
        try {
          await backend.settings.restartApp();
        } catch (error) {
          notifyError(error);
        }
      },

      checkModelUpdates: async () => {
        patchModels({
          update: {
            status: "checking",
            error: null,
            generation: null,
            models: [],
          },
        });

        try {
          const report = await backend.settings.checkModelUpdates();
          patchModels({
            update: {
              status: "checked",
              error: null,
              generation: report.generation,
              models: report.models,
            },
          });
        } catch (error) {
          patchModels({
            update: {
              status: "failed",
              error: getErrorMessage(error),
              generation: null,
              models: [],
            },
          });
        }
      },

      downloadModel: async (variant) => {
        if (view.models.downloading === variant) {
          return;
        }

        try {
          patchModels({ downloading: variant });
          await backend.settings.downloadModel(variant);
          await refreshModelStatuses();
          stores.modelBootstrap.reload();
          patchModels({ downloading: null });
        } catch (error) {
          patchModels({ downloading: null });
          notifyError(error);
        }
      },

      deleteModel: async (variant) => {
        try {
          await backend.settings.deleteModel(variant);
          await refreshModelStatuses();
          stores.modelBootstrap.reload();
        } catch (error) {
          notifyError(error);
        }
      },

      checkRuntimeUpdates,
      installRuntime,
    },
  };
}
