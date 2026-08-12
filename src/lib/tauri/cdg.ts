import type { CdgBackend, CdgStatus } from "@/lib/backend/types";
import type { InvokeCommand } from "./invoke";

export function createCdgCommands(invoke: InvokeCommand): CdgBackend {
  return {
    getCdgFrame: (songId, transportGeneration, positionMs, lastFrameVersion) =>
      invoke<ArrayBuffer>("get_cdg_frame", {
        songId,
        transportGeneration,
        positionMs: Math.round(positionMs),
        lastFrameVersion,
      }),

    getCdgStatus: (songId, transportGeneration) =>
      invoke<CdgStatus>("get_cdg_status", { songId, transportGeneration }),
  };
}
