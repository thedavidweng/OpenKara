import { useLibraryStore } from "@/stores/library-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";
import { usePlaylistStore } from "@/stores/playlist-store";
import { useQueueStore } from "@/stores/queue-store";
import { useRotationStore } from "@/stores/rotation-store";
import * as api from "@/lib/tauri";
import { notifyError, notifySuccess } from "@/lib/errors";
import {
  songCanBeSeparated,
  songSupportsInstrumentalFlag,
} from "@/lib/song-media";
import {
  buildSongListContextMenuItems,
  SONG_LANGUAGES,
  type SongLanguage,
} from "./song-list-item-menu";
import type { Song } from "@/types/ipc";

type TranslateFn = (
  key: string,
  options?: Record<string, string | number>,
) => string;

export interface SongListContextMenuActions {
  setEditDialogOpen: (open: boolean) => void;
  setPropertiesDialogOpen: (open: boolean) => void;
  setDeleteSongIds: (songIds: string[]) => void;
  setPlaylistDialogOpen: (open: boolean) => void;
}

function computePlaylistMembership(
  playlists: ReturnType<typeof usePlaylistStore.getState>["playlists"],
  playlistSongSets: ReturnType<
    typeof usePlaylistStore.getState
  >["playlistSongSets"],
  contextSongIds: string[],
): Map<string, "checked" | "mixed" | null> {
  const membership = new Map<string, "checked" | "mixed" | null>();
  for (const playlist of playlists) {
    const set = playlistSongSets.get(playlist.id);
    if (!set) {
      membership.set(playlist.id, null);
      continue;
    }
    const intersectionSize = contextSongIds.filter((id) => set.has(id)).length;
    if (intersectionSize === 0) {
      membership.set(playlist.id, null);
    } else if (intersectionSize === contextSongIds.length) {
      membership.set(playlist.id, "checked");
    } else {
      membership.set(playlist.id, "mixed");
    }
  }
  return membership;
}

export function buildSongListContextMenuForSong(
  song: Song,
  t: TranslateFn,
  actions: SongListContextMenuActions,
) {
  const library = useLibraryStore.getState();
  const playlistStore = usePlaylistStore.getState();
  const rotation = useRotationStore.getState();
  const selectedSongIds = library.selectedSongIds;
  const isSelected = selectedSongIds.has(song.hash);
  const contextSongIds = isSelected ? [...selectedSongIds] : [song.hash];
  const contextSongIdSet = new Set(contextSongIds);
  const selectedSongs = library.songs.filter((candidate) =>
    contextSongIdSet.has(candidate.hash),
  );
  const selectedHasSeparableSongs = selectedSongs.some(songCanBeSeparated);
  const selectedInstrumentalSongs = selectedSongs.filter(
    songSupportsInstrumentalFlag,
  );
  const selectedCanToggleInstrumentalSongs =
    selectedInstrumentalSongs.length > 0;
  const selectedInstrumentalState =
    selectedInstrumentalSongs.length === 0
      ? "unchecked"
      : selectedInstrumentalSongs.every((candidate) => candidate.instrumental)
        ? "checked"
        : selectedInstrumentalSongs.some((candidate) => candidate.instrumental)
          ? "mixed"
          : "unchecked";
  const selectedLanguage: SongLanguage | null =
    selectedSongIds.size > 0
      ? (() => {
          const first = selectedSongs[0]?.language;
          if (!first || !SONG_LANGUAGES.includes(first as SongLanguage))
            return null;
          const allSame = selectedSongs.every(
            (candidate) => candidate.language === first,
          );
          return allSame ? (first as SongLanguage) : null;
        })()
      : song.language && SONG_LANGUAGES.includes(song.language as SongLanguage)
        ? (song.language as SongLanguage)
        : null;
  const selectedLanguageSongIds =
    selectedSongIds.size > 0 ? [...selectedSongIds] : [song.hash];
  const isMultiSelected = selectedSongIds.size > 1 && isSelected;
  const supportsEmbeddedLyrics = song.media_g_container !== "zip";

  return buildSongListContextMenuItems({
    t: (key, options) => t(key, options) as string,
    isMultiSelected,
    selectedCount: selectedSongIds.size,
    selectedSongIds: contextSongIds,
    selectedHasSeparableSongs,
    selectedCanToggleInstrumentalSongs,
    selectedInstrumentalState,
    selectedLanguage,
    setSelectedLanguage: (language) => {
      void library.setSongsLanguage(selectedLanguageSongIds, language);
    },
    supportsEmbeddedLyrics,
    queueAllSelected: () => {
      const queue = useQueueStore.getState();
      for (const id of contextSongIds) {
        queue.addToQueue(id);
      }
    },
    separateAllSelected: () => {
      api.batchSeparate([...selectedSongIds]).catch(notifyError);
    },
    toggleSelectedInstrumental: () => {
      const nextInstrumental = selectedInstrumentalState !== "checked";
      void library.setSongsInstrumental(
        selectedInstrumentalSongs.map((candidate) => candidate.hash),
        nextInstrumental,
      );
    },
    extractSelectedEmbeddedCoverArt: () => {
      void library.extractEmbeddedCoverArt([...selectedSongIds]);
    },
    deleteSelected: () => actions.setDeleteSongIds(contextSongIds),
    playNow: () => usePlayerStore.getState().playNow(song.hash),
    playNext: () => {
      useQueueStore.getState().playNext(song.hash);
      if (rotation.singerNames.length > 0) {
        const singer = rotation.getNextSinger();
        rotation.assignSingerToQueueEntry(song.hash, singer);
        void rotation.advanceRotation();
      }
    },
    addToQueue: () => {
      useQueueStore.getState().addToQueue(song.hash);
      if (rotation.singerNames.length > 0) {
        const singer = rotation.getNextSinger();
        rotation.assignSingerToQueueEntry(song.hash, singer);
        void rotation.advanceRotation();
      }
    },
    extractEmbeddedCoverArt: () => {
      void library.extractEmbeddedCoverArt([song.hash]);
    },
    extractEmbeddedLyrics: () => {
      api.extractEmbeddedLyrics(song.hash).catch(notifyError);
    },
    fetchLyricsOnline: () => {
      api
        .fetchLyricsOnline(song.hash)
        .then((payload) => {
          const currentSongId = usePlayerStore.getState().snapshot?.song_id;
          if (currentSongId === song.hash && payload.lines.length > 0) {
            useLyricsStore.getState().clear();
            useLyricsStore.getState().fetchLyrics(song.hash);
          }
        })
        .catch(notifyError);
    },
    editInfo: () => actions.setEditDialogOpen(true),
    openProperties: () => actions.setPropertiesDialogOpen(true),
    deleteSong: () => actions.setDeleteSongIds([song.hash]),
    playlists: playlistStore.playlists,
    songPlaylistMembership: computePlaylistMembership(
      playlistStore.playlists,
      playlistStore.playlistSongSets,
      contextSongIds,
    ),
    onAddToPlaylist: async (playlistId) => {
      try {
        await playlistStore.addSongsToPlaylist(playlistId, contextSongIds);
        notifySuccess(
          t("playlist.addedToast", { count: contextSongIds.length }),
        );
      } catch (error) {
        notifyError(error);
      }
    },
    onRemoveFromPlaylist: async (playlistId) => {
      try {
        await playlistStore.removeSongsFromPlaylist(playlistId, contextSongIds);
        notifySuccess(
          t("playlist.removedFromPlaylistToast", {
            count: contextSongIds.length,
          }),
        );
      } catch (error) {
        notifyError(error);
      }
    },
    onCreatePlaylistAndAdd: () => {
      actions.setPlaylistDialogOpen(true);
    },
    activePlaylistId: playlistStore.activePlaylistId,
    onRemoveFromActivePlaylist: async () => {
      const activePlaylistId = playlistStore.activePlaylistId;
      if (activePlaylistId) {
        try {
          await playlistStore.removeSongsFromPlaylist(activePlaylistId, [
            song.hash,
          ]);
          notifySuccess(t("playlist.removedFromPlaylistToast", { count: 1 }));
        } catch (error) {
          notifyError(error);
        }
      }
    },
  });
}

export function getSongListContextSongIds(song: Song): string[] {
  const selectedSongIds = useLibraryStore.getState().selectedSongIds;
  return selectedSongIds.has(song.hash) ? [...selectedSongIds] : [song.hash];
}
