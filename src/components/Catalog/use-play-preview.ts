import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { onlinePreviewQueueId } from "@/playback";
import { useCatalogStore } from "@/stores/catalog-store";
import { useNotificationStore } from "@/stores/notification-store";
import { usePlayerStore } from "@/stores/player-store";
import { useQueueStore } from "@/stores/queue-store";
import type { StreamingTrack } from "@/types/ipc";

/** Tracks after the played one that join the queue. */
export const PREVIEW_QUEUE_AHEAD = 100;

/** A track already imported plays from the library; others stream. */
export function playbackIdFor(track: StreamingTrack): string {
  return track.library_song_id ?? onlinePreviewQueueId(track.remote_track_id);
}

/**
 * Play `tracks[index]` and queue the playable tracks after it. Imported
 * tracks play from the library; the rest stream from NetEase as previews.
 */
export function usePlayPreview() {
  const { t } = useTranslation();
  return useCallback(
    async (tracks: StreamingTrack[], index: number) => {
      const track = tracks[index];
      if (!track) return;
      if (track.refusal) {
        useNotificationStore.getState().addNotification({
          type: "info",
          title: t("catalog.netease.cantPlayTitle", { title: track.title }),
          message: t(`catalog.netease.refusalReason.${track.refusal.reason}`),
          retryable: false,
          dismissAfterMs: 5000,
        });
        return;
      }
      const ahead = tracks
        .slice(index + 1)
        .filter((candidate) => !candidate.refusal)
        .slice(0, PREVIEW_QUEUE_AHEAD);
      useCatalogStore.getState().rememberPreviewTracks([track, ...ahead]);
      useQueueStore.getState().setQueue(ahead.map(playbackIdFor));
      await usePlayerStore.getState().playNow(playbackIdFor(track));
    },
    [t],
  );
}
