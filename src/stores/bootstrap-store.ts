import { create } from "zustand";
import { tauriBackend, type Backend } from "@/lib/backend";
import type { ModelBootstrapStatusSnapshot } from "@/types/ipc";
import { mergeDownloadStatus } from "./merge-download-status";

interface BootstrapState {
  status: ModelBootstrapStatusSnapshot | null;
  loadStatus: () => Promise<void>;
  updateStatus: (status: ModelBootstrapStatusSnapshot) => void;
}

export function createBootstrapStore(backend: Backend = tauriBackend) {
  return create<BootstrapState>((set) => ({
    status: null,

    loadStatus: async () => {
      const status = await backend.settings.getModelBootstrapStatus();
      set({ status });
    },

    updateStatus: (incoming) =>
      set((s) => ({
        status: mergeDownloadStatus(s.status, incoming, "model_path"),
      })),
  }));
}

export const useBootstrapStore = createBootstrapStore();
