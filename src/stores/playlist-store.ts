import { create } from "zustand";
import * as api from "@/lib/tauri";
import type { Playlist } from "@/lib/tauri/playlist";

interface PlaylistState {
  playlists: Playlist[];
  activePlaylistId: string | null;
  isLoading: boolean;
  playlistSongSets: Map<string, Set<string>>;

  loadPlaylists: () => Promise<void>;
  loadPlaylistSongSets: () => Promise<void>;
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
  playlistSongSets: new Map(),

  loadPlaylists: async () => {
    set({ isLoading: true });
    try {
      const playlists = await api.listPlaylists();
      set({ playlists, isLoading: false });
      await get().loadPlaylistSongSets();
    } catch {
      set({ isLoading: false });
    }
  },

  loadPlaylistSongSets: async () => {
    const { playlists } = get();
    const sets = new Map<string, Set<string>>();
    for (const p of playlists) {
      const songs = await api.getPlaylistSongs(p.id);
      sets.set(p.id, new Set(songs.map((s) => s.song_hash)));
    }
    set({ playlistSongSets: sets });
  },

  createPlaylist: async (name: string) => {
    const playlist = await api.createPlaylist(name);
    await get().loadPlaylists();
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
      activePlaylistId:
        state.activePlaylistId === playlistId ? null : state.activePlaylistId,
    }));
    await get().loadPlaylists();
  },

  addSongsToPlaylist: async (playlistId, songHashes) => {
    await api.addSongsToPlaylist(playlistId, songHashes);
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
