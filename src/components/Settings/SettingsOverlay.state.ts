import { useBootstrapStore } from "@/stores/bootstrap-store";
import { useRuntimeBootstrapStore } from "@/stores/runtime-bootstrap-store";
import { useSettingsStore } from "@/stores/settings-store";
import type { LibraryRegistrySnapshot, ModelVariant } from "@/types/ipc";
import {
  createLibrarySettingsActions,
  describeLibrary,
} from "./settings-overlay.library-actions";
import { createIntegritySettingsActions } from "./settings-overlay.integrity-actions";
import { createMaintenanceSettingsActions } from "./settings-overlay.maintenance-actions";
import { createModelSettingsActions } from "./settings-overlay.model-actions";
import type {
  SettingsActionContext,
  SettingsOverlayActions,
  SettingsOverlayControllerDependencies,
  SettingsOverlayMeta,
  SettingsOverlaySnapshot,
  SettingsOverlayState,
  SettingsOverlayStateControls,
} from "./settings-overlay.types";

export type {
  DangerDialog,
  ModelStatusView,
  PatchMeta,
  PatchState,
  SettingsActionContext,
  SettingsOverlayActions,
  SettingsOverlayControllerDependencies,
  SettingsOverlayMeta,
  SettingsOverlaySnapshot,
  SettingsOverlayState,
  SettingsOverlayStateControls,
} from "./settings-overlay.types";

export function createInitialSettingsOverlaySnapshot(
  initialSettings = useSettingsStore.getState().getAppSettingsSnapshot(),
): SettingsOverlaySnapshot {
  return {
    state: {
      libraryPath: null,
      libraryError: null,
      libraryRegistry: null,
      libraries: [],
      activeLibraryId: null,
      stemMode: initialSettings.stemMode,
      modelVariant: initialSettings.modelVariant,
      modelStatuses: {},
      downloadingModel: null,
      modelUpdate: null,
      runtimeStatus: null,
      runtimeUpdate: null,
      language: initialSettings.language ?? "en",
      hideBatchSeparate: initialSettings.hideBatchSeparate,
      coverArtBackdrop: initialSettings.coverArtBackdrop,
      executionProvider: initialSettings.executionProvider,
      availableExecutionProviders: initialSettings.availableExecutionProviders,
      eqEnabled: initialSettings.eqEnabled,
      eqGainsDb: initialSettings.eqGainsDb,
      crossfadeEnabled: initialSettings.crossfadeEnabled,
      crossfadeDurationMs: initialSettings.crossfadeDurationMs,
      librarySortMode: initialSettings.librarySortMode,
      themePreference: initialSettings.themePreference,
      updatePolicy: initialSettings.updatePolicy,
      integrityReport: null,
      integritySelection: new Set(),
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
  };
}

export function createSettingsOverlayActions(
  dependencies: SettingsOverlayControllerDependencies,
  controls: SettingsOverlayStateControls,
): SettingsOverlayActions {
  const patchState = (patch: Partial<SettingsOverlayState>) => {
    controls.setSnapshot((previous) => ({
      ...previous,
      state: {
        ...previous.state,
        ...patch,
      },
    }));
  };

  const patchMeta = (patch: Partial<SettingsOverlayMeta>) => {
    controls.setSnapshot((previous) => ({
      ...previous,
      meta: {
        ...previous.meta,
        ...patch,
      },
    }));
  };

  const applyRegistrySnapshot = (registry: LibraryRegistrySnapshot) => {
    const activeLibrary = registry.libraries.find(
      (library) => library.id === registry.active_library_id,
    );

    patchState({
      libraryRegistry: registry,
      libraries: registry.libraries,
      activeLibraryId: registry.active_library_id,
      libraryPath: activeLibrary ? describeLibrary(activeLibrary) : null,
      libraryError: null,
    });
  };

  const refreshLibraryRegistry = async () => {
    try {
      const registry = await dependencies.api.getLibraryRegistry();
      applyRegistrySnapshot(registry);
    } catch (error) {
      dependencies.notifyError(error);
    }
  };

  const refreshModelStatuses = async () => {
    try {
      const [standard, hq] = await Promise.all([
        dependencies.api.getModelStatus("htdemucs"),
        dependencies.api.getModelStatus("htdemucs_ft"),
      ]);

      patchState({
        modelStatuses: {
          htdemucs: {
            downloaded: standard.downloaded,
            legacy_install_present: standard.legacy_install_present,
            file_size: standard.file_size,
            installed_version: standard.installed_version,
            pinned_version: standard.pinned_version,
          },
          htdemucs_ft: {
            downloaded: hq.downloaded,
            legacy_install_present: hq.legacy_install_present,
            file_size: hq.file_size,
            installed_version: hq.installed_version,
            pinned_version: hq.pinned_version,
          },
        },
      });
    } catch {
      // Model status is display-only and should not block the rest of settings.
    }
  };

  const refreshRuntimeStatus = async () => {
    try {
      const status = await dependencies.api.getRuntimeBootstrapStatus();
      useRuntimeBootstrapStore.getState().updateStatus(status);
      patchState({
        runtimeStatus: {
          state: status.state,
          version: status.version,
          runtime_path: status.runtime_path,
          active_artifact_id: status.active_artifact_id,
          target_triple: status.target_triple,
          candidate_version: status.candidate_version,
          restart_required: status.restart_required,
          error: status.error?.message ?? null,
        },
      });
    } catch {
      // Runtime status is display-only.
    }
  };

  const applyModelVariant = async (variant: ModelVariant) => {
    try {
      const current = controls.getSnapshot();

      if (current.state.downloadingModel === variant) {
        return;
      }

      const status = current.state.modelStatuses[variant];

      if (!status?.downloaded) {
        patchState({ downloadingModel: variant });
        await dependencies.api.downloadModel(variant);
        await refreshModelStatuses();
        void useBootstrapStore.getState().loadStatus();
        patchState({ downloadingModel: null });
      }

      const settings = await dependencies.api.setModelVariant(variant);
      dependencies.settingsStore.hydrateAppSettings(settings);
      patchState({ modelVariant: settings.model_variant });
    } catch (error) {
      patchState({ downloadingModel: null });
      dependencies.notifyError(error);
    }
  };

  const downloadRuntimeAction = async () => {
    try {
      const status = await dependencies.api.downloadRuntime();
      useRuntimeBootstrapStore.getState().updateStatus(status);
      patchState({
        runtimeStatus: {
          state: status.state,
          version: status.version,
          runtime_path: status.runtime_path,
          active_artifact_id: status.active_artifact_id,
          target_triple: status.target_triple,
          candidate_version: status.candidate_version,
          restart_required: status.restart_required,
          error: status.error?.message ?? null,
        },
        runtimeUpdate: null,
      });
      // Refresh model statuses too since model actions may now be enabled.
      await refreshModelStatuses();
    } catch (error) {
      dependencies.notifyError(error);
    }
  };

  const updateRuntimeAction = async () => {
    await downloadRuntimeAction();
  };

  const checkRuntimeUpdatesAction = async () => {
    patchState({
      runtimeUpdate: {
        status: "checking",
        error: null,
        report: null,
      },
    });
    try {
      const report = await dependencies.api.checkRuntimeUpdates();
      patchState({
        runtimeUpdate: {
          status: "checked",
          error: null,
          report,
        },
      });
      await refreshRuntimeStatus();
    } catch (error) {
      patchState({
        runtimeUpdate: {
          status: "failed",
          error: error instanceof Error ? error.message : String(error),
          report: null,
        },
      });
    }
  };

  const setUpdatePolicyAction = async (
    policy: SettingsOverlayState["updatePolicy"],
  ) => {
    patchState({ updatePolicy: policy });
    await dependencies.settingsStore.setUpdatePolicy(policy);
    patchState({
      updatePolicy:
        dependencies.settingsStore.getAppSettingsSnapshot().updatePolicy,
    });
  };

  const deleteRuntimeAction = async () => {
    try {
      await dependencies.api.deleteRuntime();
      await refreshRuntimeStatus();
      // Refresh model statuses since model actions may now be disabled.
      await refreshModelStatuses();
    } catch (error) {
      dependencies.notifyError(error);
    }
  };

  const selectSingleDirectory = async (dialogTitle: string) => {
    const selected = await dependencies.openDirectory({
      directory: true,
      title: dialogTitle,
    });

    if (!selected) {
      return null;
    }

    return typeof selected === "string" ? selected : (selected[0] ?? null);
  };

  const closeDialog = () => {
    patchMeta({ dangerDialog: null });
  };

  const actionContext: SettingsActionContext = {
    dependencies,
    controls,
    patchState,
    patchMeta,
    refreshLibraryRegistry,
    refreshModelStatuses,
    applyModelVariant,
    selectSingleDirectory,
    closeDialog,
  };

  return {
    initialize: async () => {
      patchMeta({ isInitializing: true });

      const [registryResult, settingsResult] = await Promise.allSettled([
        dependencies.api.getLibraryRegistry(),
        dependencies.api.getSettings(),
      ]);

      if (registryResult.status === "fulfilled") {
        applyRegistrySnapshot(registryResult.value);
      } else {
        dependencies.notifyError(registryResult.reason);
      }

      if (settingsResult.status === "fulfilled") {
        dependencies.settingsStore.hydrateAppSettings(settingsResult.value);
        patchState({
          stemMode: settingsResult.value.stem_mode,
          modelVariant: settingsResult.value.model_variant,
          language: settingsResult.value.language ?? "en",
          hideBatchSeparate: settingsResult.value.hide_batch_separate,
          coverArtBackdrop: settingsResult.value.cover_art_backdrop,
          executionProvider: settingsResult.value.execution_provider,
          availableExecutionProviders:
            settingsResult.value.available_execution_providers,
          eqEnabled: settingsResult.value.eq_enabled,
          eqGainsDb: settingsResult.value.eq_gains_db,
          crossfadeEnabled: settingsResult.value.crossfade_enabled,
          crossfadeDurationMs: settingsResult.value.crossfade_duration_ms,
          librarySortMode: settingsResult.value.library_sort_mode,
          themePreference: settingsResult.value.theme_preference,
          updatePolicy: settingsResult.value.update_policy,
        });
      } else {
        dependencies.notifyError(settingsResult.reason);
      }

      patchMeta({ isInitializing: false });

      void refreshRuntimeStatus();
      void refreshModelStatuses();
    },

    refreshModelStatuses,
    refreshRuntimeStatus,
    downloadRuntime: downloadRuntimeAction,
    updateRuntime: updateRuntimeAction,
    checkRuntimeUpdates: checkRuntimeUpdatesAction,
    setUpdatePolicy: setUpdatePolicyAction,
    deleteRuntime: deleteRuntimeAction,
    openDeleteRuntimeDialog: () => {
      patchMeta({ dangerDialog: "delete_runtime" });
    },
    ...createLibrarySettingsActions(actionContext),
    ...createIntegritySettingsActions(actionContext),
    ...createModelSettingsActions(actionContext),
    ...createMaintenanceSettingsActions(actionContext),
    closeDialog,
  };
}
