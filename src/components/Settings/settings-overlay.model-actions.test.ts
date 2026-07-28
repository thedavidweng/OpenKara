import { beforeEach, describe, expect, test, vi } from "vitest";

vi.mock("@/stores/bootstrap-store", () => ({
  useBootstrapStore: {
    getState: () => ({
      loadStatus: vi.fn(),
    }),
  },
}));

import { createModelSettingsActions } from "./settings-overlay.model-actions";
import type { SettingsActionContext } from "./settings-overlay.types";

function createContext(overrides?: {
  currentVariant?: string;
}): SettingsActionContext {
  const currentVariant = overrides?.currentVariant ?? "htdemucs";

  return {
    dependencies: {
      api: {
        deleteModel: vi.fn(),
        setModelVariant: vi.fn(),
        downloadModel: vi.fn(),
        getModelStatus: vi.fn(),
      } as unknown as SettingsActionContext["dependencies"]["api"],
      notifyError: vi.fn(),
      settingsStore: {
        hydrateAppSettings: vi.fn(),
      } as unknown as SettingsActionContext["dependencies"]["settingsStore"],
    } as unknown as SettingsActionContext["dependencies"],
    controls: {
      getSnapshot: vi.fn().mockReturnValue({
        state: {
          modelVariant: currentVariant,
          modelStatuses: {
            htdemucs: {
              downloaded: true,
              legacy_install_present: false,
              file_size_bytes: 100,
            },
            htdemucs_ft: {
              downloaded: true,
              legacy_install_present: false,
              file_size_bytes: 200,
            },
          },
        },
      }),
      setSnapshot: vi.fn(),
    },
    patchState: vi.fn(),
    patchMeta: vi.fn(),
    refreshLibraryRegistry: vi.fn(),
    refreshModelStatuses: vi.fn().mockResolvedValue(undefined),
    applyModelVariant: vi.fn().mockResolvedValue(undefined),
    selectSingleDirectory: vi.fn(),
    closeDialog: vi.fn(),
  };
}

describe("createModelSettingsActions", () => {
  let context: SettingsActionContext;
  let actions: ReturnType<typeof createModelSettingsActions>;

  beforeEach(() => {
    vi.clearAllMocks();
    context = createContext();
    actions = createModelSettingsActions(context);
  });

  describe("selectModelVariant", () => {
    test("shows ft_warning dialog when selecting htdemucs_ft from a different variant", async () => {
      // Default context has currentVariant = "htdemucs"
      await actions.selectModelVariant("htdemucs_ft");

      expect(context.patchMeta).toHaveBeenCalledWith({
        dangerDialog: "ft_warning",
      });
      expect(context.applyModelVariant).not.toHaveBeenCalled();
    });

    test("applies directly when already on htdemucs_ft and selecting htdemucs_ft", async () => {
      context = createContext({ currentVariant: "htdemucs_ft" });
      actions = createModelSettingsActions(context);

      await actions.selectModelVariant("htdemucs_ft");

      expect(context.patchMeta).not.toHaveBeenCalledWith(
        expect.objectContaining({ dangerDialog: "ft_warning" }),
      );
      expect(context.applyModelVariant).toHaveBeenCalledWith("htdemucs_ft");
    });

    test("applies directly for non-ft variant", async () => {
      await actions.selectModelVariant("htdemucs");

      expect(context.applyModelVariant).toHaveBeenCalledWith("htdemucs");
    });
  });

  describe("confirmFtModel", () => {
    test("closes the dialog and applies htdemucs_ft", async () => {
      await actions.confirmFtModel();

      expect(context.closeDialog).toHaveBeenCalledOnce();
      expect(context.applyModelVariant).toHaveBeenCalledWith("htdemucs_ft");
    });
  });

  describe("deleteModel", () => {
    test("calls api.deleteModel and refreshes statuses", async () => {
      vi.mocked(context.dependencies.api.deleteModel).mockResolvedValue(
        undefined,
      );

      await actions.deleteModel("htdemucs");

      expect(context.dependencies.api.deleteModel).toHaveBeenCalledWith(
        "htdemucs",
      );
      expect(context.refreshModelStatuses).toHaveBeenCalledOnce();
    });

    test("calls notifyError when deleteModel rejects", async () => {
      vi.mocked(context.dependencies.api.deleteModel).mockRejectedValue(
        new Error("delete failed"),
      );

      await actions.deleteModel("htdemucs_ft");

      expect(context.dependencies.notifyError).toHaveBeenCalledWith(
        expect.any(Error),
      );
    });

    test("calls loadStatus on the bootstrap store after deletion", async () => {
      vi.mocked(context.dependencies.api.deleteModel).mockResolvedValue(
        undefined,
      );

      await actions.deleteModel("htdemucs");

      expect(context.dependencies.api.deleteModel).toHaveBeenCalledOnce();
    });
  });

  describe("checkModelUpdates", () => {
    test("stores the update report when the check succeeds", async () => {
      const report = {
        generation: 4,
        release_id: "2026-08-01-001",
        models: [
          {
            variant: "htdemucs",
            state: "update_available" as const,
            installed_version: "model-v2.1.0",
            available_version: "model-v2.2.0",
            available_bytes: 355_000_000,
          },
        ],
      };
      context.dependencies.api.checkModelUpdates = vi
        .fn()
        .mockResolvedValue(report);
      actions = createModelSettingsActions(context);

      await actions.checkModelUpdates();

      expect(context.patchState).toHaveBeenCalledWith({
        modelUpdate: {
          status: "checking",
          error: null,
          generation: null,
          models: [],
        },
      });
      expect(context.patchState).toHaveBeenLastCalledWith({
        modelUpdate: {
          status: "checked",
          error: null,
          generation: 4,
          models: report.models,
        },
      });
    });

    test("reports a failed check without touching model readiness", async () => {
      context.dependencies.api.checkModelUpdates = vi
        .fn()
        .mockRejectedValue(new Error("offline"));
      actions = createModelSettingsActions(context);

      await actions.checkModelUpdates();

      expect(context.patchState).toHaveBeenLastCalledWith({
        modelUpdate: {
          status: "failed",
          error: "offline",
          generation: null,
          models: [],
        },
      });
      expect(context.dependencies.notifyError).not.toHaveBeenCalled();
    });
  });

  describe("updateModel", () => {
    test("downloads the update and refreshes statuses", async () => {
      vi.mocked(context.dependencies.api.downloadModel).mockResolvedValue(
        {} as never,
      );

      await actions.updateModel("htdemucs");

      expect(context.patchState).toHaveBeenCalledWith({
        downloadingModel: "htdemucs",
      });
      expect(context.dependencies.api.downloadModel).toHaveBeenCalledWith(
        "htdemucs",
      );
      expect(context.refreshModelStatuses).toHaveBeenCalledOnce();
      expect(context.patchState).toHaveBeenLastCalledWith({
        downloadingModel: null,
      });
    });

    test("ignores a second call while the variant is already downloading", async () => {
      vi.mocked(context.controls.getSnapshot).mockReturnValue({
        state: { downloadingModel: "htdemucs", modelStatuses: {} },
      } as never);

      await actions.updateModel("htdemucs");

      expect(context.dependencies.api.downloadModel).not.toHaveBeenCalled();
    });

    test("clears the downloading flag and notifies on failure", async () => {
      vi.mocked(context.dependencies.api.downloadModel).mockRejectedValue(
        new Error("download failed"),
      );

      await actions.updateModel("htdemucs_ft");

      expect(context.patchState).toHaveBeenLastCalledWith({
        downloadingModel: null,
      });
      expect(context.dependencies.notifyError).toHaveBeenCalledWith(
        expect.any(Error),
      );
    });
  });
});
