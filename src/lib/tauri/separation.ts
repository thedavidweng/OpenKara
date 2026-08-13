import type { SeparationBackend } from "@/lib/backend/types";
import type { SeparationStatusSnapshot } from "@/types/ipc";
import type { InvokeCommand } from "./invoke";

export function createSeparationCommands(
  invoke: InvokeCommand,
): SeparationBackend {
  return {
    separate: (songId) =>
      invoke<SeparationStatusSnapshot>("separate", { songId }),

    cancelSeparation: (songId) => invoke<void>("cancel_separation", { songId }),

    getSeparationStatus: (songId) =>
      invoke<SeparationStatusSnapshot>("get_separation_status", { songId }),

    getAllSeparationStatuses: () =>
      invoke<SeparationStatusSnapshot[]>("get_all_separation_statuses"),

    upgradeToFourStem: (songId) =>
      invoke<SeparationStatusSnapshot>("upgrade_to_four_stem", { songId }),

    reSeparate: (songId, stemMode) =>
      invoke<SeparationStatusSnapshot>("re_separate", { songId, stemMode }),
  };
}
