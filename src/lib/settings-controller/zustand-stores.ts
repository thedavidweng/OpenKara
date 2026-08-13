import { useBootstrapStore } from "@/stores/bootstrap-store";
import { useLibraryStore } from "@/stores/library-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";
import { useQueueStore } from "@/stores/queue-store";
import { useRuntimeBootstrapStore } from "@/stores/runtime-bootstrap-store";
import { useSettingsStore } from "@/stores/settings-store";
import type { SettingsControllerStores } from "./types";

export function createZustandSettingsStores(): SettingsControllerStores {
  return {
    preferences: {
      getSnapshot: () => useSettingsStore.getState().getAppSettingsSnapshot(),
      subscribe: (listener) => useSettingsStore.subscribe(listener),
      hydrate: (settings) =>
        useSettingsStore.getState().hydrateAppSettings(settings),
      patch: (patch) => useSettingsStore.getState().patchAppSettings(patch),
      setEqEnabled: (enabled) =>
        useSettingsStore.getState().setEqEnabled(enabled),
      setEqGains: (gainsDb) => useSettingsStore.getState().setEqGains(gainsDb),
      setCrossfadeEnabled: (enabled) =>
        useSettingsStore.getState().setCrossfadeEnabled(enabled),
      setCrossfadeDurationMs: (durationMs) =>
        useSettingsStore.getState().setCrossfadeDurationMs(durationMs),
      setThemePreference: (preference) =>
        useSettingsStore.getState().setThemePreference(preference),
      setUpdatePolicy: (policy) =>
        useSettingsStore.getState().setUpdatePolicy(policy),
    },
    runtimeStatus: {
      getStatus: () => useRuntimeBootstrapStore.getState().status,
      subscribe: (listener) => useRuntimeBootstrapStore.subscribe(listener),
      updateStatus: (status) =>
        useRuntimeBootstrapStore.getState().updateStatus(status),
    },
    modelBootstrap: {
      reload: () => {
        void useBootstrapStore.getState().loadStatus();
      },
    },
    library: {
      getSelectedSongIds: () => useLibraryStore.getState().selectedSongIds,
      clearSelection: () => useLibraryStore.getState().clearSelection(),
      clearAllSeparationStatuses: () =>
        useLibraryStore.getState().clearAllSeparationStatuses(),
      updateSeparationStatus: (status) =>
        useLibraryStore.getState().updateSeparationStatus(status),
      loadLibrary: () => useLibraryStore.getState().loadLibrary(),
    },
    queue: {
      removeSongIds: (songIds) =>
        useQueueStore.getState().removeSongIds(songIds),
    },
    player: {
      loadState: () => usePlayerStore.getState().loadState(),
    },
    lyrics: {
      clear: () => useLyricsStore.getState().clear(),
    },
  };
}
