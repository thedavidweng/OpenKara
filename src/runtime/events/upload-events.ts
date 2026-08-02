import { useEffect, useRef } from "react";
import { useEventSubscriptions } from "@/hooks/use-event-subscription";
import { useLibraryStore } from "@/stores/library-store";
import { notifyError } from "@/lib/errors";
import {
  createStatusClearScheduler,
  uploadCompleteStatus,
  uploadErrorStatus,
  uploadProgressStatus,
} from "@/runtime/event-reducers";
import {
  eventSubscription,
  tauriRuntimeEventSource,
  type RuntimeEventSource,
} from "@/runtime/event-source";

export function useUploadEvents(
  enabled: boolean,
  source: RuntimeEventSource = tauriRuntimeEventSource,
) {
  const updateUploadStatus = useLibraryStore(
    (state) => state.updateUploadStatus,
  );
  const clearUploadStatus = useLibraryStore((state) => state.clearUploadStatus);
  const clearSchedulerRef = useRef(
    createStatusClearScheduler<string>(clearUploadStatus),
  );

  useEffect(() => {
    clearSchedulerRef.current.clearAll();
    clearSchedulerRef.current =
      createStatusClearScheduler<string>(clearUploadStatus);
  }, [clearUploadStatus]);

  useEventSubscriptions(
    [
      eventSubscription(
        "upload-progress",
        (event) => {
          clearSchedulerRef.current.cancel(event.song_id);
          updateUploadStatus(uploadProgressStatus(event));
        },
        source,
      ),
      eventSubscription(
        "upload-complete",
        (event) => {
          updateUploadStatus(uploadCompleteStatus(event));
          clearSchedulerRef.current.schedule(event.song_id);
        },
        source,
      ),
      eventSubscription(
        "upload-error",
        (event) => {
          clearSchedulerRef.current.cancel(event.song_id);
          updateUploadStatus(uploadErrorStatus(event));
          notifyError(event.error);
        },
        source,
      ),
    ],
    enabled,
    () => clearSchedulerRef.current.clearAll(),
    [source, updateUploadStatus],
  );
}
