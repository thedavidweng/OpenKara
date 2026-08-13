import type { SongLanguage } from "@/components/Library/song-list-item-menu";
import type { LyricsBackend } from "@/lib/backend";
import type { LocalAudienceRomanizeState } from "@/lib/local-audience-romanize";
import type { LyricLine, LyricsSource } from "@/types/ipc";

export type LyricsAlignment = "center" | "left";

/** The lyrics a view renders, and how far playback has moved through them. */
export interface LyricsData {
  songId: string | null;
  lines: LyricLine[];
  source: LyricsSource | null;
  offsetMs: number;
  rawLrc: string;
  activeLineIndex: number;
  activeWordIndex: number;
  isLoading: boolean;

  romanizedLines: string[];
  romanizedLinesIdentity: string | null;
  isRomanizing: boolean;
  showRomanized: boolean;
  lyricsAlignment: LyricsAlignment;
}

/**
 * The entries React components reach for through the store hook. Everything
 * else a caller can ask of the session is a method on `LyricsSession`.
 */
export interface LyricsActions {
  fetchLyrics: (songId: string) => Promise<void>;
  adjustOffset: (songId: string, deltaMs: number) => Promise<void>;
  resetOffset: (songId: string) => Promise<void>;
  saveManualLyrics: (songId: string, text: string) => Promise<boolean>;
  toggleRomanized: () => void;
  setRomanizedVisibility: (show: boolean) => void;
  applyRemoteRomanizeState: (state: LocalAudienceRomanizeState) => void;
  toggleLyricsAlignment: () => void;
  clear: () => void;
}

export interface LyricsState extends LyricsData, LyricsActions {}

/**
 * Turns lyric text into a Latin transcription. The Worker adapter answers
 * asynchronously and stamps a `requestId`; a `requestId` of `-1` marks a
 * result produced without leaving the caller's turn, which is never stale.
 */
export interface RomanizationPort {
  romanize(
    lines: readonly string[],
    language: SongLanguage | null,
  ): Promise<{ result: string[]; requestId: number }>;
}

/** The catalog language recorded for a song, when it is one we romanize. */
export interface SongLanguagePort {
  read(songId: string | null): SongLanguage | null;
}

/** Playback position in milliseconds, before the lyrics offset is applied. */
export interface PlaybackClockPort {
  readPositionMs(nowMs?: () => number): number;
}

export interface LyricsSessionDependencies {
  lyrics: LyricsBackend;
  romanization: RomanizationPort;
  songLanguage: SongLanguagePort;
  clock: PlaybackClockPort;
  reportError: (error: unknown) => void;
}
