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
  ThemePreference,
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
  themePreference: ThemePreference;
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
  themePreference: AppSettingsSnapshot["themePreference"];
  /** Monotonic generation for optimistic theme-preference mutations. */
  themePreferenceMutationGeneration: number;
  toggle: () => void;
  close: () => void;
  open: () => void;
  hydrateAppSettings: (settings: AppSettings) => void;
  patchAppSettings: (patch: Partial<AppSettingsSnapshot>) => void;
  setLyricsFontStep: (step: number) => Promise<void>;
  adjustLyricsFontStep: (delta: number) => Promise<void>;
  resetLyricsFontStep: () => Promise<void>;
  setThemePreference: (preference: ThemePreference) => Promise<void>;
  getAppSettingsSnapshot: () => AppSettingsSnapshot;
}

export const DEFAULT_APP_SETTINGS: AppSettingsSnapshot = {
  hydrated: false,
  stemMode: "two_stem",
  modelVariant: "htdemucs",
  language: null,
  hideBatchSeparate: false,
  coverArtBackdrop: true,
  lyricsFontStep: 0,
  executionProvider: "cpu",
  availableExecutionProviders: ["cpu"],
  themePreference: "dark",
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
    themePreference: settings.theme_preference,
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
    themePreference: state.themePreference,
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
    themePreference: snapshot.themePreference,
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
      themePreferenceMutationGeneration: 0,
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
      setThemePreference: async (preference) => {
        if (get().themePreference === preference) {
          return;
        }

        const generation = get().themePreferenceMutationGeneration + 1;
        const previousSnapshot = selectAppSettingsSnapshot(get());

        syncPatch({
          themePreference: preference,
          themePreferenceMutationGeneration: generation,
        });

        try {
          const settings = await api.setThemePreference(preference);
          if (get().themePreferenceMutationGeneration === generation) {
            syncPatch(toAppSettingsSnapshot(settings));
          }
        } catch (error) {
          // Only roll back the theme preference that failed. Restoring the full
          // pre-attempt snapshot would wipe concurrent settings edits that
          // landed while the IPC call was in flight.
          if (get().themePreferenceMutationGeneration === generation) {
            syncPatch({
              themePreference: previousSnapshot.themePreference,
              themePreferenceMutationGeneration: generation,
            });
            notifyError(error);
          }
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
