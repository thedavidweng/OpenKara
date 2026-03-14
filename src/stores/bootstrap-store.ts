import { create } from "zustand";
import * as api from "@/lib/tauri";
import type { ModelBootstrapStatusSnapshot } from "@/types/ipc";

interface BootstrapState {
  status: ModelBootstrapStatusSnapshot | null;
  loadStatus: () => Promise<void>;
  updateStatus: (status: ModelBootstrapStatusSnapshot) => void;
}

export const useBootstrapStore = create<BootstrapState>((set) => ({
  status: null,

  loadStatus: async () => {
    const status = await api.getModelBootstrapStatus();
    set({ status });
  },

  updateStatus: (status) => set({ status }),
}));
