import { useEventSubscriptions } from "@/hooks/use-event-subscription";
import { useRemotePlaybackStore } from "@/stores/remote-playback-store";
import {
  eventSubscription,
  tauriRuntimeEventSource,
  type RuntimeEventSource,
} from "@/runtime/event-source";

export function useRemotePlaybackEvents(
  enabled: boolean,
  source: RuntimeEventSource = tauriRuntimeEventSource,
) {
  const applyReconnectEvent = useRemotePlaybackStore(
    (state) => state.applyReconnectEvent,
  );
  const applyResyncEvent = useRemotePlaybackStore(
    (state) => state.applyResyncEvent,
  );
  const applyFailedEvent = useRemotePlaybackStore(
    (state) => state.applyFailedEvent,
  );

  useEventSubscriptions(
    [
      eventSubscription(
        "remote-playback-reconnect",
        applyReconnectEvent,
        source,
      ),
      eventSubscription("remote-playback-resync", applyResyncEvent, source),
      eventSubscription("remote-playback-failed", applyFailedEvent, source),
    ],
    enabled,
    undefined,
    [applyReconnectEvent, applyResyncEvent, applyFailedEvent, source],
  );
}
