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
import { useBackend } from "@/lib/backend";
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
  const backend = useBackend();
  const loadLibrary = useLibraryStore((s) => s.loadLibrary);
  const loadBootstrapStatus = useBootstrapStore((s) => s.loadStatus);
  const loadPlayerState = usePlayerStore((s) => s.loadState);
  const hydrateAppSettings = useSettingsStore((s) => s.hydrateAppSettings);
  const patchAppSettings = useSettingsStore((s) => s.patchAppSettings);

  useEffect(() => {
    void loadStartupSettings({
      getSettings: backend.settings.getSettings,
      hydrateAppSettings,
      changeLanguage: i18next.changeLanguage,
      detectFallbackLanguage: detectSystemLanguage,
    }).catch((error) => {
      if (!useSettingsStore.getState().hydrated) {
        patchAppSettings({ ...DEFAULT_APP_SETTINGS, hydrated: true });
      }
      notifyError(error);

      void i18next.changeLanguage(detectSystemLanguage()).catch(() => {});
    });
  }, [backend, hydrateAppSettings, patchAppSettings]);

  useEffect(() => {
    backend.librarySetup
      .getLibraryRegistry()
      .then((registry) => setLibraryReady(registry.active_library_id !== null))
      .catch((error) => {
        notifyError(error);
        setLibraryReady(false);
      });
  }, [backend, setLibraryReady]);

  useEffect(() => {
    if (!libraryReady) {
      return;
    }

    void loadLibrary();
    void loadBootstrapStatus();
    void loadPlayerState();
  }, [libraryReady, loadBootstrapStatus, loadLibrary, loadPlayerState]);
}

// Hidden WebViews suspend rAF; timer guarantees the reveal request still fires.
const WINDOW_REVEAL_FALLBACK_MS = 120;

export function useAppReadyRuntime(
  libraryReady: boolean | null,
  settingsHydrated: boolean,
  startupThemeReady: boolean,
  windowShown: boolean,
  setWindowShown: (shown: boolean) => void,
  scheduleFrame: AnimationFrameScheduler = requestAnimationFrame,
) {
  const backend = useBackend();

  useEffect(() => {
    if (
      libraryReady === null ||
      !settingsHydrated ||
      !startupThemeReady ||
      windowShown
    ) {
      return;
    }

    let requested = false;
    const requestReveal = () => {
      if (requested) {
        return;
      }
      requested = true;
      void backend.settings.windowReady();
      setWindowShown(true);
    };

    const frameId = scheduleFrame(requestReveal);
    const timeoutId = setTimeout(requestReveal, WINDOW_REVEAL_FALLBACK_MS);

    return () => {
      cancelAnimationFrame(frameId);
      clearTimeout(timeoutId);
    };
  }, [
    backend,
    libraryReady,
    scheduleFrame,
    setWindowShown,
    settingsHydrated,
    startupThemeReady,
    windowShown,
  ]);
}

export function useAppRuntime(enabled: boolean) {
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
