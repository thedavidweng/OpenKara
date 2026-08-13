import type { SongLanguage } from "@/components/Library/song-list-item-menu";
import type {
  SongCommandDialogs,
  SongCommandPlaylist,
  SongCommandStores,
} from "@/lib/song-commands";
import type { Song } from "@/types/ipc";

export type SongCommandCall =
  | { call: "player.playNow"; songId: string }
  | { call: "queue.addToQueue"; songId: string }
  | { call: "queue.playNext"; songId: string }
  | { call: "rotation.assignSinger"; songId: string; singer: string | null }
  | { call: "rotation.advance" }
  | {
      call: "library.setSongsLanguage";
      songIds: string[];
      language: SongLanguage | null;
    }
  | {
      call: "library.setSongsInstrumental";
      songIds: string[];
      instrumental: boolean;
    }
  | { call: "library.extractEmbeddedCoverArt"; songIds: string[] }
  | { call: "lyrics.clear" }
  | { call: "lyrics.fetchLyrics"; songId: string }
  | { call: "playlist.create"; name: string }
  | { call: "playlist.addSongs"; playlistId: string; songIds: string[] }
  | { call: "playlist.removeSongs"; playlistId: string; songIds: string[] }
  | { call: "dialogs.editInfo" }
  | { call: "dialogs.properties" }
  | { call: "dialogs.confirmDelete"; songIds: string[] }
  | { call: "dialogs.createPlaylist" };

export type SongCommandCallName = SongCommandCall["call"];

export interface RecordingSongCommandState {
  selectedSongIds: string[];
  songs: Song[];
  playlists: SongCommandPlaylist[];
  playlistSongSets: Map<string, Set<string>>;
  activePlaylistId: string | null;
  singerNames: string[];
  nextSinger: string | null;
  currentSongId: string | null;
  createdPlaylistId: string;
}

export interface RecordingSongCommandPorts {
  stores: SongCommandStores;
  dialogs: SongCommandDialogs;
  state: RecordingSongCommandState;
  calls: SongCommandCall[];
  names(): SongCommandCallName[];
  failOn(call: SongCommandCallName, error: unknown): void;
}

/**
 * Store and dialog ports that log the calls a command issues instead of
 * running them, so a test can assert what a selection plus an action does
 * without standing up the stores behind it. Reads come from `state`, which
 * tests seed up front and may mutate between dispatches.
 */
export function createRecordingSongCommandPorts(
  seed: Partial<RecordingSongCommandState> = {},
): RecordingSongCommandPorts {
  const state: RecordingSongCommandState = {
    selectedSongIds: [],
    songs: [],
    playlists: [],
    playlistSongSets: new Map(),
    activePlaylistId: null,
    singerNames: [],
    nextSinger: null,
    currentSongId: null,
    createdPlaylistId: "playlist-created",
    ...seed,
  };

  const calls: SongCommandCall[] = [];
  const failures = new Map<SongCommandCallName, unknown>();

  const log = (call: SongCommandCall) => {
    calls.push(call);
  };

  const record = (call: SongCommandCall) => {
    log(call);
    return failures.has(call.call)
      ? Promise.reject(failures.get(call.call))
      : Promise.resolve();
  };

  return {
    state,
    calls,
    names: () => calls.map((entry) => entry.call),
    failOn: (call, error) => {
      failures.set(call, error);
    },
    stores: {
      library: {
        selectedSongIds: () => [...state.selectedSongIds],
        songs: () => state.songs,
        setSongsLanguage: (songIds, language) =>
          record({ call: "library.setSongsLanguage", songIds, language }),
        setSongsInstrumental: (songIds, instrumental) =>
          record({
            call: "library.setSongsInstrumental",
            songIds,
            instrumental,
          }),
        extractEmbeddedCoverArt: (songIds) =>
          record({ call: "library.extractEmbeddedCoverArt", songIds }),
      },
      playlist: {
        playlists: () => state.playlists,
        playlistSongSets: () => state.playlistSongSets,
        activePlaylistId: () => state.activePlaylistId,
        createPlaylist: async (name) => {
          await record({ call: "playlist.create", name });
          return { id: state.createdPlaylistId };
        },
        addSongsToPlaylist: (playlistId, songIds) =>
          record({ call: "playlist.addSongs", playlistId, songIds }),
        removeSongsFromPlaylist: (playlistId, songIds) =>
          record({ call: "playlist.removeSongs", playlistId, songIds }),
      },
      queue: {
        addToQueue: (songId) => log({ call: "queue.addToQueue", songId }),
        playNext: (songId) => log({ call: "queue.playNext", songId }),
      },
      player: {
        currentSongId: () => state.currentSongId,
        playNow: (songId) => record({ call: "player.playNow", songId }),
      },
      rotation: {
        singerNames: () => state.singerNames,
        getNextSinger: () => state.nextSinger,
        assignSingerToQueueEntry: (songId, singer) =>
          log({ call: "rotation.assignSinger", songId, singer }),
        advanceRotation: () => record({ call: "rotation.advance" }),
      },
      lyrics: {
        clear: () => log({ call: "lyrics.clear" }),
        fetchLyrics: (songId) => record({ call: "lyrics.fetchLyrics", songId }),
      },
    },
    dialogs: {
      editInfo: () => log({ call: "dialogs.editInfo" }),
      properties: () => log({ call: "dialogs.properties" }),
      confirmDelete: (songIds) =>
        log({ call: "dialogs.confirmDelete", songIds }),
      createPlaylist: () => log({ call: "dialogs.createPlaylist" }),
    },
  };
}
