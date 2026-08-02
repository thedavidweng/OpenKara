import { useEventSubscriptions } from "@/hooks/use-event-subscription";
import { usePlayerStore } from "@/stores/player-store";
import { notifyError } from "@/lib/errors";
import {
  eventSubscription,
  tauriRuntimeEventSource,
  type RuntimeEventSource,
} from "@/runtime/event-source";
import type { PlaybackPositionEvent } from "@/types/ipc";

export function usePlaybackPositionSubscription(
  enabled: boolean,
  onPosition: (event: PlaybackPositionEvent) => void,
  source: RuntimeEventSource = tauriRuntimeEventSource,
) {
  useEventSubscriptions(
    [eventSubscription("playback-position", onPosition, source)],
    enabled,
    undefined,
    [onPosition, source],
  );
}

export function usePlaybackEvents(
  enabled: boolean,
  source: RuntimeEventSource = tauriRuntimeEventSource,
) {
  const applyPlaybackPositionEvent = usePlayerStore(
    (state) => state.applyPlaybackPositionEvent,
  );

  usePlaybackPositionSubscription(enabled, applyPlaybackPositionEvent, source);

  useEventSubscriptions(
    [
      eventSubscription(
        "playback-error",
        (event) => {
          notifyError(event.error, () =>
            usePlayerStore.getState().playSong(event.song_id),
          );
        },
        source,
      ),
      eventSubscription(
        "playback-ended",
        (event) => {
          usePlayerStore.getState().playNextFromQueue(event.song_id);
        },
        source,
      ),
      eventSubscription(
        "track-transitioned",
        (event) => {
          usePlayerStore
            .getState()
            .onTrackTransitioned(event.from_song_id, event.to_song_id);
        },
        source,
      ),
    ],
    enabled,
    undefined,
    [source],
  );
}
