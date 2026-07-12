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
  eqGainsDb: EqGains;
}

/** Fixed 5-band EQ gains in dB, range [-12, 12]. */
export type EqGains = [number, number, number, number, number];

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
  toggle: () => void;
  close: () => void;
  open: () => void;
  hydrateAppSettings: (settings: AppSettings) => void;
  patchAppSettings: (patch: Partial<AppSettingsSnapshot>) => void;
  setLyricsFontStep: (step: number) => Promise<void>;
  adjustLyricsFontStep: (delta: number) => Promise<void>;
  resetLyricsFontStep: () => Promise<void>;
  setEqEnabled: (enabled: boolean) => Promise<void>;
  setEqGains: (gainsDb: EqGains) => Promise<void>;
  resetEqGains: () => Promise<void>;
  getAppSettingsSnapshot: () => AppSettingsSnapshot;
}

const DEFAULT_EQ_GAINS: EqGains = [0, 0, 0, 0, 0];

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
  eqGainsDb: DEFAULT_EQ_GAINS,
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
    eqEnabled: settings.eq_enabled,
    eqGainsDb: settings.eq_gains_db,
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
    lyricsFontStep: snapshot.lyricsFontStep,
    executionProvider: snapshot.executionProvider,
    availableExecutionProviders: snapshot.availableExecutionProviders,
    eqEnabled: snapshot.eqEnabled,
    eqGainsDb: snapshot.eqGainsDb,
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
        const previous = get().eqEnabled;
        if (previous === enabled) {
          return;
        }
        syncPatch({ eqEnabled: enabled });
        try {
          const settings = await api.setEqEnabled(enabled);
          syncPatch(toAppSettingsSnapshot(settings));
        } catch (error) {
          // Restore the latest authoritative store values on failure.
          syncPatch({ eqEnabled: previous });
          notifyError(error);
        }
      },
      setEqGains: async (gainsDb) => {
        const previous = get().eqGainsDb;
        syncPatch({ eqGainsDb: gainsDb });
        try {
          const settings = await api.setEqGains(gainsDb);
          syncPatch(toAppSettingsSnapshot(settings));
        } catch (error) {
          syncPatch({ eqGainsDb: previous });
          notifyError(error);
        }
      },
      resetEqGains: async () => {
        await get().setEqGains(DEFAULT_EQ_GAINS);
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
