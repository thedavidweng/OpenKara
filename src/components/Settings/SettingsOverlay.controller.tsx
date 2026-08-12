import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import * as api from "@/lib/tauri";
import { notifyError } from "@/lib/errors";
import { useLibraryStore } from "@/stores/library-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";
import { useQueueStore } from "@/stores/queue-store";
import { useRuntimeBootstrapStore } from "@/stores/runtime-bootstrap-store";
import { useSettingsStore } from "@/stores/settings-store";
import {
  SettingsOverlayContext,
  type SettingsOverlayContextValue,
} from "./SettingsOverlay.context";
import {
  createInitialSettingsOverlaySnapshot,
  createSettingsOverlayActions,
  type SettingsOverlayControllerDependencies,
  type SettingsOverlaySnapshot,
  type SettingsOverlayStateControls,
} from "./SettingsOverlay.state";

interface SettingsOverlayProviderProps {
  children: ReactNode;
  dependencies?: Partial<SettingsOverlayControllerDependencies>;
  initialSnapshot?: SettingsOverlaySnapshot;
  skipInitialize?: boolean;
}

export function SettingsOverlayProvider({
  children,
  dependencies,
  initialSnapshot,
  skipInitialize = false,
}: SettingsOverlayProviderProps) {
  const { i18n } = useTranslation();
  const [snapshot, setSnapshot] = useState<SettingsOverlaySnapshot>(
    initialSnapshot ?? createInitialSettingsOverlaySnapshot(),
  );
  const runtimeBootstrapStatus = useRuntimeBootstrapStore((s) => s.status);
  const didInitializeRef = useRef(false);

  const stateControls = useMemo<SettingsOverlayStateControls>(
    () => ({
      getSnapshot: () => snapshot,
      setSnapshot: (
        updater: (previous: SettingsOverlaySnapshot) => SettingsOverlaySnapshot,
      ) => {
        setSnapshot(updater);
      },
    }),
    [snapshot],
  );

  const defaultDependencies = useMemo<SettingsOverlayControllerDependencies>(
    () => ({
      api,
      notifyError,
      openDirectory: open,
      changeLanguage: (language: string) => i18n.changeLanguage(language),
      libraryStore: {
        clearAllSeparationStatuses:
          useLibraryStore.getState().clearAllSeparationStatuses,
        clearAllUploadStatuses:
          useLibraryStore.getState().clearAllUploadStatuses,
        clearSelection: useLibraryStore.getState().clearSelection,
        loadLibrary: useLibraryStore.getState().loadLibrary,
        updateSeparationStatus:
          useLibraryStore.getState().updateSeparationStatus,
      },
      queueStore: {
        clearQueue: useQueueStore.getState().clearQueue,
        removeSongIds: useQueueStore.getState().removeSongIds,
      },
      playerStore: {
        loadState: usePlayerStore.getState().loadState,
      },
      lyricsStore: {
        clear: useLyricsStore.getState().clear,
      },
      settingsStore: {
        getAppSettingsSnapshot:
          useSettingsStore.getState().getAppSettingsSnapshot,
        hydrateAppSettings: useSettingsStore.getState().hydrateAppSettings,
        patchAppSettings: useSettingsStore.getState().patchAppSettings,
        setEqEnabled: useSettingsStore.getState().setEqEnabled,
        setEqGains: useSettingsStore.getState().setEqGains,
        setCrossfadeEnabled: useSettingsStore.getState().setCrossfadeEnabled,
        setCrossfadeDurationMs:
          useSettingsStore.getState().setCrossfadeDurationMs,
        setThemePreference: useSettingsStore.getState().setThemePreference,
        setUpdatePolicy: useSettingsStore.getState().setUpdatePolicy,
      },
    }),
    [i18n],
  );

  const resolvedDependencies = useMemo(
    () => ({
      ...defaultDependencies,
      ...dependencies,
    }),
    [defaultDependencies, dependencies],
  );

  const actions = useMemo(
    () => createSettingsOverlayActions(resolvedDependencies, stateControls),
    [resolvedDependencies, stateControls],
  );

  useEffect(() => {
    if (skipInitialize || didInitializeRef.current) {
      return;
    }

    didInitializeRef.current = true;
    void actions.initialize();
  }, [actions, skipInitialize]);

  useEffect(() => {
    if (!runtimeBootstrapStatus) {
      return;
    }

    setSnapshot((previous) => ({
      ...previous,
      state: {
        ...previous.state,
        runtimeStatus: {
          state: runtimeBootstrapStatus.state,
          version: runtimeBootstrapStatus.version,
          runtime_path: runtimeBootstrapStatus.runtime_path,
          active_artifact_id: runtimeBootstrapStatus.active_artifact_id,
          target_triple: runtimeBootstrapStatus.target_triple,
          candidate_version: runtimeBootstrapStatus.candidate_version,
          restart_required: runtimeBootstrapStatus.restart_required,
          error: runtimeBootstrapStatus.error?.message ?? null,
          failure_phase: runtimeBootstrapStatus.failure_phase ?? null,
        },
      },
    }));
  }, [runtimeBootstrapStatus]);

  const value = useMemo<SettingsOverlayContextValue>(
    () => ({
      state: snapshot.state,
      meta: snapshot.meta,
      actions,
    }),
    [actions, snapshot.meta, snapshot.state],
  );

  return (
    <SettingsOverlayContext value={value}>{children}</SettingsOverlayContext>
  );
}
