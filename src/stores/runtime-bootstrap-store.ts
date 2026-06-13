import { create } from "zustand";
import * as api from "@/lib/tauri";
import type { RuntimeBootstrapStatusSnapshot } from "@/types/ipc";

interface RuntimeBootstrapState {
  status: RuntimeBootstrapStatusSnapshot | null;
  loadStatus: () => Promise<void>;
  updateStatus: (status: RuntimeBootstrapStatusSnapshot) => void;
}

function mergeRuntimeStatus(
  previous: RuntimeBootstrapStatusSnapshot | null,
  incoming: RuntimeBootstrapStatusSnapshot,
): RuntimeBootstrapStatusSnapshot {
  if (
    previous &&
    incoming.state === "downloading" &&
    previous.state === "downloading" &&
    incoming.runtime_path === previous.runtime_path
  ) {
    const prevDown = previous.downloaded_bytes ?? 0;
    const nextDown = Math.max(prevDown, incoming.downloaded_bytes ?? 0);
    const nextTotal = incoming.total_bytes ?? previous.total_bytes ?? null;
    return {
      ...incoming,
      downloaded_bytes: nextDown,
      total_bytes: nextTotal,
    };
  }
  return incoming;
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
        status: mergeRuntimeStatus(state.status, incoming),
      })),
  }),
);
