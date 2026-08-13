import { create } from "zustand";
import { tauriBackend, type Backend } from "@/lib/backend";
import type { RuntimeBootstrapStatusSnapshot } from "@/types/ipc";
import { mergeDownloadStatus } from "./merge-download-status";

interface RuntimeBootstrapState {
  status: RuntimeBootstrapStatusSnapshot | null;
  loadStatus: () => Promise<void>;
  updateStatus: (status: RuntimeBootstrapStatusSnapshot) => void;
}

export function createRuntimeBootstrapStore(backend: Backend = tauriBackend) {
  return create<RuntimeBootstrapState>((set) => ({
    status: null,

    loadStatus: async () => {
      const status = await backend.settings.getRuntimeBootstrapStatus();
      set({ status });
    },

    updateStatus: (incoming) =>
      set((state) => ({
        status: mergeDownloadStatus(state.status, incoming, "runtime_path"),
      })),
  }));
}

export const useRuntimeBootstrapStore = createRuntimeBootstrapStore();
