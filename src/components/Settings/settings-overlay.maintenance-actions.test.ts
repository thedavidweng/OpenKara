import { beforeEach, describe, expect, test, vi } from "vitest";
import { createMaintenanceSettingsActions } from "./settings-overlay.maintenance-actions";
import type { SettingsActionContext } from "./settings-overlay.types";

function createContext(): SettingsActionContext {
  return {
    dependencies: {
      api: {
        estimateStemsSize: vi.fn(),
        deleteAllStems: vi.fn(),
        estimateDowngradeSavings: vi.fn(),
        downgradeAllToTwoStem: vi.fn(),
        getAllSeparationStatuses: vi.fn(),
        deleteAllCachedLyrics: vi.fn(),
      } as unknown as SettingsActionContext["dependencies"]["api"],
      notifyError: vi.fn(),
      libraryStore: {
        clearAllSeparationStatuses: vi.fn(),
        updateSeparationStatus: vi.fn(),
      } as unknown as SettingsActionContext["dependencies"]["libraryStore"],
      lyricsStore: {
        clear: vi.fn(),
      } as unknown as SettingsActionContext["dependencies"]["lyricsStore"],
    } as unknown as SettingsActionContext["dependencies"],
    controls: {
      getSnapshot: vi.fn(),
      setSnapshot: vi.fn(),
    },
    patchState: vi.fn(),
    patchMeta: vi.fn(),
    refreshLibraryRegistry: vi.fn(),
    refreshModelStatuses: vi.fn(),
    applyModelVariant: vi.fn(),
    selectSingleDirectory: vi.fn(),
    closeDialog: vi.fn(),
  };
}

describe("createMaintenanceSettingsActions", () => {
  let context: SettingsActionContext;
  let actions: ReturnType<typeof createMaintenanceSettingsActions>;

  beforeEach(() => {
    vi.clearAllMocks();
    context = createContext();
    actions = createMaintenanceSettingsActions(context);
  });

  describe("openDeleteStemsDialog", () => {
    test("calls estimateStemsSize and patches meta with the size", async () => {
      vi.mocked(context.dependencies.api.estimateStemsSize).mockResolvedValue(
        512,
      );

      await actions.openDeleteStemsDialog();

      expect(context.dependencies.api.estimateStemsSize).toHaveBeenCalledOnce();
      expect(context.patchMeta).toHaveBeenCalledWith({
        stemsSize: 512,
        dangerDialog: "delete_stems",
      });
    });

    test("patches stemsSize as null when estimateStemsSize rejects", async () => {
      vi.mocked(context.dependencies.api.estimateStemsSize).mockRejectedValue(
        new Error("disk error"),
      );

      await actions.openDeleteStemsDialog();

      expect(context.patchMeta).toHaveBeenCalledWith({
        stemsSize: null,
        dangerDialog: "delete_stems",
      });
    });
  });

  describe("confirmDeleteStems", () => {
    test("calls deleteAllStems, clears statuses, and resets meta", async () => {
      vi.mocked(context.dependencies.api.deleteAllStems).mockResolvedValue(
        undefined,
      );

      await actions.confirmDeleteStems();

      expect(context.patchMeta).toHaveBeenCalledWith({
        deletingStemsInProgress: true,
      });
      expect(context.dependencies.api.deleteAllStems).toHaveBeenCalledOnce();
      expect(
        (
          context.dependencies.libraryStore
            .clearAllSeparationStatuses as ReturnType<typeof vi.fn>
        ).mock.calls.length,
      ).toBe(1);
      expect(context.patchMeta).toHaveBeenCalledWith({
        deletingStemsInProgress: false,
        dangerDialog: null,
      });
    });

    test("calls notifyError on failure but still resets loading state", async () => {
      vi.mocked(context.dependencies.api.deleteAllStems).mockRejectedValue(
        new Error("delete failed"),
      );

      await actions.confirmDeleteStems();

      expect(context.dependencies.notifyError).toHaveBeenCalledWith(
        expect.any(Error),
      );
      expect(context.patchMeta).toHaveBeenCalledWith({
        deletingStemsInProgress: false,
        dangerDialog: null,
      });
    });
  });

  describe("openDowngradeDialog", () => {
    test("calls estimateDowngradeSavings and patches meta", async () => {
      vi.mocked(
        context.dependencies.api.estimateDowngradeSavings,
      ).mockResolvedValue(4096);

      await actions.openDowngradeDialog();

      expect(
        context.dependencies.api.estimateDowngradeSavings,
      ).toHaveBeenCalledOnce();
      expect(context.patchMeta).toHaveBeenCalledWith({
        downgradeSavings: 4096,
        dangerDialog: "downgrade_stems",
      });
    });

    test("patches downgradeSavings as null when estimate rejects", async () => {
      vi.mocked(
        context.dependencies.api.estimateDowngradeSavings,
      ).mockRejectedValue(new Error("estimate failed"));

      await actions.openDowngradeDialog();

      expect(context.patchMeta).toHaveBeenCalledWith({
        downgradeSavings: null,
        dangerDialog: "downgrade_stems",
      });
    });
  });

  describe("confirmDowngrade", () => {
    test("calls downgradeAllToTwoStem, refreshes statuses, and resets meta", async () => {
      const statuses = [
        {
          song_id: "s1",
          state: "completed",
          percent: 100,
          cache_hit: false,
          vocals_path: "v.ogg",
          accomp_path: "a.ogg",
          drums_path: null,
          bass_path: null,
          other_path: null,
          model_variant: "htdemucs",
          error: null,
        },
      ];

      vi.mocked(
        context.dependencies.api.downgradeAllToTwoStem,
      ).mockResolvedValue(undefined);
      vi.mocked(
        context.dependencies.api.getAllSeparationStatuses,
      ).mockResolvedValue(statuses);

      await actions.confirmDowngrade();

      expect(context.patchMeta).toHaveBeenCalledWith({
        downgradingInProgress: true,
      });
      expect(
        context.dependencies.api.downgradeAllToTwoStem,
      ).toHaveBeenCalledOnce();
      expect(
        context.dependencies.api.getAllSeparationStatuses,
      ).toHaveBeenCalledOnce();
      expect(
        (
          context.dependencies.libraryStore
            .clearAllSeparationStatuses as ReturnType<typeof vi.fn>
        ).mock.calls.length,
      ).toBe(1);
      expect(
        (
          context.dependencies.libraryStore
            .updateSeparationStatus as ReturnType<typeof vi.fn>
        ).mock.calls[0][0],
      ).toEqual(statuses[0]);
      expect(context.patchMeta).toHaveBeenCalledWith({
        downgradingInProgress: false,
        dangerDialog: null,
      });
    });

    test("calls notifyError on failure but still resets loading state", async () => {
      vi.mocked(
        context.dependencies.api.downgradeAllToTwoStem,
      ).mockRejectedValue(new Error("downgrade failed"));

      await actions.confirmDowngrade();

      expect(context.dependencies.notifyError).toHaveBeenCalledWith(
        expect.any(Error),
      );
      expect(context.patchMeta).toHaveBeenCalledWith({
        downgradingInProgress: false,
        dangerDialog: null,
      });
    });
  });

  describe("openDeleteLyricsDialog", () => {
    test("patches dangerDialog to delete_lyrics", () => {
      actions.openDeleteLyricsDialog();

      expect(context.patchMeta).toHaveBeenCalledWith({
        dangerDialog: "delete_lyrics",
      });
    });
  });

  describe("confirmDeleteLyrics", () => {
    test("calls deleteAllCachedLyrics, clears lyrics store, and resets meta", async () => {
      vi.mocked(
        context.dependencies.api.deleteAllCachedLyrics,
      ).mockResolvedValue(undefined);

      await actions.confirmDeleteLyrics();

      expect(context.patchMeta).toHaveBeenCalledWith({
        deletingLyricsInProgress: true,
      });
      expect(
        context.dependencies.api.deleteAllCachedLyrics,
      ).toHaveBeenCalledOnce();
      expect(
        (context.dependencies.lyricsStore.clear as ReturnType<typeof vi.fn>)
          .mock.calls.length,
      ).toBe(1);
      expect(context.patchMeta).toHaveBeenCalledWith({
        deletingLyricsInProgress: false,
        dangerDialog: null,
      });
    });

    test("calls notifyError on failure but still resets loading state", async () => {
      vi.mocked(
        context.dependencies.api.deleteAllCachedLyrics,
      ).mockRejectedValue(new Error("delete failed"));

      await actions.confirmDeleteLyrics();

      expect(context.dependencies.notifyError).toHaveBeenCalledWith(
        expect.any(Error),
      );
      expect(context.patchMeta).toHaveBeenCalledWith({
        deletingLyricsInProgress: false,
        dangerDialog: null,
      });
    });
  });
});
