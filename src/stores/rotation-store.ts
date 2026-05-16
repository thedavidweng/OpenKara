import { create } from "zustand";
import * as api from "@/lib/tauri/playlist";

interface RotationState {
  active: boolean;
  singerNames: string[];
  currentIndex: number;
  mode: "round_robin" | "single";
  queueSingers: Map<string, string | null>;
  isLoading: boolean;

  loadRotation: () => Promise<void>;
  toggleActive: () => Promise<void>;
  addSinger: (name: string) => Promise<void>;
  removeSinger: (name: string) => Promise<void>;
  advanceRotation: () => Promise<void>;
  setCurrentSinger: (name: string) => Promise<void>;
  assignSingerToQueueEntry: (songHash: string, singer: string | null) => void;
  getNextSinger: () => string | null;
}

export const useRotationStore = create<RotationState>((set, get) => ({
  active: false,
  singerNames: [],
  currentIndex: 0,
  mode: "round_robin",
  queueSingers: new Map(),
  isLoading: false,

  loadRotation: async () => {
    set({ isLoading: true });
    try {
      const state = await api.getRotationState();
      set({
        active: state.active,
        singerNames: state.singer_names,
        currentIndex: state.current_index,
        mode: (state.mode === "single" ? "single" : "round_robin") as
          | "round_robin"
          | "single",
        isLoading: false,
      });
    } catch {
      set({ isLoading: false });
    }
  },

  toggleActive: async () => {
    const { active, singerNames, currentIndex, mode } = get();
    const nextActive = !active;
    await api.setRotationState({
      active: nextActive,
      singer_names: singerNames,
      current_index: currentIndex,
      mode,
    });
    set({ active: nextActive });
  },

  addSinger: async (name: string) => {
    const { singerNames, currentIndex, mode, active } = get();
    const trimmed = name.trim();
    if (!trimmed || singerNames.includes(trimmed)) return;
    const next = [...singerNames, trimmed];
    await api.setRotationState({
      active,
      singer_names: next,
      current_index: currentIndex,
      mode,
    });
    set({ singerNames: next });
  },

  removeSinger: async (name: string) => {
    const { singerNames, currentIndex, mode, active } = get();
    const removedIndex = singerNames.indexOf(name);
    const next = singerNames.filter((n) => n !== name);
    let nextIndex = currentIndex;
    if (removedIndex !== -1 && removedIndex < currentIndex) {
      nextIndex = currentIndex - 1;
    }
    if (currentIndex >= next.length && next.length > 0) {
      nextIndex = next.length - 1;
    }
    await api.setRotationState({
      active,
      singer_names: next,
      current_index: nextIndex,
      mode,
    });
    set({ singerNames: next, currentIndex: nextIndex });
  },

  setCurrentSinger: async (name: string) => {
    const { singerNames, active, currentIndex, mode } = get();
    const nextIndex = singerNames.indexOf(name);
    if (nextIndex < 0 || nextIndex === currentIndex) return;
    await api.setRotationState({
      active,
      singer_names: singerNames,
      current_index: nextIndex,
      mode,
    });
    set({ currentIndex: nextIndex });
  },

  advanceRotation: async () => {
    try {
      const state = await api.advanceRotation();
      set({
        singerNames: state.singer_names,
        currentIndex: state.current_index,
      });
    } catch {
      // silently fail
    }
  },

  assignSingerToQueueEntry: (songHash, singer) => {
    set((state) => {
      const next = new Map(state.queueSingers);
      if (singer === null) {
        next.delete(songHash);
      } else {
        next.set(songHash, singer);
      }
      return { queueSingers: next };
    });
  },

  getNextSinger: () => {
    const { singerNames, currentIndex } = get();
    if (singerNames.length === 0) return null;
    return singerNames[currentIndex % singerNames.length];
  },
}));
