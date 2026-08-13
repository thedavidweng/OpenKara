import type { MaintenanceBackend } from "@/lib/backend/types";
import type {
  DeleteStemsResult,
  DowngradeResult,
  ExtractEmbeddedCoverArtResult,
  SeparationStatusSnapshot,
} from "@/types/ipc";
import type { InvokeCommand } from "./invoke";

export function createMaintenanceCommands(
  invoke: InvokeCommand,
): MaintenanceBackend {
  return {
    deleteAllStems: () => invoke<DeleteStemsResult>("delete_all_stems"),

    estimateStemsSize: () => invoke<number>("estimate_stems_size"),

    deleteAllCachedLyrics: () => invoke<number>("delete_all_cached_lyrics"),

    extractEmbeddedCoverArt: (songIds) =>
      invoke<ExtractEmbeddedCoverArtResult>("extract_embedded_cover_art", {
        songIds,
      }),

    batchSeparate: (songIds) => invoke<void>("batch_separate", { songIds }),

    cancelBatchSeparation: () => invoke<void>("cancel_batch_separation"),

    downgradeToTwoStem: (songId) =>
      invoke<SeparationStatusSnapshot>("downgrade_single_to_two_stem", {
        songId,
      }),

    downgradeAllToTwoStem: () =>
      invoke<DowngradeResult>("downgrade_all_to_two_stem"),

    estimateDowngradeSavings: () =>
      invoke<number>("estimate_downgrade_savings"),
  };
}
