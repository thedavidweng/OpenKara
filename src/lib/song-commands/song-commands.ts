import {
  buildSongListContextMenuItems,
  SONG_LANGUAGES,
  type SongLanguage,
} from "@/components/Library/song-list-item-menu";
import { tauriBackend } from "@/lib/backend";
import { notifyError, notifySuccess } from "@/lib/errors";
import {
  songCanBeSeparated,
  songSupportsInstrumentalFlag,
} from "@/lib/song-media";
import type { Song } from "@/types/ipc";
import type {
  SongCommand,
  SongCommandContext,
  SongCommandDependencies,
  SongCommandLibraryStore,
  SongCommandPlaylist,
  SongCommands,
} from "./types";
import { createZustandSongCommandStores } from "./zustand-stores";

interface SongSelection {
  contextSongIds: string[];
  selectedCount: number;
  isMultiSelected: boolean;
  hasSeparableSongs: boolean;
  instrumentalSongIds: string[];
  instrumentalState: "checked" | "mixed" | "unchecked";
  language: SongLanguage | null;
}

function asSongLanguage(language: string | null): SongLanguage | null {
  return language != null && SONG_LANGUAGES.includes(language as SongLanguage)
    ? (language as SongLanguage)
    : null;
}

function sharedLanguage(songs: Song[]): SongLanguage | null {
  const first = asSongLanguage(songs[0]?.language ?? null);
  if (first == null) return null;
  return songs.every((song) => song.language === first) ? first : null;
}

function instrumentalStateOf(
  songs: Song[],
): SongSelection["instrumentalState"] {
  if (songs.length === 0) return "unchecked";
  if (songs.every((song) => song.instrumental)) return "checked";
  return songs.some((song) => song.instrumental) ? "mixed" : "unchecked";
}

function describeSelection(
  library: SongCommandLibraryStore,
  song: Song,
): SongSelection {
  const selectedSongIds = library.selectedSongIds();
  const isSelected = selectedSongIds.includes(song.hash);
  const contextSongIds = isSelected ? selectedSongIds : [song.hash];
  const contextSongIdSet = new Set(contextSongIds);
  const contextSongs = library
    .songs()
    .filter((candidate) => contextSongIdSet.has(candidate.hash));
  const instrumentalSongs = contextSongs.filter(songSupportsInstrumentalFlag);

  return {
    contextSongIds,
    selectedCount: selectedSongIds.length,
    isMultiSelected: selectedSongIds.length > 1 && isSelected,
    hasSeparableSongs: contextSongs.some(songCanBeSeparated),
    instrumentalSongIds: instrumentalSongs.map((candidate) => candidate.hash),
    instrumentalState: instrumentalStateOf(instrumentalSongs),
    language:
      selectedSongIds.length > 0
        ? sharedLanguage(contextSongs)
        : asSongLanguage(song.language),
  };
}

function playlistMembership(
  playlists: SongCommandPlaylist[],
  playlistSongSets: Map<string, Set<string>>,
  contextSongIds: string[],
): Map<string, "checked" | "mixed" | null> {
  const membership = new Map<string, "checked" | "mixed" | null>();
  for (const playlist of playlists) {
    const set = playlistSongSets.get(playlist.id);
    const matched = set ? contextSongIds.filter((id) => set.has(id)).length : 0;
    membership.set(
      playlist.id,
      matched === 0
        ? null
        : matched === contextSongIds.length
          ? "checked"
          : "mixed",
    );
  }
  return membership;
}

export function createSongCommands({
  backend = tauriBackend,
  stores = createZustandSongCommandStores(),
  dialogs,
}: SongCommandDependencies): SongCommands {
  const { library, playlist, queue, player, rotation, lyrics } = stores;

  const assignNextSinger = (songId: string) => {
    if (rotation.singerNames().length === 0) return;
    rotation.assignSingerToQueueEntry(songId, rotation.getNextSinger());
    void rotation.advanceRotation();
  };

  const changePlaylistMembership = async (
    change: () => Promise<void>,
    toast: string,
  ) => {
    try {
      await change();
      notifySuccess(toast);
    } catch (error) {
      notifyError(error);
    }
  };

  const execute = async (
    command: SongCommand,
    { song, t }: SongCommandContext,
  ): Promise<void> => {
    switch (command.id) {
      case "playNow":
        void player.playNow(song.hash);
        return;

      case "playNext":
        queue.playNext(song.hash);
        assignNextSinger(song.hash);
        return;

      case "addToQueue":
        queue.addToQueue(song.hash);
        assignNextSinger(song.hash);
        return;

      case "queueSelected":
        for (const songId of describeSelection(library, song).contextSongIds) {
          queue.addToQueue(songId);
        }
        return;

      case "separateSelected":
        await backend.maintenance
          .batchSeparate(describeSelection(library, song).contextSongIds)
          .catch(notifyError);
        return;

      case "toggleInstrumental": {
        const { instrumentalSongIds, instrumentalState } = describeSelection(
          library,
          song,
        );
        void library.setSongsInstrumental(
          instrumentalSongIds,
          instrumentalState !== "checked",
        );
        return;
      }

      case "setLanguage":
        void library.setSongsLanguage(
          describeSelection(library, song).contextSongIds,
          command.language,
        );
        return;

      case "extractCoverArt":
        void library.extractEmbeddedCoverArt([song.hash]);
        return;

      case "extractSelectedCoverArt":
        void library.extractEmbeddedCoverArt(
          describeSelection(library, song).contextSongIds,
        );
        return;

      case "extractEmbeddedLyrics":
        await backend.lyrics
          .extractEmbeddedLyrics(song.hash)
          .catch(notifyError);
        return;

      case "fetchLyricsOnline":
        try {
          const payload = await backend.lyrics.fetchLyricsOnline(
            song.hash,
            "user_replace",
          );
          if (
            player.currentSongId() === song.hash &&
            payload.lines.length > 0
          ) {
            lyrics.clear();
            void lyrics.fetchLyrics(song.hash);
          }
        } catch (error) {
          notifyError(error);
        }
        return;

      case "addToPlaylist": {
        const { contextSongIds } = describeSelection(library, song);
        await changePlaylistMembership(
          () => playlist.addSongsToPlaylist(command.playlistId, contextSongIds),
          t("playlist.addedToast", { count: contextSongIds.length }),
        );
        return;
      }

      case "removeFromPlaylist": {
        const { contextSongIds } = describeSelection(library, song);
        await changePlaylistMembership(
          () =>
            playlist.removeSongsFromPlaylist(
              command.playlistId,
              contextSongIds,
            ),
          t("playlist.removedFromPlaylistToast", {
            count: contextSongIds.length,
          }),
        );
        return;
      }

      case "removeFromActivePlaylist": {
        const activePlaylistId = playlist.activePlaylistId();
        if (activePlaylistId == null) return;
        await changePlaylistMembership(
          () => playlist.removeSongsFromPlaylist(activePlaylistId, [song.hash]),
          t("playlist.removedFromPlaylistToast", { count: 1 }),
        );
        return;
      }

      case "openCreatePlaylist":
        dialogs.createPlaylist();
        return;

      case "createPlaylistAndAdd": {
        const { contextSongIds } = describeSelection(library, song);
        try {
          const created = await playlist.createPlaylist(command.name.trim());
          await playlist.addSongsToPlaylist(created.id, contextSongIds);
          notifySuccess(
            t("playlist.createdAndAddedToast", {
              count: contextSongIds.length,
            }),
          );
        } catch (error) {
          notifyError(error);
        }
        return;
      }

      case "editInfo":
        dialogs.editInfo();
        return;

      case "openProperties":
        dialogs.properties();
        return;

      case "deleteSong":
        dialogs.confirmDelete([song.hash]);
        return;

      case "deleteSelected":
        dialogs.confirmDelete(describeSelection(library, song).contextSongIds);
        return;

      default: {
        const unhandled: never = command;
        return unhandled;
      }
    }
  };

  const buildMenu = (context: SongCommandContext) => {
    const { song, t } = context;
    const selection = describeSelection(library, song);
    const playlists = playlist.playlists();
    const dispatch = (command: SongCommand) => {
      void execute(command, context);
    };

    return buildSongListContextMenuItems({
      t,
      isMultiSelected: selection.isMultiSelected,
      selectedCount: selection.selectedCount,
      selectedSongIds: selection.contextSongIds,
      selectedHasSeparableSongs: selection.hasSeparableSongs,
      selectedCanToggleInstrumentalSongs:
        selection.instrumentalSongIds.length > 0,
      selectedInstrumentalState: selection.instrumentalState,
      selectedLanguage: selection.language,
      supportsEmbeddedLyrics: song.media_g_container !== "zip",
      playlists,
      songPlaylistMembership: playlistMembership(
        playlists,
        playlist.playlistSongSets(),
        selection.contextSongIds,
      ),
      activePlaylistId: playlist.activePlaylistId(),
      setSelectedLanguage: (language) =>
        dispatch({ id: "setLanguage", language }),
      queueAllSelected: () => dispatch({ id: "queueSelected" }),
      separateAllSelected: () => dispatch({ id: "separateSelected" }),
      toggleSelectedInstrumental: () => dispatch({ id: "toggleInstrumental" }),
      extractSelectedEmbeddedCoverArt: () =>
        dispatch({ id: "extractSelectedCoverArt" }),
      deleteSelected: () => dispatch({ id: "deleteSelected" }),
      playNow: () => dispatch({ id: "playNow" }),
      playNext: () => dispatch({ id: "playNext" }),
      addToQueue: () => dispatch({ id: "addToQueue" }),
      extractEmbeddedCoverArt: () => dispatch({ id: "extractCoverArt" }),
      extractEmbeddedLyrics: () => dispatch({ id: "extractEmbeddedLyrics" }),
      fetchLyricsOnline: () => dispatch({ id: "fetchLyricsOnline" }),
      editInfo: () => dispatch({ id: "editInfo" }),
      openProperties: () => dispatch({ id: "openProperties" }),
      deleteSong: () => dispatch({ id: "deleteSong" }),
      onAddToPlaylist: (playlistId) =>
        dispatch({ id: "addToPlaylist", playlistId }),
      onRemoveFromPlaylist: (playlistId) =>
        dispatch({ id: "removeFromPlaylist", playlistId }),
      onCreatePlaylistAndAdd: () => dispatch({ id: "openCreatePlaylist" }),
      onRemoveFromActivePlaylist: () =>
        dispatch({ id: "removeFromActivePlaylist" }),
    });
  };

  return { buildMenu, execute };
}
