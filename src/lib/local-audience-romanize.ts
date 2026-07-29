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

export interface LocalAudienceRomanizeState {
  revision: number;
  songId: string | null;
  lyricsIdentity: string | null;
  showRomanized: boolean;
  isRomanizing: boolean;
  romanizedLines: string[];
}

export interface LocalAudienceRomanizeSetRequest {
  songId: string;
  showRomanized: boolean;
}

export function buildLyricsIdentity(lines: LyricLine[]): string | null {
  if (lines.length === 0) return null;
  return JSON.stringify(lines.map((line) => [line.time_ms, line.text]));
}

export async function emitLocalAudienceRomanizeState(
  state: LocalAudienceRomanizeState,
): Promise<void> {
  await emitTo(
    FULLSCREEN_PLAYER_WINDOW_LABEL,
    LOCAL_AUDIENCE_ROMANIZE_STATE_EVENT,
    state,
  );
}

export async function emitLocalAudienceRomanizeSetRequest(
  request: LocalAudienceRomanizeSetRequest,
): Promise<void> {
  await emitTo(MAIN_WINDOW_LABEL, LOCAL_AUDIENCE_ROMANIZE_SET_EVENT, request);
}

export async function emitLocalAudienceRomanizeSyncRequest(): Promise<void> {
  await emit(LOCAL_AUDIENCE_ROMANIZE_SYNC_REQUEST_EVENT, {});
}
