import {
  SONG_LANGUAGES,
  type SongLanguage,
} from "@/components/Library/song-list-item-menu";
import { tauriBackend } from "@/lib/backend";
import { notifyError } from "@/lib/errors";
import {
  createLyricsSession,
  type PlaybackClockPort,
  type SongLanguagePort,
} from "@/lib/lyrics-session";
import { romanizeLyricsLines } from "@/lib/lyrics-romanizer";
import { useLibraryStore } from "@/stores/library-store";
import { selectCurrentPositionMs, usePlayerStore } from "@/stores/player-store";

function isSongLanguage(value: string): value is SongLanguage {
  return SONG_LANGUAGES.some((language) => language === value);
}

const songLanguage: SongLanguagePort = {
  read: (songId) => {
    if (!songId) return null;
    const language = useLibraryStore
      .getState()
      .songs.find((song) => song.hash === songId)?.language;
    return language && isSongLanguage(language) ? language : null;
  },
};

const playbackClock: PlaybackClockPort = {
  readPositionMs: (nowMs = () => performance.now()) => {
    const { snapshot, positionMs, playingSinceMs } = usePlayerStore.getState();
    return selectCurrentPositionMs(
      { snapshot, positionMs, playingSinceMs },
      nowMs,
    );
  },
};

export const lyricsSession = createLyricsSession({
  lyrics: tauriBackend.lyrics,
  romanization: { romanize: romanizeLyricsLines },
  songLanguage,
  clock: playbackClock,
  reportError: notifyError,
});

export const useLyricsStore = lyricsSession.store;

useLibraryStore.subscribe((state, prevState) => {
  const currentSongId = lyricsSession.getState().songId;
  if (!currentSongId) return;

  const previous = prevState.songs.find((song) => song.hash === currentSongId);
  const current = state.songs.find((song) => song.hash === currentSongId);
  if (previous?.language === current?.language) return;

  lyricsSession.refreshRomanization();
});
