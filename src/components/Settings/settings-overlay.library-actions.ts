import { getErrorMessage } from "@/lib/errors";
import { useLibraryStore } from "@/stores/library-store";
import type { LibraryRegistrySnapshot, RegisteredLibrary } from "@/types/ipc";
import type {
  SettingsActionContext,
  SettingsOverlayActions,
} from "./settings-overlay.types";

async function applyLibrarySwitchSideEffects(
  context: SettingsActionContext,
  libraryId: string,
  registry: LibraryRegistrySnapshot,
  options: { refreshRemoteRepository?: boolean } = {},
) {
  const target = registry.libraries.find((library) => library.id === libraryId);
  if (target?.kind === "remote" && options.refreshRemoteRepository !== false) {
    await context.dependencies.api.refreshRemoteRepository();
  }

  context.dependencies.libraryStore.clearAllSeparationStatuses();
  context.dependencies.libraryStore.clearAllUploadStatuses();
  context.dependencies.libraryStore.clearSelection();
  context.dependencies.queueStore.clearQueue();
  context.dependencies.lyricsStore.clear();
  await context.dependencies.playerStore.loadState();
  await context.dependencies.libraryStore.loadLibrary();
  await context.refreshLibraryRegistry();
  await context.refreshModelStatuses();
}

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
    return "the selected storage provider";
  }

  return library.provider === "google_drive"
    ? "Google Drive"
    : library.provider === "dropbox"
      ? "Dropbox"
      : "WebDAV";
}

export function createLibrarySettingsActions(
  context: SettingsActionContext,
): Pick<
  SettingsOverlayActions,
  | "createLibrary"
  | "openLibrary"
  | "switchLibrary"
  | "refreshRemoteRepository"
  | "renameLibrary"
  | "removeLibrary"
  | "deleteLibrary"
  | "setLanguage"
  | "restartApp"
  | "setStemMode"
  | "setExecutionProvider"
  | "toggleHideBatchSeparate"
  | "toggleCoverArtBackdrop"
  | "toggleHideUpgradeAll"
  | "setEqEnabled"
  | "setEqGains"
  | "resetEqGains"
  | "setCrossfadeEnabled"
  | "setCrossfadeDurationMs"
  | "setThemePreference"
> {
  const {
    dependencies,
    controls,
    patchState,
    refreshLibraryRegistry,
    refreshModelStatuses,
    selectSingleDirectory,
  } = context;

  const refreshLibraryStateAfterMutation = async (
    registry?: LibraryRegistrySnapshot,
  ) => {
    const nextRegistry =
      registry ?? (await dependencies.api.getLibraryRegistry());
    if (nextRegistry.active_library_id) {
      await dependencies.libraryStore.loadLibrary();
    } else {
      useLibraryStore.setState({ songs: [], searchQuery: "" });
      dependencies.libraryStore.clearAllSeparationStatuses();
      dependencies.libraryStore.clearAllUploadStatuses();
      dependencies.libraryStore.clearSelection();
      dependencies.queueStore.clearQueue();
      dependencies.lyricsStore.clear();
      await dependencies.playerStore.loadState();
    }
    await refreshLibraryRegistry();
    await refreshModelStatuses();
    return nextRegistry;
  };

  return {
    createLibrary: async (dialogTitle) => {
      const selectedDirectory = await selectSingleDirectory(dialogTitle);
      if (!selectedDirectory) return;

      const libraryDir = `${selectedDirectory}/OpenKara`;
      patchState({ libraryError: null });

      try {
        await dependencies.api.createLocalLibrary(libraryDir);
        await refreshLibraryRegistry();
      } catch (error: unknown) {
        patchState({
          libraryError: getErrorMessage(error),
        });
      }
    },

    openLibrary: async (dialogTitle) => {
      const selectedDirectory = await selectSingleDirectory(dialogTitle);
      if (!selectedDirectory) return;

      patchState({ libraryError: null });

      try {
        await dependencies.api.registerLocalLibrary(selectedDirectory);
        await refreshLibraryRegistry();
      } catch (error: unknown) {
        patchState({
          libraryError: getErrorMessage(error),
        });
      }
    },

    switchLibrary: async (libraryId) => {
      patchState({ libraryError: null });

      try {
        const registry = await dependencies.api.switchLibrary(libraryId);
        await applyLibrarySwitchSideEffects(context, libraryId, registry);
      } catch (error: unknown) {
        patchState({
          libraryError: getErrorMessage(error),
        });
      }
    },

    refreshRemoteRepository: async (libraryId) => {
      patchState({ libraryError: null });

      const current = controls.getSnapshot().state;
      const target = current.libraries.find(
        (library) => library.id === libraryId,
      );
      if (target?.kind !== "remote") {
        return;
      }

      try {
        if (current.activeLibraryId !== libraryId) {
          const registry = await dependencies.api.switchLibrary(libraryId);
          await applyLibrarySwitchSideEffects(context, libraryId, registry);
          return;
        }

        await dependencies.api.refreshRemoteRepository();
        const registry = await dependencies.api.getLibraryRegistry();
        await applyLibrarySwitchSideEffects(context, libraryId, registry, {
          refreshRemoteRepository: false,
        });
      } catch (error: unknown) {
        patchState({
          libraryError: getErrorMessage(error),
        });
      }
    },

    renameLibrary: async (libraryId, displayName) => {
      patchState({ libraryError: null });

      const currentLibrary = controls
        .getSnapshot()
        .state.libraries.find((library) => library.id === libraryId);
      const currentName = currentLibrary?.display_name ?? "";
      const trimmedName = displayName.trim();
      if (!trimmedName || trimmedName === currentName) {
        return;
      }

      try {
        const registry = await dependencies.api.renameLibrary(
          libraryId,
          trimmedName,
        );
        await refreshLibraryStateAfterMutation(registry);
      } catch (error: unknown) {
        patchState({
          libraryError: getErrorMessage(error),
        });
      }
    },

    removeLibrary: async (libraryId) => {
      patchState({ libraryError: null });

      const currentLibrary = controls
        .getSnapshot()
        .state.libraries.find((library) => library.id === libraryId);

      if (
        !window.confirm(
          currentLibrary?.kind === "remote"
            ? `Disconnect "${currentLibrary.display_name}" from OpenKara? The remote repository contents will stay in ${remoteProviderDisplayName(currentLibrary)}.`
            : "Disconnect this library from OpenKara? The library data will stay on disk.",
        )
      ) {
        return;
      }

      try {
        const registry = await dependencies.api.removeLibrary(libraryId);
        await refreshLibraryStateAfterMutation(registry);
      } catch (error: unknown) {
        patchState({
          libraryError: getErrorMessage(error),
        });
      }
    },

    deleteLibrary: async (libraryId, confirmationName) => {
      patchState({ libraryError: null });

      const currentLibrary = controls
        .getSnapshot()
        .state.libraries.find((library) => library.id === libraryId);
      const isRemoteRepository = currentLibrary?.kind === "remote";
      const displayName = currentLibrary?.display_name ?? "this library";

      if (
        !window.confirm(
          isRemoteRepository
            ? `Delete "${displayName}"? This will delete the remote repository contents from ${remoteProviderDisplayName(currentLibrary)} at ${describeLibrary(currentLibrary)} and remove the local working copy.`
            : `Delete "${displayName}" from OpenKara? This removes the local library data from disk.`,
        )
      ) {
        return;
      }

      if (confirmationName !== displayName) {
        return;
      }

      try {
        const registry = await dependencies.api.deleteLibrary(libraryId);
        await refreshLibraryStateAfterMutation(registry);
      } catch (error: unknown) {
        patchState({
          libraryError: getErrorMessage(error),
        });
      }
    },

    setLanguage: async (language) => {
      patchState({ language });
      dependencies.settingsStore.patchAppSettings({ language });
      await Promise.resolve(dependencies.changeLanguage(language));

      try {
        const settings = await dependencies.api.setLanguage(language);
        dependencies.settingsStore.hydrateAppSettings(settings);
      } catch (error) {
        dependencies.notifyError(error);
      }
    },

    restartApp: async () => {
      try {
        await dependencies.api.restartApp();
      } catch (error) {
        dependencies.notifyError(error);
      }
    },

    setStemMode: async (mode) => {
      try {
        const settings = await dependencies.api.setStemMode(mode);
        dependencies.settingsStore.hydrateAppSettings(settings);
        patchState({ stemMode: settings.stem_mode });
      } catch (error) {
        dependencies.notifyError(error);
      }
    },

    setExecutionProvider: async (provider) => {
      try {
        const settings = await dependencies.api.setExecutionProvider(provider);
        dependencies.settingsStore.hydrateAppSettings(settings);
        patchState({
          executionProvider: settings.execution_provider,
          availableExecutionProviders: settings.available_execution_providers,
          compatibleExecutionProviders: settings.compatible_execution_providers,
        });
      } catch (error) {
        dependencies.notifyError(error);
      }
    },

    toggleHideBatchSeparate: async (value) => {
      patchState({ hideBatchSeparate: value });
      dependencies.settingsStore.patchAppSettings({ hideBatchSeparate: value });

      try {
        const settings = await dependencies.api.setHideBatchSeparate(value);
        dependencies.settingsStore.hydrateAppSettings(settings);
      } catch (error) {
        dependencies.notifyError(error);
      }
    },

    toggleCoverArtBackdrop: async (value) => {
      patchState({ coverArtBackdrop: value });
      dependencies.settingsStore.patchAppSettings({ coverArtBackdrop: value });

      try {
        const settings = await dependencies.api.setCoverArtBackdrop(value);
        dependencies.settingsStore.hydrateAppSettings(settings);
      } catch (error) {
        dependencies.notifyError(error);
      }
    },

    toggleHideUpgradeAll: async (value) => {
      patchState({ hideUpgradeAll: value });
      dependencies.settingsStore.patchAppSettings({ hideUpgradeAll: value });

      try {
        const settings = await dependencies.api.setHideUpgradeAll(value);
        dependencies.settingsStore.hydrateAppSettings(settings);
      } catch (error) {
        dependencies.notifyError(error);
      }
    },

    setEqEnabled: async (enabled) => {
      patchState({ eqEnabled: enabled });
      await dependencies.settingsStore.setEqEnabled(enabled);
      patchState({
        eqEnabled:
          dependencies.settingsStore.getAppSettingsSnapshot().eqEnabled,
      });
    },

    setEqGains: async (gainsDb) => {
      const clamped = gainsDb.map((g) => Math.max(-12, Math.min(12, g))) as [
        number,
        number,
        number,
        number,
        number,
      ];
      const current = controls.getSnapshot().state.eqGainsDb;
      if (current.every((g, i) => g === clamped[i])) {
        return;
      }
      patchState({ eqGainsDb: clamped });
      await dependencies.settingsStore.setEqGains(clamped);
      patchState({
        eqGainsDb:
          dependencies.settingsStore.getAppSettingsSnapshot().eqGainsDb,
      });
    },

    resetEqGains: async () => {
      const flat = [0, 0, 0, 0, 0] as [number, number, number, number, number];
      patchState({ eqGainsDb: flat });
      await dependencies.settingsStore.setEqGains(flat);
      patchState({
        eqGainsDb:
          dependencies.settingsStore.getAppSettingsSnapshot().eqGainsDb,
      });
    },

    setCrossfadeEnabled: async (enabled) => {
      patchState({ crossfadeEnabled: enabled });
      await dependencies.settingsStore.setCrossfadeEnabled(enabled);
      patchState({
        crossfadeEnabled:
          dependencies.settingsStore.getAppSettingsSnapshot().crossfadeEnabled,
      });
    },

    setCrossfadeDurationMs: async (durationMs) => {
      const clamped = Math.max(500, Math.min(10_000, Math.round(durationMs)));
      const current = controls.getSnapshot().state.crossfadeDurationMs;
      if (current === clamped) {
        return;
      }
      patchState({ crossfadeDurationMs: clamped });
      await dependencies.settingsStore.setCrossfadeDurationMs(clamped);
      patchState({
        crossfadeDurationMs:
          dependencies.settingsStore.getAppSettingsSnapshot()
            .crossfadeDurationMs,
      });
    },

    setThemePreference: async (preference) => {
      patchState({ themePreference: preference });
      await dependencies.settingsStore.setThemePreference(preference);
      patchState({
        themePreference:
          dependencies.settingsStore.getAppSettingsSnapshot().themePreference,
      });
    },
  };
}
