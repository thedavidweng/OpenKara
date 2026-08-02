import { useEffect } from "react";
import { usePlayerStore } from "@/stores/player-store";
import { useSettingsStore } from "@/stores/settings-store";
import { notifyError } from "@/lib/errors";
import * as api from "@/lib/tauri";
import i18next, { detectSystemLanguage } from "@/lib/i18n";
import {
  usePlaybackEvents,
  usePlaybackPositionSubscription,
} from "@/runtime/events/playback-events";
import { useBootstrapEvents } from "@/runtime/events/bootstrap-events";
import {
  useBatchSeparationEvents,
  useSeparationEvents,
} from "@/runtime/events/separation-events";
import { useRemotePlaybackEvents } from "@/runtime/events/remote-events";
import { useUploadEvents } from "@/runtime/events/upload-events";
import {
  tauriRuntimeEventSource,
  type RuntimeEventSource,
} from "@/runtime/event-source";
import { usePreloadCandidateEffect } from "@/runtime/playback-effects";
import { loadStartupSettings } from "@/runtime/settings-runtime";

export { useLyricsAutoFetch } from "@/runtime/lyrics-effects";
export { usePlaybackPositionSubscription } from "@/runtime/events/playback-events";

export function useEventListeners(
  enabled = true,
  source: RuntimeEventSource = tauriRuntimeEventSource,
) {
  usePlaybackEvents(enabled, source);
  useBootstrapEvents(enabled, source);
  useSeparationEvents(enabled, source);
  useBatchSeparationEvents(enabled, source);
  useUploadEvents(enabled, source);
  useRemotePlaybackEvents(enabled, source);
  usePreloadCandidateEffect(enabled);
}

export function useFullscreenPlaybackRuntime(
  source: RuntimeEventSource = tauriRuntimeEventSource,
) {
  const applyPlaybackPositionEvent = usePlayerStore(
    (state) => state.applyPlaybackPositionEvent,
  );
  const updateSnapshot = usePlayerStore((state) => state.updateSnapshot);
  const hydrateAppSettings = useSettingsStore(
    (state) => state.hydrateAppSettings,
  );

  useEffect(() => {
    void api
      .getPlaybackState()
      .then((snapshot) => updateSnapshot(snapshot))
      .catch(notifyError);

    void loadStartupSettings({
      getSettings: api.getSettings,
      hydrateAppSettings,
      changeLanguage: i18next.changeLanguage,
      detectFallbackLanguage: detectSystemLanguage,
    }).catch(notifyError);
  }, [hydrateAppSettings, updateSnapshot]);

  usePlaybackPositionSubscription(true, applyPlaybackPositionEvent, source);
}
