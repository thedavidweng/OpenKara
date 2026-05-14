import { create } from "zustand";
import * as api from "@/lib/tauri";
import type { Playlist } from "@/lib/tauri/playlist";

interface PlaylistState {
  playlists: Playlist[];
  activePlaylistId: string | null;
  isLoading: boolean;

  loadPlaylists: () => Promise<void>;
  createPlaylist: (name: string) => Promise<Playlist>;
  renamePlaylist: (playlistId: string, name: string) => Promise<void>;
  deletePlaylist: (playlistId: string) => Promise<void>;
  addSongsToPlaylist: (
    playlistId: string,
    songHashes: string[],
  ) => Promise<void>;
  removeSongsFromPlaylist: (
    playlistId: string,
    songHashes: string[],
  ) => Promise<void>;
  getPlaylistSongs: (playlistId: string) => Promise<api.PlaylistSong[]>;
  setActivePlaylist: (playlistId: string | null) => void;
}

export const usePlaylistStore = create<PlaylistState>((set, get) => ({
  playlists: [],
  activePlaylistId: null,
  isLoading: false,

  loadPlaylists: async () => {
    set({ isLoading: true });
    try {
      const playlists = await api.listPlaylists();
      set({ playlists, isLoading: false });
    } catch {
      set({ isLoading: false });
    }
  },

  createPlaylist: async (name: string) => {
    const playlist = await api.createPlaylist(name);
    set((state) => ({ playlists: [...state.playlists, playlist] }));
    return playlist;
  },

  renamePlaylist: async (playlistId: string, name: string) => {
    await api.renamePlaylist(playlistId, name);
    set((state) => ({
      playlists: state.playlists.map((p) =>
        p.id === playlistId ? { ...p, name } : p,
      ),
    }));
  },

  deletePlaylist: async (playlistId: string) => {
    await api.deletePlaylist(playlistId);
    set((state) => ({
      playlists: state.playlists.filter((p) => p.id !== playlistId),
      activePlaylistId:
        state.activePlaylistId === playlistId ? null : state.activePlaylistId,
    }));
  },

  addSongsToPlaylist: async (playlistId, songHashes) => {
    await api.addSongsToPlaylist(playlistId, songHashes);
    // Refresh playlist list to update song counts
    await get().loadPlaylists();
  },

  removeSongsFromPlaylist: async (playlistId, songHashes) => {
    await api.removeSongsFromPlaylist(playlistId, songHashes);
    await get().loadPlaylists();
  },

  getPlaylistSongs: async (playlistId) => {
    return api.getPlaylistSongs(playlistId);
  },

  setActivePlaylist: (playlistId) => {
    set({ activePlaylistId: playlistId });
  },
}));
