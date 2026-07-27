import { create } from "zustand";
import * as api from "@/lib/tauri/playlist";
import { useQueueStore } from "./queue-store";

interface RotationState {
  active: boolean;
  singerNames: string[];
  currentIndex: number;
  mode: "round_robin" | "single";
  queueSingers: Map<string, string | null>;
  filterSinger: string | null;
  isLoading: boolean;

  loadRotation: () => Promise<void>;
  toggleActive: () => Promise<void>;
  addSinger: (name: string) => Promise<void>;
  removeSinger: (name: string) => Promise<void>;
  advanceRotation: () => Promise<void>;
  setCurrentSinger: (name: string) => Promise<void>;
  setFilterSinger: (name: string | null) => void;
  assignSingerToQueueEntry: (songHash: string, singer: string | null) => void;
  getNextSinger: () => string | null;
  shuffleQueue: () => void;
}

export const useRotationStore = create<RotationState>((set, get) => ({
  active: false,
  singerNames: [],
  currentIndex: 0,
  mode: "round_robin",
  queueSingers: new Map(),
  filterSinger: null,
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
    const { singerNames, currentIndex, mode, active, filterSinger } = get();
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
    set({
      singerNames: next,
      currentIndex: nextIndex,
      filterSinger: filterSinger === name ? null : filterSinger,
    });
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

  setFilterSinger: (name: string | null) => {
    set({ filterSinger: name });
  },

  advanceRotation: async () => {
    try {
      const state = await api.advanceRotation();
      const newSinger = state.singer_names[state.current_index] ?? null;
      set({
        singerNames: state.singer_names,
        currentIndex: state.current_index,
        filterSinger: newSinger,
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

  shuffleQueue: () => {
    const { queueSingers } = get();
    const { queue, setQueue } = useQueueStore.getState();
    if (queue.length <= 1) return;

    // Check if any songs have assigned singers
    const hasAssignments = queue.some((id) => queueSingers.has(id));

    if (!hasAssignments) {
      // Fisher-Yates shuffle
      const shuffled = [...queue];
      for (let i = shuffled.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1));
        [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
      }
      setQueue(shuffled);
      return;
    }

    // Interleave by singer to avoid back-to-back
    const groups = new Map<string, string[]>();
    const unassigned: string[] = [];
    for (const id of queue) {
      const singer = queueSingers.get(id);
      if (singer) {
        const group = groups.get(singer);
        if (group) group.push(id);
        else groups.set(singer, [id]);
      } else {
        unassigned.push(id);
      }
    }

    const shuffle = (arr: string[]) => {
      for (let i = arr.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1));
        [arr[i], arr[j]] = [arr[j], arr[i]];
      }
    };
    for (const group of groups.values()) shuffle(group);
    shuffle(unassigned);

    const sorted = [...groups.entries()].sort(
      (a, b) => b[1].length - a[1].length,
    );
    for (let i = 0; i < sorted.length;) {
      let j = i + 1;
      while (j < sorted.length && sorted[j][1].length === sorted[i][1].length) {
        j++;
      }
      // Fisher-Yates within the equal-size tier [i, j)
      for (let k = j - 1; k > i; k--) {
        const r = i + Math.floor(Math.random() * (k - i + 1));
        [sorted[k], sorted[r]] = [sorted[r], sorted[k]];
      }
      i = j;
    }
    const result: string[] = [];
    let hasMore = true;
    while (hasMore) {
      hasMore = false;
      for (const [, songs] of sorted) {
        if (songs.length > 0) {
          result.push(songs.shift()!);
          if (songs.length > 0) hasMore = true;
        }
      }
    }
    result.push(...unassigned);
    setQueue(result);
  },
}));
