import { create } from "zustand";
import { notifyError } from "@/lib/errors";
import {
  createWebviewSyncChannel,
  type WebviewSyncChannel,
} from "@/runtime/webview-sync";
import * as api from "@/lib/tauri";
import type {
  AppSettings,
  ExecutionProvider,
  ModelVariant,
  StemMode,
} from "@/types/ipc";

export interface AppSettingsSnapshot {
  hydrated: boolean;
  stemMode: StemMode;
  modelVariant: ModelVariant;
  language: string | null;
  hideBatchSeparate: boolean;
  coverArtBackdrop: boolean;
  lyricsFontStep: number;
  executionProvider: ExecutionProvider;
  availableExecutionProviders: ExecutionProvider[];
  eqEnabled: boolean;
  eqGainsDb: [number, number, number, number, number];
  crossfadeEnabled: boolean;
  crossfadeDurationMs: number;
}

interface SettingsState {
  isOpen: boolean;
  hydrated: AppSettingsSnapshot["hydrated"];
  stemMode: AppSettingsSnapshot["stemMode"];
  modelVariant: AppSettingsSnapshot["modelVariant"];
  language: AppSettingsSnapshot["language"];
  hideBatchSeparate: AppSettingsSnapshot["hideBatchSeparate"];
  coverArtBackdrop: AppSettingsSnapshot["coverArtBackdrop"];
  lyricsFontStep: AppSettingsSnapshot["lyricsFontStep"];
  executionProvider: AppSettingsSnapshot["executionProvider"];
  availableExecutionProviders: AppSettingsSnapshot["availableExecutionProviders"];
  eqEnabled: AppSettingsSnapshot["eqEnabled"];
  eqGainsDb: AppSettingsSnapshot["eqGainsDb"];
  crossfadeEnabled: AppSettingsSnapshot["crossfadeEnabled"];
  crossfadeDurationMs: AppSettingsSnapshot["crossfadeDurationMs"];
  toggle: () => void;
  close: () => void;
  open: () => void;
  hydrateAppSettings: (settings: AppSettings) => void;
  patchAppSettings: (patch: Partial<AppSettingsSnapshot>) => void;
  setLyricsFontStep: (step: number) => Promise<void>;
  adjustLyricsFontStep: (delta: number) => Promise<void>;
  resetLyricsFontStep: () => Promise<void>;
  setEqEnabled: (enabled: boolean) => Promise<void>;
  setEqGains: (
    gainsDb: [number, number, number, number, number],
  ) => Promise<void>;
  setEqBandGain: (band: number, gainDb: number) => Promise<void>;
  resetEqGains: () => Promise<void>;
  setCrossfadeEnabled: (enabled: boolean) => Promise<void>;
  setCrossfadeDurationMs: (durationMs: number) => Promise<void>;
  getAppSettingsSnapshot: () => AppSettingsSnapshot;
}

const DEFAULT_APP_SETTINGS: AppSettingsSnapshot = {
  hydrated: false,
  stemMode: "two_stem",
  modelVariant: "htdemucs",
  language: null,
  hideBatchSeparate: false,
  coverArtBackdrop: true,
  lyricsFontStep: 0,
  executionProvider: "cpu",
  availableExecutionProviders: ["cpu"],
  eqEnabled: false,
  eqGainsDb: [0, 0, 0, 0, 0],
  crossfadeEnabled: false,
  crossfadeDurationMs: 3000,
};

function toAppSettingsSnapshot(settings: AppSettings): AppSettingsSnapshot {
  return {
    hydrated: true,
    stemMode: settings.stem_mode,
    modelVariant: settings.model_variant,
    language: settings.language,
    hideBatchSeparate: settings.hide_batch_separate,
    coverArtBackdrop: settings.cover_art_backdrop,
    lyricsFontStep: settings.lyrics_font_step,
    executionProvider: settings.execution_provider,
    availableExecutionProviders: settings.available_execution_providers,
    // Defensive defaults: incomplete IPC payloads must not leave eqGainsDb undefined.
    eqEnabled: settings.eq_enabled ?? false,
    eqGainsDb: settings.eq_gains_db ?? [0, 0, 0, 0, 0],
    crossfadeEnabled: settings.crossfade_enabled ?? false,
    crossfadeDurationMs: settings.crossfade_duration_ms ?? 3000,
  };
}

function selectAppSettingsSnapshot(
  state: AppSettingsSnapshot,
): AppSettingsSnapshot {
  return {
    hydrated: state.hydrated,
    stemMode: state.stemMode,
    modelVariant: state.modelVariant,
    language: state.language,
    hideBatchSeparate: state.hideBatchSeparate,
    coverArtBackdrop: state.coverArtBackdrop,
    lyricsFontStep: state.lyricsFontStep,
    executionProvider: state.executionProvider,
    availableExecutionProviders: state.availableExecutionProviders,
    eqEnabled: state.eqEnabled,
    eqGainsDb: state.eqGainsDb,
    crossfadeEnabled: state.crossfadeEnabled,
    crossfadeDurationMs: state.crossfadeDurationMs,
  };
}

export interface SettingsSyncSnapshot extends AppSettingsSnapshot {
  isOpen: boolean;
}

function toSettingsSyncSnapshot(state: SettingsState): SettingsSyncSnapshot {
  return {
    isOpen: state.isOpen,
    ...selectAppSettingsSnapshot(state),
  };
}

function applySettingsSyncSnapshot(
  set: (partial: Partial<SettingsState>) => void,
  snapshot: SettingsSyncSnapshot,
) {
  set({
    isOpen: snapshot.isOpen,
    hydrated: snapshot.hydrated,
    stemMode: snapshot.stemMode,
    modelVariant: snapshot.modelVariant,
    language: snapshot.language,
    hideBatchSeparate: snapshot.hideBatchSeparate,
    coverArtBackdrop: snapshot.coverArtBackdrop,
    lyricsFontStep: snapshot.lyricsFontStep,
    executionProvider: snapshot.executionProvider,
    availableExecutionProviders: snapshot.availableExecutionProviders,
    eqEnabled: snapshot.eqEnabled,
    eqGainsDb: snapshot.eqGainsDb,
    crossfadeEnabled: snapshot.crossfadeEnabled,
    crossfadeDurationMs: snapshot.crossfadeDurationMs,
  });
}

export function createSettingsStore(
  syncChannel: WebviewSyncChannel<SettingsSyncSnapshot> = createWebviewSyncChannel<SettingsSyncSnapshot>(
    "openkara.settings",
  ),
) {
  const store = create<SettingsState>((set, get) => {
    const syncPatch = (patch: Partial<SettingsState>) => {
      set(patch);
      syncChannel.publish(toSettingsSyncSnapshot(get()));
    };

    return {
      isOpen: false,
      ...DEFAULT_APP_SETTINGS,
      toggle: () => syncPatch({ isOpen: !get().isOpen }),
      close: () => syncPatch({ isOpen: false }),
      open: () => syncPatch({ isOpen: true }),
      hydrateAppSettings: (settings) =>
        syncPatch(toAppSettingsSnapshot(settings)),
      patchAppSettings: (patch) => syncPatch(patch),
      setLyricsFontStep: async (step) => {
        try {
          const settings = await api.setLyricsFontStep(step);
          syncPatch(toAppSettingsSnapshot(settings));
        } catch (error) {
          notifyError(error);
        }
      },
      adjustLyricsFontStep: async (delta) => {
        const current = get().lyricsFontStep;
        const nextStep = Math.max(-2, Math.min(2, current + delta));
        if (nextStep === current) {
          return;
        }
        await get().setLyricsFontStep(nextStep);
      },
      resetLyricsFontStep: async () => {
        if (get().lyricsFontStep === 0) {
          return;
        }
        await get().setLyricsFontStep(0);
      },
      setEqEnabled: async (enabled) => {
        // Capture the authoritative value before the optimistic patch so we
        // revert to it (not the inverse of the requested value) on failure.
        // Reverting to !enabled would flip the store even when the backend
        // state was already the requested value.
        const previous = get().eqEnabled;
        // Optimistically update local state so the toggle reflects immediately.
        syncPatch({ eqEnabled: enabled });
        try {
          const settings = await api.setEqEnabled(enabled);
          syncPatch(toAppSettingsSnapshot(settings));
        } catch (error) {
          // Revert to the previous authoritative value on failure.
          syncPatch({ eqEnabled: previous });
          notifyError(error);
        }
      },
      setEqGains: async (gainsDb) => {
        // Optimistically update local state so sliders reflect immediately.
        const previous = get().eqGainsDb;
        syncPatch({ eqGainsDb: gainsDb });
        try {
          const settings = await api.setEqGains(gainsDb);
          syncPatch(toAppSettingsSnapshot(settings));
        } catch (error) {
          // Revert to the previous authoritative values on failure.
          syncPatch({ eqGainsDb: previous });
          notifyError(error);
        }
      },
      setEqBandGain: async (band, gainDb) => {
        const clamped = Math.max(-12, Math.min(12, gainDb));
        const current = get().eqGainsDb;
        if (current[band] === clamped) {
          return;
        }
        const next = [...current] as [number, number, number, number, number];
        next[band] = clamped;
        await get().setEqGains(next);
      },
      resetEqGains: async () => {
        await get().setEqGains([0, 0, 0, 0, 0]);
      },
      setCrossfadeEnabled: async (enabled) => {
        const previous = get().crossfadeEnabled;
        if (previous === enabled) {
          return;
        }
        syncPatch({ crossfadeEnabled: enabled });
        try {
          const settings = await api.setCrossfadeEnabled(enabled);
          // Only apply the response if no newer toggle has superseded this
          // request — otherwise a stale late-arriving response would revert
          // the store to an older enabled state.
          if (get().crossfadeEnabled === enabled) {
            syncPatch(toAppSettingsSnapshot(settings));
          }
        } catch (error) {
          if (get().crossfadeEnabled === enabled) {
            syncPatch({ crossfadeEnabled: previous });
          }
          notifyError(error);
        }
      },
      setCrossfadeDurationMs: async (durationMs) => {
        const previous = get().crossfadeDurationMs;
        if (previous === durationMs) {
          return;
        }
        syncPatch({ crossfadeDurationMs: durationMs });
        try {
          const settings = await api.setCrossfadeDurationMs(durationMs);
          // Only apply the response if no newer save has superseded this
          // request — otherwise a stale late-arriving response would revert
          // the store to an older duration.
          if (get().crossfadeDurationMs === durationMs) {
            syncPatch(toAppSettingsSnapshot(settings));
          }
        } catch (error) {
          if (get().crossfadeDurationMs === durationMs) {
            syncPatch({ crossfadeDurationMs: previous });
          }
          notifyError(error);
        }
      },
      getAppSettingsSnapshot: () => selectAppSettingsSnapshot(get()),
    };
  });

  const unsubscribe = syncChannel.subscribe((snapshot) => {
    applySettingsSyncSnapshot(store.setState, snapshot);
  });

  return {
    store,
    dispose() {
      unsubscribe();
      syncChannel.close();
    },
  };
}

const defaultSettingsStore = createSettingsStore();

export const useSettingsStore = defaultSettingsStore.store;
