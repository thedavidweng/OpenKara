import { emit, emitTo } from "@tauri-apps/api/event";
import type { LyricLine } from "@/types/ipc";

export const LOCAL_AUDIENCE_ROMANIZE_STATE_EVENT =
  "openkara://local-audience-romanize-state";

export const LOCAL_AUDIENCE_ROMANIZE_SET_EVENT =
  "openkara://local-audience-romanize-set";

export const LOCAL_AUDIENCE_ROMANIZE_SYNC_REQUEST_EVENT =
  "openkara://local-audience-romanize-sync-request";

export const FULLSCREEN_PLAYER_WINDOW_LABEL = "fullscreen-player";
export const MAIN_WINDOW_LABEL = "main";

/**
 * Authoritative romanization snapshot projected from the main window to the
 * fullscreen audience window. `revision` is monotonic per main-window runtime
 * instance; the receiver discards any payload whose revision is older than
 * the latest retained revision.
 *
 * `lyricsIdentity` is an exact deterministic serialization of the ordered
 * source lyrics used to compute `romanizedLines`. The same `songId` may
 * temporarily reference different lyric content in the two WebViews (e.g.
 * local lyrics vs an online-upgraded set), so identity must not collapse to
 * songId or line count.
 */
export interface LocalAudienceRomanizeState {
  revision: number;
  songId: string | null;
  lyricsIdentity: string | null;
  showRomanized: boolean;
  isRomanizing: boolean;
  romanizedLines: string[];
}

/**
 * Explicit set request sent from the fullscreen control to the main window.
 * The desired boolean is sent (not a toggle) so the main window can validate
 * the request against its current authoritative state without ambiguity.
 */
export interface LocalAudienceRomanizeSetRequest {
  songId: string;
  showRomanized: boolean;
}

/**
 * Deterministic serialization of the ordered source lyrics used for
 * romanization. Returns null for empty lyrics so the receiver can treat a
 * null identity as "no romanization available yet" without matching it
 * against stale content.
 */
export function buildLyricsIdentity(lines: LyricLine[]): string | null {
  if (lines.length === 0) return null;
  return JSON.stringify(lines.map((line) => [line.time_ms, line.text]));
}

/** Project the authoritative state to the fullscreen audience window. */
export async function emitLocalAudienceRomanizeState(
  state: LocalAudienceRomanizeState,
): Promise<void> {
  await emitTo(
    FULLSCREEN_PLAYER_WINDOW_LABEL,
    LOCAL_AUDIENCE_ROMANIZE_STATE_EVENT,
    state,
  );
}

/** Send an explicit set request from the fullscreen control to the main window. */
export async function emitLocalAudienceRomanizeSetRequest(
  request: LocalAudienceRomanizeSetRequest,
): Promise<void> {
  await emitTo(MAIN_WINDOW_LABEL, LOCAL_AUDIENCE_ROMANIZE_SET_EVENT, request);
}

/** Request the current authoritative snapshot from the main window. */
export async function emitLocalAudienceRomanizeSyncRequest(): Promise<void> {
  await emit(LOCAL_AUDIENCE_ROMANIZE_SYNC_REQUEST_EVENT, {});
}
