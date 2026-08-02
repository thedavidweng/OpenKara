import { listen } from "@tauri-apps/api/event";
import type {
  BatchSeparationProgress,
  ModelBootstrapStatusSnapshot,
  PlaybackEndedEvent,
  PlaybackErrorEvent,
  PlaybackPositionEvent,
  RemotePlaybackFailedEvent,
  RemotePlaybackReconnectEvent,
  RemotePlaybackResyncEvent,
  RuntimeBootstrapStatusSnapshot,
  SeparationCancelledEvent,
  SeparationCompleteEvent,
  SeparationErrorEvent,
  SeparationProgressEvent,
  TrackTransitionedEvent,
  UploadCompleteEvent,
  UploadErrorEvent,
  UploadProgressEvent,
} from "@/types/ipc";

export interface RuntimeEventMap {
  "playback-position": PlaybackPositionEvent;
  "playback-error": PlaybackErrorEvent;
  "playback-ended": PlaybackEndedEvent;
  "track-transitioned": TrackTransitionedEvent;
  "model-bootstrap-progress": ModelBootstrapStatusSnapshot;
  "model-bootstrap-ready": ModelBootstrapStatusSnapshot;
  "model-bootstrap-error": ModelBootstrapStatusSnapshot;
  "runtime-bootstrap-progress": RuntimeBootstrapStatusSnapshot;
  "runtime-bootstrap-ready": RuntimeBootstrapStatusSnapshot;
  "runtime-bootstrap-error": RuntimeBootstrapStatusSnapshot;
  "separation-progress": SeparationProgressEvent;
  "separation-complete": SeparationCompleteEvent;
  "separation-error": SeparationErrorEvent;
  "separation-cancelled": SeparationCancelledEvent;
  "batch-separation-progress": BatchSeparationProgress;
  "batch-separation-complete": BatchSeparationProgress;
  "batch-separation-cancelled": BatchSeparationProgress;
  "upload-progress": UploadProgressEvent;
  "upload-complete": UploadCompleteEvent;
  "upload-error": UploadErrorEvent;
  "remote-playback-reconnect": RemotePlaybackReconnectEvent;
  "remote-playback-resync": RemotePlaybackResyncEvent;
  "remote-playback-failed": RemotePlaybackFailedEvent;
}

export type RuntimeEventName = keyof RuntimeEventMap;
export type RuntimeEventHandler<K extends RuntimeEventName> = (
  payload: RuntimeEventMap[K],
) => void;

export interface RuntimeEventSource {
  listen<K extends RuntimeEventName>(
    event: K,
    handler: RuntimeEventHandler<K>,
  ): Promise<() => void>;
}

export const tauriRuntimeEventSource: RuntimeEventSource = {
  listen: (event, handler) =>
    listen<RuntimeEventMap[typeof event]>(event, (received) =>
      handler(received.payload),
    ),
};

export interface RuntimeEventSubscription {
  subscribe(): Promise<() => void>;
}

export function eventSubscription<K extends RuntimeEventName>(
  event: K,
  handler: RuntimeEventHandler<K>,
  source: RuntimeEventSource = tauriRuntimeEventSource,
): RuntimeEventSubscription {
  return {
    subscribe: () => source.listen(event, handler),
  };
}

export function createRecordingRuntimeEventSource(): RuntimeEventSource & {
  emit<K extends RuntimeEventName>(event: K, payload: RuntimeEventMap[K]): void;
} {
  type RuntimeEventHandlers = {
    [K in RuntimeEventName]: Set<RuntimeEventHandler<K>>;
  };

  const handlers: RuntimeEventHandlers = {
    "playback-position": new Set(),
    "playback-error": new Set(),
    "playback-ended": new Set(),
    "track-transitioned": new Set(),
    "model-bootstrap-progress": new Set(),
    "model-bootstrap-ready": new Set(),
    "model-bootstrap-error": new Set(),
    "runtime-bootstrap-progress": new Set(),
    "runtime-bootstrap-ready": new Set(),
    "runtime-bootstrap-error": new Set(),
    "separation-progress": new Set(),
    "separation-complete": new Set(),
    "separation-error": new Set(),
    "separation-cancelled": new Set(),
    "batch-separation-progress": new Set(),
    "batch-separation-complete": new Set(),
    "batch-separation-cancelled": new Set(),
    "upload-progress": new Set(),
    "upload-complete": new Set(),
    "upload-error": new Set(),
    "remote-playback-reconnect": new Set(),
    "remote-playback-resync": new Set(),
    "remote-playback-failed": new Set(),
  };

  return {
    listen: async <K extends RuntimeEventName>(
      event: K,
      handler: RuntimeEventHandler<K>,
    ) => {
      const eventHandlers = handlers[event];
      eventHandlers.add(handler);
      return () => eventHandlers.delete(handler);
    },
    emit: <K extends RuntimeEventName>(
      event: K,
      payload: RuntimeEventMap[K],
    ) => {
      handlers[event].forEach((handler) => handler(payload));
    },
  };
}
