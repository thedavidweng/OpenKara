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
  LibrarySortMode,
  ModelVariant,
  StemMode,
  ThemePreference,
  UpdatePolicy,
} from "@/types/ipc";

export interface AppSettingsSnapshot {
  hydrated: boolean;
  stemMode: StemMode;
  modelVariant: ModelVariant;
  language: string | null;
  hideBatchSeparate: boolean;
  coverArtBackdrop: boolean;
  hideUpgradeAll: boolean;
  lyricsFontStep: number;
  executionProvider: ExecutionProvider;
  availableExecutionProviders: ExecutionProvider[];
  compatibleExecutionProviders: ExecutionProvider[];
  eqEnabled: boolean;
  eqGainsDb: [number, number, number, number, number];
  crossfadeEnabled: boolean;
  crossfadeDurationMs: number;
  librarySortMode: LibrarySortMode;
  themePreference: ThemePreference;
  updatePolicy: UpdatePolicy;
}

interface SettingsState {
  isOpen: boolean;
  hydrated: AppSettingsSnapshot["hydrated"];
  stemMode: AppSettingsSnapshot["stemMode"];
  modelVariant: AppSettingsSnapshot["modelVariant"];
  language: AppSettingsSnapshot["language"];
  hideBatchSeparate: AppSettingsSnapshot["hideBatchSeparate"];
  coverArtBackdrop: AppSettingsSnapshot["coverArtBackdrop"];
  hideUpgradeAll: AppSettingsSnapshot["hideUpgradeAll"];
  lyricsFontStep: AppSettingsSnapshot["lyricsFontStep"];
  executionProvider: AppSettingsSnapshot["executionProvider"];
  availableExecutionProviders: AppSettingsSnapshot["availableExecutionProviders"];
  compatibleExecutionProviders: AppSettingsSnapshot["compatibleExecutionProviders"];
  eqEnabled: AppSettingsSnapshot["eqEnabled"];
  eqGainsDb: AppSettingsSnapshot["eqGainsDb"];
  crossfadeEnabled: AppSettingsSnapshot["crossfadeEnabled"];
  crossfadeDurationMs: AppSettingsSnapshot["crossfadeDurationMs"];
  librarySortMode: AppSettingsSnapshot["librarySortMode"];
  themePreference: AppSettingsSnapshot["themePreference"];
  updatePolicy: AppSettingsSnapshot["updatePolicy"];
  themePreferenceMutationGeneration: number;
  updatePolicyMutationGeneration: number;
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
  setLibrarySortMode: (mode: LibrarySortMode) => Promise<void>;
  setThemePreference: (preference: ThemePreference) => Promise<void>;
  setUpdatePolicy: (policy: UpdatePolicy) => Promise<void>;
  getAppSettingsSnapshot: () => AppSettingsSnapshot;
}

export const DEFAULT_APP_SETTINGS: AppSettingsSnapshot = {
  hydrated: false,
  stemMode: "four_stem",
  modelVariant: "htdemucs",
  language: null,
  hideBatchSeparate: false,
  coverArtBackdrop: true,
  hideUpgradeAll: false,
  lyricsFontStep: 0,
  executionProvider: "cpu",
  availableExecutionProviders: ["cpu"],
  compatibleExecutionProviders: ["cpu"],
  eqEnabled: false,
  eqGainsDb: [0, 0, 0, 0, 0],
  crossfadeEnabled: false,
  crossfadeDurationMs: 3_000,
  librarySortMode: "recently_imported",
  themePreference: "dark",
  updatePolicy: "notify",
};

function toAppSettingsSnapshot(settings: AppSettings): AppSettingsSnapshot {
  return {
    hydrated: true,
    stemMode: settings.stem_mode,
    modelVariant: settings.model_variant,
    language: settings.language,
    hideBatchSeparate: settings.hide_batch_separate,
    coverArtBackdrop: settings.cover_art_backdrop,
    hideUpgradeAll: settings.hide_upgrade_all,
    lyricsFontStep: settings.lyrics_font_step,
    executionProvider: settings.execution_provider,
    availableExecutionProviders: settings.available_execution_providers,
    compatibleExecutionProviders: settings.compatible_execution_providers,
    eqEnabled: settings.eq_enabled ?? false,
    eqGainsDb: settings.eq_gains_db ?? [0, 0, 0, 0, 0],
    crossfadeEnabled: settings.crossfade_enabled ?? false,
    crossfadeDurationMs: settings.crossfade_duration_ms ?? 3_000,
    librarySortMode: settings.library_sort_mode,
    themePreference: settings.theme_preference,
    updatePolicy: settings.update_policy ?? "notify",
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
    hideUpgradeAll: state.hideUpgradeAll,
    lyricsFontStep: state.lyricsFontStep,
    executionProvider: state.executionProvider,
    availableExecutionProviders: state.availableExecutionProviders,
    compatibleExecutionProviders: state.compatibleExecutionProviders,
    eqEnabled: state.eqEnabled,
    eqGainsDb: state.eqGainsDb,
    crossfadeEnabled: state.crossfadeEnabled,
    crossfadeDurationMs: state.crossfadeDurationMs,
    librarySortMode: state.librarySortMode,
    themePreference: state.themePreference,
    updatePolicy: state.updatePolicy,
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
    hideUpgradeAll: snapshot.hideUpgradeAll,
    lyricsFontStep: snapshot.lyricsFontStep,
    executionProvider: snapshot.executionProvider,
    availableExecutionProviders: snapshot.availableExecutionProviders,
    compatibleExecutionProviders: snapshot.compatibleExecutionProviders,
    eqEnabled: snapshot.eqEnabled,
    eqGainsDb: snapshot.eqGainsDb,
    crossfadeEnabled: snapshot.crossfadeEnabled,
    crossfadeDurationMs: snapshot.crossfadeDurationMs,
    librarySortMode: snapshot.librarySortMode,
    themePreference: snapshot.themePreference,
    updatePolicy: snapshot.updatePolicy,
  });
}

interface OptimisticField<T> {
  begin: (currentValue: T, nextValue: T) => number;
  confirm: (generation: number, value: T) => T;
  reject: (generation: number) => { value: T; shouldNotify: boolean };
  reconcileSnapshot: (value: T) => T;
}

function createOptimisticField<T>(initialValue: T): OptimisticField<T> {
  let latestGeneration = 0;
  let confirmedGeneration = 0;
  let confirmedValue = initialValue;
  const pending = new Map<number, T>();

  const visibleValue = () => {
    let latestPendingGeneration = confirmedGeneration;
    let value = confirmedValue;

    for (const [generation, pendingValue] of pending) {
      if (generation > latestPendingGeneration) {
        latestPendingGeneration = generation;
        value = pendingValue;
      }
    }

    return value;
  };

  return {
    begin: (currentValue, nextValue) => {
      if (pending.size === 0) {
        confirmedGeneration = latestGeneration;
        confirmedValue = currentValue;
      }

      const generation = ++latestGeneration;
      pending.set(generation, nextValue);
      return generation;
    },
    confirm: (generation, value) => {
      pending.delete(generation);

      if (generation > confirmedGeneration) {
        confirmedGeneration = generation;
        confirmedValue = value;

        for (const pendingGeneration of pending.keys()) {
          if (pendingGeneration <= generation) {
            pending.delete(pendingGeneration);
          }
        }
      }

      return visibleValue();
    },
    reject: (generation) => {
      pending.delete(generation);
      return {
        value: visibleValue(),
        shouldNotify: generation === latestGeneration,
      };
    },
    reconcileSnapshot: (value) => {
      confirmedValue = value;
      if (pending.size === 0) {
        confirmedGeneration = latestGeneration;
      }
      return visibleValue();
    },
  };
}

export function createSettingsStore(
  syncChannel: WebviewSyncChannel<SettingsSyncSnapshot> = createWebviewSyncChannel<SettingsSyncSnapshot>(
    "openkara.settings",
  ),
) {
  const eqEnabledField = createOptimisticField(DEFAULT_APP_SETTINGS.eqEnabled);
  const eqGainsField = createOptimisticField(DEFAULT_APP_SETTINGS.eqGainsDb);
  const crossfadeEnabledField = createOptimisticField(
    DEFAULT_APP_SETTINGS.crossfadeEnabled,
  );
  const crossfadeDurationField = createOptimisticField(
    DEFAULT_APP_SETTINGS.crossfadeDurationMs,
  );
  const librarySortModeField = createOptimisticField(
    DEFAULT_APP_SETTINGS.librarySortMode,
  );

  const store = create<SettingsState>((set, get) => {
    const syncPatch = (patch: Partial<SettingsState>) => {
      set(patch);
      syncChannel.publish(toSettingsSyncSnapshot(get()));
    };

    const syncAppSettings = (settings: AppSettings) => {
      const snapshot = toAppSettingsSnapshot(settings);
      snapshot.eqEnabled = eqEnabledField.reconcileSnapshot(snapshot.eqEnabled);
      snapshot.eqGainsDb = eqGainsField.reconcileSnapshot(snapshot.eqGainsDb);
      snapshot.crossfadeEnabled = crossfadeEnabledField.reconcileSnapshot(
        snapshot.crossfadeEnabled,
      );
      snapshot.crossfadeDurationMs = crossfadeDurationField.reconcileSnapshot(
        snapshot.crossfadeDurationMs,
      );
      snapshot.librarySortMode = librarySortModeField.reconcileSnapshot(
        snapshot.librarySortMode,
      );
      syncPatch(snapshot);
    };

    return {
      isOpen: false,
      ...DEFAULT_APP_SETTINGS,
      themePreferenceMutationGeneration: 0,
      updatePolicyMutationGeneration: 0,
      toggle: () => syncPatch({ isOpen: !get().isOpen }),
      close: () => syncPatch({ isOpen: false }),
      open: () => syncPatch({ isOpen: true }),
      hydrateAppSettings: (settings) => syncAppSettings(settings),
      patchAppSettings: (patch) => syncPatch(patch),
      setLyricsFontStep: async (step) => {
        try {
          const settings = await api.setLyricsFontStep(step);
          syncAppSettings(settings);
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
        const generation = eqEnabledField.begin(get().eqEnabled, enabled);
        syncPatch({ eqEnabled: enabled });
        try {
          const settings = await api.setEqEnabled(enabled);
          syncPatch({
            eqEnabled: eqEnabledField.confirm(
              generation,
              settings.eq_enabled ?? false,
            ),
          });
        } catch (error) {
          const result = eqEnabledField.reject(generation);
          syncPatch({ eqEnabled: result.value });
          if (result.shouldNotify) {
            notifyError(error);
          }
        }
      },
      setEqGains: async (gainsDb) => {
        const generation = eqGainsField.begin(get().eqGainsDb, gainsDb);
        syncPatch({ eqGainsDb: gainsDb });
        try {
          const settings = await api.setEqGains(gainsDb);
          syncPatch({
            eqGainsDb: eqGainsField.confirm(
              generation,
              settings.eq_gains_db ?? [0, 0, 0, 0, 0],
            ),
          });
        } catch (error) {
          const result = eqGainsField.reject(generation);
          syncPatch({ eqGainsDb: result.value });
          if (result.shouldNotify) {
            notifyError(error);
          }
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
        const generation = crossfadeEnabledField.begin(
          get().crossfadeEnabled,
          enabled,
        );
        syncPatch({ crossfadeEnabled: enabled });
        try {
          const settings = await api.setCrossfadeEnabled(enabled);
          syncPatch({
            crossfadeEnabled: crossfadeEnabledField.confirm(
              generation,
              settings.crossfade_enabled ?? false,
            ),
          });
        } catch (error) {
          const result = crossfadeEnabledField.reject(generation);
          syncPatch({ crossfadeEnabled: result.value });
          if (result.shouldNotify) {
            notifyError(error);
          }
        }
      },
      setCrossfadeDurationMs: async (durationMs) => {
        const clamped = Math.max(500, Math.min(10_000, Math.round(durationMs)));
        const generation = crossfadeDurationField.begin(
          get().crossfadeDurationMs,
          clamped,
        );
        syncPatch({ crossfadeDurationMs: clamped });
        try {
          const settings = await api.setCrossfadeDurationMs(clamped);
          syncPatch({
            crossfadeDurationMs: crossfadeDurationField.confirm(
              generation,
              settings.crossfade_duration_ms ?? 3_000,
            ),
          });
        } catch (error) {
          const result = crossfadeDurationField.reject(generation);
          syncPatch({ crossfadeDurationMs: result.value });
          if (result.shouldNotify) {
            notifyError(error);
          }
        }
      },
      setLibrarySortMode: async (mode) => {
        const generation = librarySortModeField.begin(
          get().librarySortMode,
          mode,
        );
        syncPatch({ librarySortMode: mode });
        try {
          const settings = await api.setLibrarySortMode(mode);
          syncPatch({
            librarySortMode: librarySortModeField.confirm(
              generation,
              settings.library_sort_mode,
            ),
          });
        } catch (error) {
          const result = librarySortModeField.reject(generation);
          syncPatch({ librarySortMode: result.value });
          if (result.shouldNotify) {
            notifyError(error);
          }
        }
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
            syncAppSettings(settings);
          }
        } catch (error) {
          if (get().themePreferenceMutationGeneration === generation) {
            syncPatch({
              themePreference: previousSnapshot.themePreference,
              themePreferenceMutationGeneration: generation,
            });
            notifyError(error);
          }
        }
      },
      setUpdatePolicy: async (policy) => {
        if (get().updatePolicy === policy) {
          return;
        }

        const generation = get().updatePolicyMutationGeneration + 1;
        const previousSnapshot = selectAppSettingsSnapshot(get());

        syncPatch({
          updatePolicy: policy,
          updatePolicyMutationGeneration: generation,
        });

        try {
          const settings = await api.setUpdatePolicy(policy);
          if (get().updatePolicyMutationGeneration === generation) {
            syncAppSettings(settings);
          }
        } catch (error) {
          if (get().updatePolicyMutationGeneration === generation) {
            syncPatch({
              updatePolicy: previousSnapshot.updatePolicy,
              updatePolicyMutationGeneration: generation,
            });
            notifyError(error);
          }
        }
      },
      getAppSettingsSnapshot: () => selectAppSettingsSnapshot(get()),
    };
  });

  const unsubscribe = syncChannel.subscribe((snapshot) => {
    applySettingsSyncSnapshot(store.setState, {
      ...snapshot,
      eqEnabled: eqEnabledField.reconcileSnapshot(snapshot.eqEnabled),
      eqGainsDb: eqGainsField.reconcileSnapshot(snapshot.eqGainsDb),
      crossfadeEnabled: crossfadeEnabledField.reconcileSnapshot(
        snapshot.crossfadeEnabled,
      ),
      crossfadeDurationMs: crossfadeDurationField.reconcileSnapshot(
        snapshot.crossfadeDurationMs,
      ),
      librarySortMode: librarySortModeField.reconcileSnapshot(
        snapshot.librarySortMode,
      ),
    });
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
