import { create } from "zustand";
import type { CdgAvailability, CdgErrorCode } from "@/lib/tauri/cdg";

interface CdgState {
  hasCdg: boolean;
  songId: string | null;
  availability: CdgAvailability;
  errorCode: CdgErrorCode | null;
  frameVersion: number;
  transportGeneration: number;

  setSong: (songId: string | null, hasCdg: boolean) => void;
  setStatus: (
    availability: CdgAvailability,
    errorCode: CdgErrorCode | null,
  ) => void;
  setFrameVersion: (frameVersion: number, transportGeneration: number) => void;
  clear: () => void;
}

export const useCdgStore = create<CdgState>((set) => ({
  hasCdg: false,
  songId: null,
  availability: "none",
  errorCode: null,
  frameVersion: 0,
  transportGeneration: 0,

  setSong: (songId, hasCdg) => set({ songId, hasCdg }),
  setStatus: (availability, errorCode) => set({ availability, errorCode }),
  setFrameVersion: (frameVersion, transportGeneration) =>
    set({ frameVersion, transportGeneration }),
  clear: () =>
    set({
      hasCdg: false,
      songId: null,
      availability: "none",
      errorCode: null,
      frameVersion: 0,
      transportGeneration: 0,
    }),
}));
