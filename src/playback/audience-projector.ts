import { buildAudiencePresentationSpec } from "@/lib/audience-presentation";
import type {
  AirPlayAudienceStatePayload,
  PlaybackStateSnapshot,
} from "@/types/ipc";

export const AIRPLAY_AUDIENCE_VIEWPORT = {
  width_px: 1280,
  height_px: 720,
  bottom_inset_px: 0,
} as const;

export interface AudienceProjectorInput {
  playbackSnapshot: PlaybackStateSnapshot | null;
  lyricsSongId: string | null;
  lines: AirPlayAudienceStatePayload["lines"];
  offsetMs: number;
  isLoading: boolean;
  lyricsFontStep: number;
  hasCdg: boolean;
  currentSongHasCdg: boolean;
  messages: AirPlayAudienceStatePayload["messages"];
  viewport?: AirPlayAudienceStatePayload["viewport"];
}

export function projectAudienceState(
  input: AudienceProjectorInput,
): AirPlayAudienceStatePayload {
  const {
    playbackSnapshot,
    lyricsSongId,
    lines,
    offsetMs,
    isLoading,
    lyricsFontStep,
    hasCdg,
    currentSongHasCdg,
    messages,
  } = input;
  const viewport = input.viewport ?? AIRPLAY_AUDIENCE_VIEWPORT;
  const songId = playbackSnapshot?.song_id ?? null;
  const lyricsBelongToCurrentSong = lyricsSongId === songId;
  const presentationSpec = buildAudiencePresentationSpec(lyricsFontStep);

  if (!songId) {
    return {
      mode: "idle",
      songId: null,
      lines: [],
      offsetMs: 0,
      isLoading,
      lyricsFontStep,
      messages,
      viewport,
      presentationSpec,
    };
  }

  if (hasCdg || currentSongHasCdg) {
    return {
      mode: "cdg",
      songId,
      lines: [],
      offsetMs: 0,
      isLoading,
      lyricsFontStep,
      messages,
      viewport,
      presentationSpec,
    };
  }

  return {
    mode: "lyrics",
    songId,
    lines: lyricsBelongToCurrentSong ? lines : [],
    offsetMs,
    isLoading: isLoading || !lyricsBelongToCurrentSong,
    lyricsFontStep,
    messages,
    viewport,
    presentationSpec,
  };
}

/** @deprecated Prefer projectAudienceState — kept for AirPlay adapter call sites. */
export function buildAirPlayAudienceState(
  input: AudienceProjectorInput,
): AirPlayAudienceStatePayload {
  return projectAudienceState(input);
}
