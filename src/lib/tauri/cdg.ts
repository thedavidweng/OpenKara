import { invoke } from "@tauri-apps/api/core";

/**
 * Fetch a CDG frame envelope (32-byte header + optional RGBA payload) for the
 * given playback position.
 *
 * Parameters:
 * - `songId`: The current song ID (must match backend's active CDG song).
 * - `transportGeneration`: The transport generation from the playback snapshot.
 * - `positionMs`: The current playback position in milliseconds.
 * - `lastFrameVersion`: The last frame version the caller has, or 0 if none.
 *
 * Returns a binary `ArrayBuffer`:
 * - 0 bytes: no active CDG, stale song/generation, or error state.
 * - 32 bytes (header only): active CDG but caller already has current frame.
 * - 32 + 221,184 bytes: caller needs the current frame (RGBA payload present).
 *
 * Use `parseCdgFrameResponse()` to parse the response into a structured envelope.
 *
 * PERF: The backend returns raw bytes via `tauri::ipc::Response`, which the
 * IPC bridge delivers as an `ArrayBuffer` - no base64 encoding/decoding is
 * involved. This is a deliberate performance choice: base64 inflates the
 * payload by ~33% and requires an expensive O(n) decode loop on the main
 * thread. Do not change the return type to `string` without benchmarking.
 */
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

/** CDG availability status from the backend. */
export type CdgAvailability = "none" | "loading" | "ready" | "error";

/** CDG error code from the backend. */
export type CdgErrorCode =
  | "missing"
  | "empty"
  | "invalid"
  | "read_failed"
  | "zip_failed";

/** CDG status payload returned by `getCdgStatus`. */
export interface CdgStatus {
  availability: CdgAvailability;
  songId: string | null;
  transportGeneration: number | null;
  packetCount: number | null;
  errorCode: CdgErrorCode | null;
}

/**
 * Query the current CDG availability status for a song and transport generation.
 *
 * Returns `{ availability: "none" }` if no CDG is active or the song/generation
 * doesn't match the backend's current CDG slot.
 */
export function getCdgStatus(
  songId: string,
  transportGeneration: number,
): Promise<CdgStatus> {
  return invoke<CdgStatus>("get_cdg_status", {
    songId,
    transportGeneration,
  });
}
