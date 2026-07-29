import { create } from "zustand";
import * as api from "@/lib/tauri";
import { notifyError } from "@/lib/errors";
import { romanizeLyricsLines } from "@/lib/lyrics-romanizer";
import {
  buildLyricsIdentity,
  type LocalAudienceRomanizeState,
} from "@/lib/local-audience-romanize";
import { splitCompanionRomanization } from "@/lib/lyrics-companion-romanization";
import {
  SONG_LANGUAGES,
  type SongLanguage,
} from "@/components/Library/song-list-item-menu";
import { useLibraryStore } from "@/stores/library-store";
import type { LyricLine, LyricsSource } from "@/types/ipc";

let fetchGeneration = 0;

const AUTO_UPGRADE_PROTECTED_SOURCES: ReadonlySet<LyricsSource> =
  new Set<LyricsSource>([
    "manual",
    "manual_ttml",
    "manual_lys",
    "sidecar",
    "sidecar_ttml",
    "sidecar_lys",
  ]);

/**
 * Lift interleaved romaji lines out of a fetched lyric set.
 *
 * Bilingual sources ship the transcription as its own timestamped line, which
 * the parser faithfully turns into a peer lyric. Extracting it here — at the
 * single point where lines enter the store — keeps romanization out of the
 * lyric list entirely, so it can only surface as the attached sub-line under
 * its primary line while the Romanized-lyrics toggle is on.
 *
 * A complete source set seeds the romanization cache (identity set), so
 * enabling the toggle shows the source transcription without running the
 * romanizer. A partial set is still shown, but the identity stays null so the
 * romanizer recomputes a full set on enable.
 */
function normalizeFetchedLyrics(lines: LyricLine[]): {
  lines: LyricLine[];
  romanizedLines: string[];
  romanizedLinesIdentity: string | null;
} {
  const split = splitCompanionRomanization(lines);
  return {
    lines: split.lines,
    romanizedLines: split.romanizedLines,
    romanizedLinesIdentity: split.complete
      ? buildLyricsIdentity(split.lines)
      : null,
  };
}

function getSongLanguage(songId: string | null): SongLanguage | null {
  if (!songId) return null;
  const song = useLibraryStore.getState().songs.find((s) => s.hash === songId);
  const lang = song?.language;
  if (lang && SONG_LANGUAGES.includes(lang as SongLanguage)) {
    return lang as SongLanguage;
  }
  return null;
}

interface LyricsState {
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
  lyricsAlignment: "center" | "left";

  fetchLyrics: (songId: string) => Promise<void>;
  setOffset: (songId: string, ms: number) => Promise<void>;
  adjustOffset: (songId: string, deltaMs: number) => Promise<void>;
  resetOffset: (songId: string) => Promise<void>;
  saveManualLyrics: (songId: string, text: string) => Promise<boolean>;
  setActiveLineIndex: (index: number) => void;
  setActiveWordIndex: (index: number) => void;
  toggleRomanized: () => void;
  setRomanizedVisibility: (show: boolean) => void;
  applyRemoteRomanizeState: (state: LocalAudienceRomanizeState) => void;
  romanizeCurrentLyrics: () => Promise<void>;
  setLyricsAlignment: (alignment: "center" | "left") => void;
  toggleLyricsAlignment: () => void;
  clear: () => void;
}

export const useLyricsStore = create<LyricsState>((set, get) => ({
  songId: null,
  lines: [],
  source: null,
  offsetMs: 0,
  rawLrc: "",
  activeLineIndex: -1,
  activeWordIndex: -1,
  isLoading: false,
  romanizedLines: [],
  romanizedLinesIdentity: null,
  isRomanizing: false,
  showRomanized: false,
  lyricsAlignment: "left",

  fetchLyrics: async (songId) => {
    const gen = ++fetchGeneration;
    set({
      isLoading: true,
      lines: [],
      source: null,
      rawLrc: "",
      activeLineIndex: -1,
      activeWordIndex: -1,
      romanizedLines: [],
      romanizedLinesIdentity: null,
      showRomanized: false,
    });
    try {
      const payload = await api.fetchLyrics(songId);
      if (gen !== fetchGeneration) return;
      const normalized = normalizeFetchedLyrics(payload.lines);
      set({
        songId: payload.song_id,
        lines: normalized.lines,
        romanizedLines: normalized.romanizedLines,
        romanizedLinesIdentity: normalized.romanizedLinesIdentity,
        source: payload.source,
        offsetMs: payload.offset_ms,
        rawLrc: payload.raw_lrc,
        isLoading: false,
      });

      if (
        normalized.lines.length > 0 &&
        payload.source !== "lrc_lib" &&
        !(
          payload.source !== null &&
          AUTO_UPGRADE_PROTECTED_SOURCES.has(payload.source)
        ) &&
        normalized.lines.every((l) => l.time_ms === 0)
      ) {
        try {
          const online = await api.fetchLyricsOnline(songId, false);
          if (
            gen === fetchGeneration &&
            get().songId === songId &&
            online.lines.length > 0 &&
            online.lines.some((l) => l.time_ms > 0)
          ) {
            const normalizedOnline = normalizeFetchedLyrics(online.lines);
            set({
              songId: online.song_id,
              lines: normalizedOnline.lines,
              romanizedLines: normalizedOnline.romanizedLines,
              romanizedLinesIdentity: normalizedOnline.romanizedLinesIdentity,
              source: online.source,
              offsetMs: online.offset_ms,
              rawLrc: online.raw_lrc,
            });
          }
        } catch {
          // Network failure is non-fatal; keep original local lyrics.
        }
      }
    } catch (e) {
      if (gen !== fetchGeneration) return;
      notifyError(e);
      set({ lines: [], source: null, rawLrc: "", isLoading: false });
    }
  },

  setOffset: async (songId, ms) => {
    await api.setLyricsOffset(songId, ms);
    set({ offsetMs: ms });
  },

  adjustOffset: async (songId, deltaMs) => {
    const newOffset = get().offsetMs + deltaMs;
    set({ offsetMs: newOffset });
    try {
      await api.setLyricsOffset(songId, newOffset);
    } catch (e) {
      try {
        const payload = await api.fetchLyrics(songId);
        if (get().songId === songId) {
          set({ offsetMs: payload.offset_ms });
        }
      } catch {
        if (get().songId === songId) {
          set({ offsetMs: get().offsetMs - deltaMs });
        }
      }
      notifyError(e);
    }
  },

  resetOffset: async (songId) => {
    const currentOffset = get().offsetMs;
    if (currentOffset === 0) return;
    await get().adjustOffset(songId, -currentOffset);
  },

  saveManualLyrics: async (songId, text) => {
    try {
      const payload = await api.saveManualLyrics(songId, text);
      const normalized = normalizeFetchedLyrics(payload.lines);
      set({
        songId: payload.song_id,
        lines: normalized.lines,
        romanizedLines: normalized.romanizedLines,
        romanizedLinesIdentity: normalized.romanizedLinesIdentity,
        source: payload.source,
        offsetMs: payload.offset_ms,
        rawLrc: payload.raw_lrc,
      });
      return true;
    } catch (e) {
      notifyError(e);
      return false;
    }
  },

  setActiveLineIndex: (index) => {
    if (index !== get().activeLineIndex) {
      set({ activeLineIndex: index, activeWordIndex: -1 });
    }
  },

  setActiveWordIndex: (index) => {
    if (index !== get().activeWordIndex) {
      set({ activeWordIndex: index });
    }
  },

  toggleRomanized: () => {
    const { showRomanized, lines } = get();
    if (lines.length === 0) return;
    get().setRomanizedVisibility(!showRomanized);
  },

  setRomanizedVisibility: (show) => {
    const { showRomanized, lines, romanizedLines, romanizedLinesIdentity } =
      get();
    if (lines.length === 0) return;
    if (show === showRomanized) return;

    if (show) {
      set({ showRomanized: true });
      const currentIdentity = buildLyricsIdentity(lines);
      if (
        currentIdentity !== romanizedLinesIdentity ||
        romanizedLines.length === 0
      ) {
        void get().romanizeCurrentLyrics();
      }
    } else {
      set({ showRomanized: false });
    }
  },

  applyRemoteRomanizeState: (state) => {
    set({
      showRomanized: state.showRomanized,
      isRomanizing: state.isRomanizing,
      romanizedLines: [...state.romanizedLines],
      romanizedLinesIdentity: state.lyricsIdentity,
    });
  },

  setLyricsAlignment: (alignment) => set({ lyricsAlignment: alignment }),

  toggleLyricsAlignment: () =>
    set((state) => ({
      lyricsAlignment: state.lyricsAlignment === "left" ? "center" : "left",
    })),

  romanizeCurrentLyrics: async () => {
    const { lines, isRomanizing } = get();
    if (isRomanizing || lines.length === 0) return;

    const texts = lines.map((l) => l.text);
    const currentSongId = get().songId;
    set({ isRomanizing: true });
    try {
      const language = getSongLanguage(currentSongId);
      const { result, requestId } = await romanizeLyricsLines(texts, language);
      if (get().songId !== currentSongId || requestId === -1) {
        if (requestId !== -1) return;
      }
      set({
        romanizedLines: result,
        romanizedLinesIdentity: buildLyricsIdentity(get().lines),
      });
    } catch (err) {
      console.error("Romanization failed:", err);
      set({ romanizedLines: [], romanizedLinesIdentity: null });
    } finally {
      set({ isRomanizing: false });
    }
  },

  clear: () =>
    set({
      songId: null,
      lines: [],
      source: null,
      offsetMs: 0,
      rawLrc: "",
      activeLineIndex: -1,
      activeWordIndex: -1,
      romanizedLines: [],
      romanizedLinesIdentity: null,
      isRomanizing: false,
      showRomanized: false,
    }),
}));

useLibraryStore.subscribe((state, prevState) => {
  const currentSongId = useLyricsStore.getState().songId;
  if (!currentSongId) return;

  const prevSong = prevState.songs.find((s) => s.hash === currentSongId);
  const currSong = state.songs.find((s) => s.hash === currentSongId);
  if (prevSong?.language === currSong?.language) return;

  const { showRomanized, lines } = useLyricsStore.getState();
  if (showRomanized && lines.length > 0) {
    void useLyricsStore.getState().romanizeCurrentLyrics();
  }
});
