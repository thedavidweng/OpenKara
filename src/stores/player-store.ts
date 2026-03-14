import { create } from "zustand";
import * as api from "@/lib/tauri";
import type { PlaybackMode, PlaybackStateSnapshot } from "@/types/ipc";

interface PlayerState {
  snapshot: PlaybackStateSnapshot | null;
  positionMs: number;

  playSong: (songId: string) => Promise<void>;
  pause: () => Promise<void>;
  seek: (ms: number) => Promise<void>;
  setVolume: (level: number) => Promise<void>;
  setMode: (mode: PlaybackMode) => Promise<void>;
  updatePosition: (ms: number) => void;
  updateSnapshot: (snapshot: PlaybackStateSnapshot) => void;
  loadState: () => Promise<void>;
}

export const usePlayerStore = create<PlayerState>((set) => ({
  snapshot: null,
  positionMs: 0,

  playSong: async (songId) => {
    const snapshot = await api.play(songId);
    set({ snapshot, positionMs: snapshot.position_ms });
  },

  pause: async () => {
    const snapshot = await api.pause();
    set({ snapshot, positionMs: snapshot.position_ms });
  },

  seek: async (ms) => {
    const clamped = Math.max(0, ms);
    const snapshot = await api.seek(clamped);
    set({ snapshot, positionMs: snapshot.position_ms });
  },

  setVolume: async (level) => {
    const clamped = Math.max(0, Math.min(1, level));
    const snapshot = await api.setVolume(clamped);
    set({ snapshot });
  },

  setMode: async (mode) => {
    const snapshot = await api.setPlaybackMode(mode);
    set({ snapshot });
  },

  updatePosition: (ms) => {
    set({ positionMs: ms });
  },

  updateSnapshot: (snapshot) => {
    set({ snapshot, positionMs: snapshot.position_ms });
  },

  loadState: async () => {
    const snapshot = await api.getPlaybackState();
    set({ snapshot, positionMs: snapshot.position_ms });
  },
}));
