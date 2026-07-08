import { buildAudiencePresentationSpec } from "@/lib/audience-presentation";
import type {
  AirPlayAudienceStatePayload,
  PlaybackStateSnapshot,
} from "@/types/ipc";

/** Fixed AirPlay viewport contract shared by the projector and TV renderer. */
export const AIRPLAY_AUDIENCE_VIEWPORT = {
  widthPx: 1280,
  heightPx: 720,
  bottomInsetPx: 0,
} as const;

/**
 * Inputs for pure audience projection.
 * Adapters (AirPlay sync, local fullscreen) gather these from stores/hooks.
 */
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

/**
 * Pure audience presentation projector.
 *
 * RATIONALE: Mode selection (idle / cdg / lyrics) and payload assembly must
 * stay identical for AirPlay and local fullscreen so the two surfaces do not
 * drift into different products. Transport and store wiring stay in adapters.
 */
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
