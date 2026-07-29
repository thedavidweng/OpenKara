import { invoke } from "@tauri-apps/api/core";

export function getCdgFrame(
  songId: string,
  transportGeneration: number,
  positionMs: number,
  lastFrameVersion: number,
): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>("get_cdg_frame", {
    songId,
    transportGeneration,
    positionMs: Math.round(positionMs),
    lastFrameVersion,
  });
}

export type CdgAvailability = "none" | "loading" | "ready" | "error";

export type CdgErrorCode =
  | "missing"
  | "empty"
  | "invalid"
  | "read_failed"
  | "zip_failed";

export interface CdgStatus {
  availability: CdgAvailability;
  songId: string | null;
  transportGeneration: number | null;
  packetCount: number | null;
  errorCode: CdgErrorCode | null;
}

export function getCdgStatus(
  songId: string,
  transportGeneration: number,
): Promise<CdgStatus> {
  return invoke<CdgStatus>("get_cdg_status", {
    songId,
    transportGeneration,
  });
}
