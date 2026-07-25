import { useBootstrapStore } from "@/stores/bootstrap-store";
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
  | "checkModelUpdates"
  | "updateModel"
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

    checkModelUpdates: async () => {
      patchState({
        modelUpdate: {
          status: "checking",
          error: null,
          generation: null,
          models: [],
        },
      });
      try {
        const report = await dependencies.api.checkModelUpdates();
        patchState({
          modelUpdate: {
            status: "checked",
            error: null,
            generation: report.generation,
            models: report.models,
          },
        });
      } catch (error) {
        // An update-check failure never affects installed-model readiness;
        // it is reported on its own line in the model section.
        patchState({
          modelUpdate: {
            status: "failed",
            error: error instanceof Error ? error.message : String(error),
            generation: null,
            models: [],
          },
        });
      }
    },

    updateModel: async (variant) => {
      const current = controls.getSnapshot();
      if (current.state.downloadingModel === variant) {
        return;
      }
      try {
        patchState({ downloadingModel: variant });
        await dependencies.api.downloadModel(variant);
        await refreshModelStatuses();
        void useBootstrapStore.getState().loadStatus();
        patchState({ downloadingModel: null });
      } catch (error) {
        patchState({ downloadingModel: null });
        dependencies.notifyError(error);
      }
    },
  };
}
