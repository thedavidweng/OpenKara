import { invoke } from "@tauri-apps/api/core";

export interface Playlist {
  id: string;
  name: string;
  song_count: number;
  created_at: number;
  updated_at: number;
}

export interface PlaylistSong {
  song_hash: string;
  added_at: number;
  sort_order: number;
  singer: string | null;
}

export interface RotationState {
  singer_names: string[];
  current_index: number;
  mode: string;
  active: boolean;
}

export function listPlaylists(): Promise<Playlist[]> {
  return invoke<Playlist[]>("list_playlists");
}

export function createPlaylist(name: string): Promise<Playlist> {
  return invoke<Playlist>("create_playlist", { name });
}

export function renamePlaylist(
  playlistId: string,
  name: string,
): Promise<void> {
  return invoke<void>("rename_playlist", { playlistId, name });
}

export function deletePlaylist(playlistId: string): Promise<void> {
  return invoke<void>("delete_playlist", { playlistId });
}

export function addSongsToPlaylist(
  playlistId: string,
  songHashes: string[],
): Promise<void> {
  return invoke<void>("add_songs_to_playlist", { playlistId, songHashes });
}

export function removeSongsFromPlaylist(
  playlistId: string,
  songHashes: string[],
): Promise<void> {
  return invoke<void>("remove_songs_from_playlist", {
    playlistId,
    songHashes,
  });
}

export function getPlaylistSongs(playlistId: string): Promise<PlaylistSong[]> {
  return invoke<PlaylistSong[]>("get_playlist_songs", { playlistId });
}

export function setRotationState(rotation: RotationState): Promise<void> {
  return invoke<void>("set_rotation_state", { rotation });
}

export function getRotationState(): Promise<RotationState> {
  return invoke<RotationState>("get_rotation_state");
}

export function advanceRotation(): Promise<RotationState> {
  return invoke<RotationState>("advance_rotation");
}

export function setQueueEntrySinger(
  playlistId: string,
  songHash: string,
  singer: string | null,
): Promise<void> {
  return invoke<void>("set_queue_entry_singer", {
    playlistId,
    songHash,
    singer,
  });
}
