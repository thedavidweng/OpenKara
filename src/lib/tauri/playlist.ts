import type {
  Playlist,
  PlaylistBackend,
  PlaylistSong,
  RotationState,
} from "@/lib/backend/types";
import type { InvokeCommand } from "./invoke";

export function createPlaylistCommands(invoke: InvokeCommand): PlaylistBackend {
  return {
    listPlaylists: () => invoke<Playlist[]>("list_playlists"),

    createPlaylist: (name) => invoke<Playlist>("create_playlist", { name }),

    renamePlaylist: (playlistId, name) =>
      invoke<void>("rename_playlist", { playlistId, name }),

    deletePlaylist: (playlistId) =>
      invoke<void>("delete_playlist", { playlistId }),

    addSongsToPlaylist: (playlistId, songHashes) =>
      invoke<void>("add_songs_to_playlist", { playlistId, songHashes }),

    removeSongsFromPlaylist: (playlistId, songHashes) =>
      invoke<void>("remove_songs_from_playlist", { playlistId, songHashes }),

    getPlaylistSongs: (playlistId) =>
      invoke<PlaylistSong[]>("get_playlist_songs", { playlistId }),

    setRotationState: (rotation) =>
      invoke<void>("set_rotation_state", { rotation }),

    getRotationState: () => invoke<RotationState>("get_rotation_state"),

    advanceRotation: () => invoke<RotationState>("advance_rotation"),

    setQueueEntrySinger: (playlistId, songHash, singer) =>
      invoke<void>("set_queue_entry_singer", { playlistId, songHash, singer }),
  };
}
