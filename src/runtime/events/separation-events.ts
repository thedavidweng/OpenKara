import { useEffect, useRef } from "react";
import { useEventSubscriptions } from "@/hooks/use-event-subscription";
import { useLibraryStore } from "@/stores/library-store";
import { usePlayerStore } from "@/stores/player-store";
import { notifyError, notifySuccess } from "@/lib/errors";
import { notifyWhenUnfocused } from "@/lib/notifications";
import { songDisplayTitle } from "@/lib/song-display";
import i18next from "@/lib/i18n";
import {
  createBatchSeparationClearScheduler,
  separationCancelledStatus,
  separationErrorStatus,
  separationProgressStatus,
} from "@/runtime/event-reducers";
import {
  eventSubscription,
  tauriRuntimeEventSource,
  type RuntimeEventSource,
} from "@/runtime/event-source";

function songTitleFor(songId: string): string {
  return songDisplayTitle(
    useLibraryStore.getState().songs.find((song) => song.hash === songId),
  );
}

function batchInProgress(): boolean {
  return useLibraryStore.getState().batchSeparation != null;
}

export function useSeparationEvents(
  enabled: boolean,
  source: RuntimeEventSource = tauriRuntimeEventSource,
) {
  const updateSeparationStatus = useLibraryStore(
    (state) => state.updateSeparationStatus,
  );
  const loadStems = usePlayerStore((state) => state.loadStems);
  const currentSongId =
    usePlayerStore((state) => state.snapshot?.song_id) ?? undefined;
  const currentSongIdRef = useRef<string | undefined>(undefined);

  useEffect(() => {
    if (!enabled) {
      currentSongIdRef.current = undefined;
      return;
    }
    currentSongIdRef.current = currentSongId;
  }, [enabled, currentSongId]);

  useEventSubscriptions(
    [
      eventSubscription(
        "separation-progress",
        (payload) => {
          updateSeparationStatus(separationProgressStatus(payload));
        },
        source,
      ),
      eventSubscription(
        "separation-complete",
        (payload) => {
          updateSeparationStatus(payload.status);
          if (payload.status.cache_hit) {
            notifySuccess(i18next.t("library.usingCachedSeparation"));
          } else if (!batchInProgress()) {
            void notifyWhenUnfocused(
              i18next.t("notifications.separationComplete"),
              songTitleFor(payload.song_id),
            );
          }
          if (payload.song_id === currentSongIdRef.current) {
            loadStems().catch((error) => notifyError(error));
          }
        },
        source,
      ),
      eventSubscription(
        "separation-error",
        (payload) => {
          updateSeparationStatus(separationErrorStatus(payload));
          notifyError(payload.error);
          if (!batchInProgress()) {
            void notifyWhenUnfocused(
              i18next.t("notifications.separationFailed"),
              songTitleFor(payload.song_id),
            );
          }
        },
        source,
      ),
      eventSubscription(
        "separation-cancelled",
        (payload) => {
          updateSeparationStatus(separationCancelledStatus(payload));
        },
        source,
      ),
    ],
    enabled,
    undefined,
    [loadStems, source, updateSeparationStatus],
  );
}

export function useBatchSeparationEvents(
  enabled: boolean,
  source: RuntimeEventSource = tauriRuntimeEventSource,
) {
  const updateBatchProgress = useLibraryStore(
    (state) => state.updateBatchProgress,
  );
  const clearBatchSeparation = useLibraryStore(
    (state) => state.clearBatchSeparation,
  );
  const clearSchedulerRef = useRef(
    createBatchSeparationClearScheduler(clearBatchSeparation),
  );

  useEffect(() => {
    clearSchedulerRef.current.clearAll();
    clearSchedulerRef.current =
      createBatchSeparationClearScheduler(clearBatchSeparation);
  }, [clearBatchSeparation]);

  useEventSubscriptions(
    [
      eventSubscription(
        "batch-separation-progress",
        (progress) => {
          updateBatchProgress(progress);
        },
        source,
      ),
      eventSubscription(
        "batch-separation-complete",
        (progress) => {
          updateBatchProgress(progress);
          clearSchedulerRef.current.scheduleAfterTerminalProgress();
          void notifyWhenUnfocused(
            i18next.t("notifications.batchSeparationComplete"),
            i18next.t("notifications.batchSeparationSummary", {
              done: progress.completed,
              failed: progress.failed,
            }),
          );
        },
        source,
      ),
      eventSubscription(
        "batch-separation-cancelled",
        (progress) => {
          updateBatchProgress(progress);
          clearSchedulerRef.current.scheduleAfterTerminalProgress();
        },
        source,
      ),
    ],
    enabled,
    () => clearSchedulerRef.current.clearAll(),
    [source, updateBatchProgress],
  );
}
