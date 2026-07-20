import { create } from "zustand";
import * as api from "@/lib/tauri";
import type { RuntimeBootstrapStatusSnapshot } from "@/types/ipc";
import { mergeDownloadStatus } from "./merge-download-status";

interface RuntimeBootstrapState {
  status: RuntimeBootstrapStatusSnapshot | null;
  loadStatus: () => Promise<void>;
  updateStatus: (status: RuntimeBootstrapStatusSnapshot) => void;
}

export const useRuntimeBootstrapStore = create<RuntimeBootstrapState>(
  (set) => ({
    status: null,

    loadStatus: async () => {
      const status = await api.getRuntimeBootstrapStatus();
      set({ status });
    },

    updateStatus: (incoming) =>
      set((state) => ({
        status: mergeDownloadStatus(state.status, incoming, "runtime_path"),
      })),
  }),
);
