import { create } from "zustand";
import * as api from "@/lib/tauri";
import { notifyError } from "@/lib/errors";
import { romanizeLyricsLines } from "@/lib/lyrics-romanizer";
import {
  SONG_LANGUAGES,
  type SongLanguage,
} from "@/components/Library/song-list-item-menu";
import { useLibraryStore } from "@/stores/library-store";
import type { LyricLine, LyricsSource } from "@/types/ipc";

// F1: Generation counter to prevent stale fetch results from overwriting current lyrics.
let fetchGeneration = 0;

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
  isLoading: boolean;

  romanizedLines: string[];
  isRomanizing: boolean;
  showRomanized: boolean;

  fetchLyrics: (songId: string) => Promise<void>;
  setOffset: (songId: string, ms: number) => Promise<void>;
  adjustOffset: (songId: string, deltaMs: number) => Promise<void>;
  saveManualLyrics: (songId: string, text: string) => Promise<boolean>;
  setActiveLineIndex: (index: number) => void;
  toggleRomanized: () => void;
  romanizeCurrentLyrics: () => Promise<void>;
  clear: () => void;
}

export const useLyricsStore = create<LyricsState>((set, get) => ({
  songId: null,
  lines: [],
  source: null,
  offsetMs: 0,
  rawLrc: "",
  activeLineIndex: -1,
  isLoading: false,
  romanizedLines: [],
  isRomanizing: false,
  showRomanized: false,

  fetchLyrics: async (songId) => {
    const gen = ++fetchGeneration;
    set({
      isLoading: true,
      lines: [],
      source: null,
      rawLrc: "",
      activeLineIndex: -1,
      romanizedLines: [],
      showRomanized: false,
    });
    try {
      const payload = await api.fetchLyrics(songId);
      if (gen !== fetchGeneration) return;
      set({
        songId: payload.song_id,
        lines: payload.lines,
        source: payload.source,
        offsetMs: payload.offset_ms,
        rawLrc: payload.raw_lrc,
        isLoading: false,
      });

      // Auto-upgrade: if lyrics are unsynced (all time_ms === 0) and not
      // from LrcLib, try fetching synced lyrics from the network silently.
      // isLoading is already false so the UI shows local lyrics immediately.
      if (
        payload.lines.length > 0 &&
        payload.source !== "lrc_lib" &&
        payload.lines.every((l) => l.time_ms === 0)
      ) {
        try {
          const online = await api.fetchLyricsOnline(songId);
          if (
            gen === fetchGeneration &&
            get().songId === songId &&
            online.lines.length > 0 &&
            online.lines.some((l) => l.time_ms > 0)
          ) {
            set({
              songId: online.song_id,
              lines: online.lines,
              source: online.source,
              offsetMs: online.offset_ms,
              rawLrc: online.raw_lrc,
            });
          }
        } catch {
          // Network failure is non-fatal; keep original local lyrics
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
    await api.setLyricsOffset(songId, newOffset);
    set({ offsetMs: newOffset });
  },

  saveManualLyrics: async (songId, text) => {
    try {
      const payload = await api.saveManualLyrics(songId, text);
      set({
        songId: payload.song_id,
        lines: payload.lines,
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
      set({ activeLineIndex: index });
    }
  },

  toggleRomanized: () => {
    const { showRomanized, lines } = get();
    if (lines.length === 0) return;
    if (!showRomanized) {
      set({ showRomanized: true });
      void get().romanizeCurrentLyrics();
    } else {
      set({ showRomanized: false });
    }
  },

  romanizeCurrentLyrics: async () => {
    const { lines, isRomanizing } = get();
    if (isRomanizing || lines.length === 0) return;

    const texts = lines.map((l) => l.text);
    const currentSongId = get().songId;
    set({ isRomanizing: true });
    try {
      const language = getSongLanguage(currentSongId);
      const { result, requestId } = await romanizeLyricsLines(texts, language);
      // Item 7: Discard stale responses if the song changed during romanization.
      if (get().songId !== currentSongId || requestId === -1) {
        // requestId === -1 means Latin-only (no worker involved), always apply.
        if (requestId !== -1) return;
      }
      set({ romanizedLines: result });
    } catch (err) {
      console.error("Romanization failed:", err);
      set({ romanizedLines: [] });
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
      romanizedLines: [],
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
