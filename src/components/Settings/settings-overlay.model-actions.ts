import { useBootstrapStore } from "@/stores/bootstrap-store";
import type { ModelVariant } from "@/types/ipc";
import type {
  SettingsActionContext,
  SettingsOverlayActions,
} from "./settings-overlay.types";

export function createModelSettingsActions(
  context: SettingsActionContext,
): Pick<
  SettingsOverlayActions,
  | "selectModelVariant"
  | "confirmFtModel"
  | "deleteModel"
  | "checkModelUpdate"
  | "upgradeModel"
> {
  const {
    dependencies,
    controls,
    patchState,
    patchMeta,
    refreshModelStatuses,
    applyModelVariant,
    closeDialog,
  } = context;

  return {
    selectModelVariant: async (variant) => {
      const currentVariant = controls.getSnapshot().state.modelVariant;

      if (variant === "htdemucs_ft" && currentVariant !== "htdemucs_ft") {
        patchMeta({ dangerDialog: "ft_warning" });
        return;
      }

      await applyModelVariant(variant);
    },

    confirmFtModel: async () => {
      closeDialog();
      await applyModelVariant("htdemucs_ft");
    },

    deleteModel: async (variant) => {
      try {
        await dependencies.api.deleteModel(variant);
        await refreshModelStatuses();
        void useBootstrapStore.getState().loadStatus();
      } catch (error) {
        dependencies.notifyError(error);
      }
    },

    checkModelUpdate: async (variant: ModelVariant) => {
      try {
        patchState({ checkingModelUpdate: variant });
        const info = await dependencies.api.checkModelUpdate(variant);
        patchState({
          modelUpdateInfo: {
            ...controls.getSnapshot().state.modelUpdateInfo,
            [variant]: info,
          },
          checkingModelUpdate: null,
        });
      } catch (error) {
        patchState({ checkingModelUpdate: null });
        dependencies.notifyError(error);
      }
    },

    upgradeModel: async (variant: ModelVariant) => {
      try {
        patchMeta({ upgradingModel: variant });
        // Delete the existing model first, then download the latest. The
        // download resolves the upstream latest.json at call time, so this
        // always fetches the newest release.
        await dependencies.api.deleteModel(variant);
        await refreshModelStatuses();
        patchState({ downloadingModel: variant });
        await dependencies.api.downloadModel(variant);
        await refreshModelStatuses();
        void useBootstrapStore.getState().loadStatus();
        // Clear the cached update info since the install is now current.
        const remainingUpdates = {
          ...controls.getSnapshot().state.modelUpdateInfo,
        };
        delete remainingUpdates[variant];
        patchState({
          downloadingModel: null,
          modelUpdateInfo: remainingUpdates,
        });
        patchMeta({ upgradingModel: null });
      } catch (error) {
        patchState({ downloadingModel: null });
        patchMeta({ upgradingModel: null });
        dependencies.notifyError(error);
      }
    },
  };
}
