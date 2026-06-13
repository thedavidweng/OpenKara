import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { useEventSubscriptions } from "./use-event-subscription";
import { usePlayerStore } from "@/stores/player-store";
import { useLibraryStore } from "@/stores/library-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import { useBootstrapStore } from "@/stores/bootstrap-store";
import { useRuntimeBootstrapStore } from "@/stores/runtime-bootstrap-store";
import { useSettingsStore } from "@/stores/settings-store";
import { notifyError } from "@/lib/errors";
import i18next, { detectSystemLanguage } from "@/lib/i18n";
import * as api from "@/lib/tauri";
import {
  createBatchSeparationClearScheduler,
  createStatusClearScheduler,
  separationErrorStatus,
  separationProgressStatus,
  uploadCompleteStatus,
  uploadErrorStatus,
  uploadProgressStatus,
} from "@/runtime/event-reducers";
import { loadStartupSettings } from "@/runtime/settings-runtime";
import type {
  BatchSeparationProgress,
  ModelBootstrapStatusSnapshot,
  PlaybackEndedEvent,
  PlaybackErrorEvent,
  PlaybackPositionEvent,
  RuntimeBootstrapStatusSnapshot,
  SeparationCompleteEvent,
  SeparationErrorEvent,
  SeparationProgressEvent,
  UploadCompleteEvent,
  UploadErrorEvent,
  UploadProgressEvent,
} from "@/types/ipc";

export function useLyricsAutoFetch(enabled = true) {
  const songId = usePlayerStore((s) => s.snapshot?.song_id) ?? undefined;
  const fetchLyrics = useLyricsStore((s) => s.fetchLyrics);
  const prevSongIdRef = useRef<string | undefined>(undefined);

  useEffect(() => {
    if (!enabled) {
      prevSongIdRef.current = undefined;
      return;
    }

    if (songId && songId !== prevSongIdRef.current) {
      fetchLyrics(songId);
    }
    prevSongIdRef.current = songId;
  }, [enabled, songId, fetchLyrics]);
}

function usePlaybackPositionSubscription(
  enabled: boolean,
  onPosition: (event: PlaybackPositionEvent) => void,
) {
  useEffect(() => {
    if (!enabled) {
      return;
    }

    let cancelled = false;
    let unlisten: (() => void) | null = null;

    const setup = async () => {
      unlisten = await listen<PlaybackPositionEvent>(
        "playback-position",
        (e) => {
          if (!cancelled) onPosition(e.payload);
        },
      );
      // F3: If unmount happened before listen() resolved, clean up now.
      if (cancelled) {
        unlisten();
        return;
      }
    };

    void setup();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [enabled, onPosition]);
}

function usePlaybackPositionEvents(enabled: boolean) {
  const applyPlaybackPositionEvent = usePlayerStore(
    (s) => s.applyPlaybackPositionEvent,
  );
  usePlaybackPositionSubscription(enabled, applyPlaybackPositionEvent);
}

function useSeparationEvents(enabled: boolean) {
  const updateSeparationStatus = useLibraryStore(
    (s) => s.updateSeparationStatus,
  );
  const loadStems = usePlayerStore((s) => s.loadStems);

  const currentSongIdRef = useRef<string | undefined>(undefined);
  const currentSongId = usePlayerStore((s) => s.snapshot?.song_id) ?? undefined;

  useEffect(() => {
    if (!enabled) {
      currentSongIdRef.current = undefined;
      return;
    }

    currentSongIdRef.current = currentSongId;
  }, [enabled, currentSongId]);

  useEffect(() => {
    if (!enabled) {
      return;
    }

    const unlisteners: (() => void)[] = [];
    let cancelled = false;

    const setup = async () => {
      const progressUnlisten = await listen<SeparationProgressEvent>(
        "separation-progress",
        (e) => {
          if (!cancelled) {
            updateSeparationStatus(separationProgressStatus(e.payload));
          }
        },
      );

      const completeUnlisten = await listen<SeparationCompleteEvent>(
        "separation-complete",
        (e) => {
          if (cancelled) return;
          updateSeparationStatus(e.payload.status);

          if (e.payload.song_id === currentSongIdRef.current) {
            loadStems().catch((err) => notifyError(err));
          }
        },
      );

      const errorUnlisten = await listen<SeparationErrorEvent>(
        "separation-error",
        (e) => {
          if (!cancelled) {
            updateSeparationStatus(separationErrorStatus(e.payload));
            notifyError(e.payload.error);
          }
        },
      );

      if (cancelled) {
        progressUnlisten();
        completeUnlisten();
        errorUnlisten();
      } else {
        unlisteners.push(progressUnlisten, completeUnlisten, errorUnlisten);
      }
    };

    void setup();

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [enabled, loadStems, updateSeparationStatus]);
}

function useBootstrapEvents(enabled: boolean) {
  const updateBootstrapStatus = useBootstrapStore((s) => s.updateStatus);
  const updateRuntimeBootstrapStatus = useRuntimeBootstrapStore(
    (s) => s.updateStatus,
  );

  useEventSubscriptions(
    [
      {
        event: "model-bootstrap-progress",
        handler: (payload) =>
          updateBootstrapStatus(payload as ModelBootstrapStatusSnapshot),
      },
      {
        event: "model-bootstrap-ready",
        handler: (payload) =>
          updateBootstrapStatus(payload as ModelBootstrapStatusSnapshot),
      },
      {
        event: "model-bootstrap-error",
        handler: (payload) =>
          updateBootstrapStatus(payload as ModelBootstrapStatusSnapshot),
      },
      {
        event: "runtime-bootstrap-progress",
        handler: (payload) =>
          updateRuntimeBootstrapStatus(
            payload as RuntimeBootstrapStatusSnapshot,
          ),
      },
      {
        event: "runtime-bootstrap-ready",
        handler: (payload) =>
          updateRuntimeBootstrapStatus(
            payload as RuntimeBootstrapStatusSnapshot,
          ),
      },
      {
        event: "runtime-bootstrap-error",
        handler: (payload) =>
          updateRuntimeBootstrapStatus(
            payload as RuntimeBootstrapStatusSnapshot,
          ),
      },
    ],
    enabled,
    undefined,
    [updateBootstrapStatus, updateRuntimeBootstrapStatus],
  );
}

function usePlaybackErrorEvents(enabled: boolean) {
  useEventSubscriptions(
    [
      {
        event: "playback-error",
        handler: (payload) => {
          const event = payload as PlaybackErrorEvent;
          notifyError(event.error, () =>
            usePlayerStore.getState().playSong(event.song_id),
          );
        },
      },
    ],
    enabled,
  );
}

function usePlaybackEndedQueueAdvance(enabled: boolean) {
  useEventSubscriptions(
    [
      {
        event: "playback-ended",
        handler: (payload) => {
          usePlayerStore
            .getState()
            .playNextFromQueue((payload as PlaybackEndedEvent).song_id);
        },
      },
    ],
    enabled,
  );
}

function useBatchSeparationEvents(enabled: boolean) {
  const updateBatchProgress = useLibraryStore((s) => s.updateBatchProgress);
  const clearBatchSeparation = useLibraryStore((s) => s.clearBatchSeparation);
  const clearSchedulerRef = useRef(
    createBatchSeparationClearScheduler(clearBatchSeparation),
  );

  // Recreate scheduler if the clear function changes — drain the old one first
  // so any pending timer invokes the stale closure.
  useEffect(() => {
    clearSchedulerRef.current.clearAll();
    clearSchedulerRef.current =
      createBatchSeparationClearScheduler(clearBatchSeparation);
  }, [clearBatchSeparation]);

  useEventSubscriptions(
    [
      {
        event: "batch-separation-progress",
        handler: (payload) =>
          updateBatchProgress(payload as BatchSeparationProgress),
      },
      {
        event: "batch-separation-complete",
        handler: (payload) => {
          updateBatchProgress(payload as BatchSeparationProgress);
          clearSchedulerRef.current.scheduleAfterTerminalProgress();
        },
      },
      {
        event: "batch-separation-cancelled",
        handler: (payload) => {
          updateBatchProgress(payload as BatchSeparationProgress);
          clearSchedulerRef.current.scheduleAfterTerminalProgress();
        },
      },
    ],
    enabled,
    () => clearSchedulerRef.current.clearAll(),
    [updateBatchProgress],
  );
}

function useUploadEvents(enabled: boolean) {
  const updateUploadStatus = useLibraryStore((s) => s.updateUploadStatus);
  const clearUploadStatus = useLibraryStore((s) => s.clearUploadStatus);
  const clearSchedulerRef = useRef(
    createStatusClearScheduler<string>(clearUploadStatus),
  );

  // Recreate scheduler if the clear function changes — drain the old one first
  // so any pending timer invokes the stale closure.
  useEffect(() => {
    clearSchedulerRef.current.clearAll();
    clearSchedulerRef.current =
      createStatusClearScheduler<string>(clearUploadStatus);
  }, [clearUploadStatus]);

  useEventSubscriptions(
    [
      {
        event: "upload-progress",
        handler: (payload) => {
          const event = payload as UploadProgressEvent;
          clearSchedulerRef.current.cancel(event.song_id);
          updateUploadStatus(uploadProgressStatus(event));
        },
      },
      {
        event: "upload-complete",
        handler: (payload) => {
          const event = payload as UploadCompleteEvent;
          updateUploadStatus(uploadCompleteStatus(event));
          clearSchedulerRef.current.schedule(event.song_id);
        },
      },
      {
        event: "upload-error",
        handler: (payload) => {
          const event = payload as UploadErrorEvent;
          clearSchedulerRef.current.cancel(event.song_id);
          updateUploadStatus(uploadErrorStatus(event));
          notifyError(event.error);
        },
      },
    ],
    enabled,
    () => clearSchedulerRef.current.clearAll(),
    [updateUploadStatus],
  );
}

export function useEventListeners(enabled = true) {
  usePlaybackPositionEvents(enabled);
  usePlaybackErrorEvents(enabled);
  useSeparationEvents(enabled);
  useBootstrapEvents(enabled);
  usePlaybackEndedQueueAdvance(enabled);
  useBatchSeparationEvents(enabled);
  useUploadEvents(enabled);
}

export function useFullscreenPlaybackRuntime() {
  const applyPlaybackPositionEvent = usePlayerStore(
    (s) => s.applyPlaybackPositionEvent,
  );
  const updateSnapshot = usePlayerStore((s) => s.updateSnapshot);
  const hydrateAppSettings = useSettingsStore((s) => s.hydrateAppSettings);

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

  usePlaybackPositionSubscription(true, applyPlaybackPositionEvent);
}
