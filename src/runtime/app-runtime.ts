import { useEffect } from "react";
import { usePlayerStore } from "@/stores/player-store";
import { useLibraryStore } from "@/stores/library-store";
import { useBootstrapStore } from "@/stores/bootstrap-store";
import {
  DEFAULT_APP_SETTINGS,
  useSettingsStore,
} from "@/stores/settings-store";
import {
  useEventListeners,
  useLyricsAutoFetch,
} from "@/hooks/use-playback-runtime";
import { useCdgSync } from "@/hooks/use-cdg-sync";
import { useKeyboardShortcuts } from "@/hooks/use-keyboard-shortcuts";
import { useFileDrop } from "@/hooks/use-file-drop";
import { notifyError } from "@/lib/errors";
import * as api from "@/lib/tauri";
import i18next, { detectSystemLanguage } from "@/lib/i18n";
import {
  useAirPlayAudienceSync,
  useLocalAudienceOutputState,
  useAirPlayOutputState,
} from "@/runtime/airplay-runtime";
import { useLocalAudienceRomanizeRuntime } from "@/runtime/local-audience-romanize-runtime";
import { useAppMenuRuntime } from "./menu-runtime";
import { loadStartupSettings } from "./settings-runtime";

type AnimationFrameScheduler = (callback: FrameRequestCallback) => number;

export function useAppStartupRuntime(
  libraryReady: boolean | null,
  setLibraryReady: (ready: boolean) => void,
) {
  const loadLibrary = useLibraryStore((s) => s.loadLibrary);
  const loadBootstrapStatus = useBootstrapStore((s) => s.loadStatus);
  const loadPlayerState = usePlayerStore((s) => s.loadState);
  const hydrateAppSettings = useSettingsStore((s) => s.hydrateAppSettings);
  const patchAppSettings = useSettingsStore((s) => s.patchAppSettings);

  useEffect(() => {
    void loadStartupSettings({
      getSettings: api.getSettings,
      hydrateAppSettings,
      changeLanguage: i18next.changeLanguage,
      detectFallbackLanguage: detectSystemLanguage,
    }).catch((error) => {
      // Missing/corrupt/unreadable config must not strand the hidden window.
      // Apply all defaults with hydrated: true so the theme runtime and ready
      // gate can proceed, then report the original settings error once.
      // Guard against wiping already-loaded preferences: loadStartupSettings
      // hydrates saved settings before attempting the language switch, so a
      // language failure reaching this catch must not overwrite them.
      if (!useSettingsStore.getState().hydrated) {
        patchAppSettings({ ...DEFAULT_APP_SETTINGS, hydrated: true });
      }
      notifyError(error);

      // Fallback system-language setup is independent of settings hydration so
      // a language failure cannot clear the selected theme.
      void i18next.changeLanguage(detectSystemLanguage()).catch(() => {
        // Language failure after settings fallback is non-fatal; the default
        // language remains active and the window can still be shown.
      });
    });
  }, [hydrateAppSettings, patchAppSettings]);

  useEffect(() => {
    api
      .getLibraryRegistry()
      .then((registry) => setLibraryReady(registry.active_library_id !== null))
      .catch((error) => {
        notifyError(error);
        setLibraryReady(false);
      });
  }, [setLibraryReady]);

  useEffect(() => {
    if (!libraryReady) {
      return;
    }

    void loadLibrary();
    void loadBootstrapStatus();
    void loadPlayerState();
  }, [libraryReady, loadBootstrapStatus, loadLibrary, loadPlayerState]);
}

export function useAppReadyRuntime(
  libraryReady: boolean | null,
  settingsHydrated: boolean,
  startupThemeReady: boolean,
  windowShown: boolean,
  setWindowShown: (shown: boolean) => void,
  scheduleFrame: AnimationFrameScheduler = requestAnimationFrame,
) {
  useEffect(() => {
    if (
      libraryReady === null ||
      !settingsHydrated ||
      !startupThemeReady ||
      windowShown
    ) {
      return;
    }

    const frameId = scheduleFrame(() => {
      void api.windowReady();
      setWindowShown(true);
    });

    return () => {
      cancelAnimationFrame(frameId);
    };
  }, [
    libraryReady,
    scheduleFrame,
    setWindowShown,
    settingsHydrated,
    startupThemeReady,
    windowShown,
  ]);
}

/**
 * Single product runtime graph (library-ready gated). There is no second
 * webview/sidebar runtime path.
 */
export function useAppRuntime(enabled: boolean) {
  // Keep side effects behind the library-ready gate so first-run setup does
  // not accidentally enable file-drop imports or playback listeners before a
  // library exists.
  useEventListeners(enabled);
  useLyricsAutoFetch(enabled);
  useCdgSync(enabled);
  useAirPlayAudienceSync(enabled);
  useLocalAudienceOutputState(enabled);
  useAirPlayOutputState(enabled);
  useLocalAudienceRomanizeRuntime(enabled);
  useKeyboardShortcuts(enabled);
  useFileDrop(enabled);
  useAppMenuRuntime(enabled);
}
