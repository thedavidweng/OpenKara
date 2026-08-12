import { useLibraryStore } from "@/stores/library-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";
import { usePlaylistStore } from "@/stores/playlist-store";
import { useQueueStore } from "@/stores/queue-store";
import { useRotationStore } from "@/stores/rotation-store";
import type { SongCommandStores } from "./types";

export function createZustandSongCommandStores(): SongCommandStores {
  return {
    library: {
      selectedSongIds: () => [...useLibraryStore.getState().selectedSongIds],
      songs: () => useLibraryStore.getState().songs,
      setSongsLanguage: (songIds, language) =>
        useLibraryStore.getState().setSongsLanguage(songIds, language),
      setSongsInstrumental: (songIds, instrumental) =>
        useLibraryStore.getState().setSongsInstrumental(songIds, instrumental),
      extractEmbeddedCoverArt: (songIds) =>
        useLibraryStore.getState().extractEmbeddedCoverArt(songIds),
    },
    playlist: {
      playlists: () => usePlaylistStore.getState().playlists,
      playlistSongSets: () => usePlaylistStore.getState().playlistSongSets,
      activePlaylistId: () => usePlaylistStore.getState().activePlaylistId,
      createPlaylist: (name) =>
        usePlaylistStore.getState().createPlaylist(name),
      addSongsToPlaylist: (playlistId, songIds) =>
        usePlaylistStore.getState().addSongsToPlaylist(playlistId, songIds),
      removeSongsFromPlaylist: (playlistId, songIds) =>
        usePlaylistStore
          .getState()
          .removeSongsFromPlaylist(playlistId, songIds),
    },
    queue: {
      addToQueue: (songId) => useQueueStore.getState().addToQueue(songId),
      playNext: (songId) => useQueueStore.getState().playNext(songId),
    },
    player: {
      currentSongId: () => usePlayerStore.getState().snapshot?.song_id ?? null,
      playNow: (songId) => usePlayerStore.getState().playNow(songId),
    },
    rotation: {
      singerNames: () => useRotationStore.getState().singerNames,
      getNextSinger: () => useRotationStore.getState().getNextSinger(),
      assignSingerToQueueEntry: (songId, singer) =>
        useRotationStore.getState().assignSingerToQueueEntry(songId, singer),
      advanceRotation: () => useRotationStore.getState().advanceRotation(),
    },
    lyrics: {
      clear: () => useLyricsStore.getState().clear(),
      fetchLyrics: (songId) => useLyricsStore.getState().fetchLyrics(songId),
    },
  };
}
