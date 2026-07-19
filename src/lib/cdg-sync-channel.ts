export interface CdgSyncStatusPayload {
  songId: string | null;
  hasCdg: boolean;
}

/**
 * Frame payload broadcast over the sync channel. Carries the RGBA bytes plus
 * the frame version and transport generation so receivers can skip redundant
 * redraws and detect stale frames.
 */
export interface CdgSyncFramePayload {
  /** RGBA frame bytes (288×192×4 = 221,184 bytes). */
  rgba: Uint8Array;
  /** Frame version from the backend. */
  frameVersion: number;
  /** Transport generation from the backend. */
  transportGeneration: number;
}

export type CdgSyncMessage =
  | { type: "request-sync" }
  | { type: "clear" }
  | { type: "status"; payload: CdgSyncStatusPayload }
  | { type: "frame"; payload: CdgSyncFramePayload };

export interface CdgSyncChannel {
  postMessage: (message: CdgSyncMessage) => void;
  addEventListener: (
    type: "message",
    listener: (event: MessageEvent<CdgSyncMessage>) => void,
  ) => void;
  removeEventListener: (
    type: "message",
    listener: (event: MessageEvent<CdgSyncMessage>) => void,
  ) => void;
  close: () => void;
}

const CDG_SYNC_CHANNEL_NAME = "openkara-cdg-sync-v2";

let cachedChannel: CdgSyncChannel | null | undefined;

function createDefaultChannel(name: string): CdgSyncChannel {
  return new BroadcastChannel(name) as unknown as CdgSyncChannel;
}

export function createCdgSyncChannel(
  factory: (name: string) => CdgSyncChannel = createDefaultChannel,
): CdgSyncChannel | null {
  if (typeof BroadcastChannel === "undefined") {
    return null;
  }

  try {
    return factory(CDG_SYNC_CHANNEL_NAME);
  } catch {
    return null;
  }
}

export function getCdgSyncChannel(): CdgSyncChannel | null {
  if (cachedChannel !== undefined) {
    return cachedChannel;
  }

  cachedChannel = createCdgSyncChannel();
  return cachedChannel;
}

export function postCdgStatus(
  channel: CdgSyncChannel | null,
  payload: CdgSyncStatusPayload,
): void {
  channel?.postMessage({ type: "status", payload });
}

export function postCdgFrame(
  channel: CdgSyncChannel | null,
  payload: CdgSyncFramePayload,
): void {
  channel?.postMessage({ type: "frame", payload });
}

export function postCdgClear(channel: CdgSyncChannel | null): void {
  channel?.postMessage({ type: "clear" });
}

export function startCdgSyncRequestListener({
  channel,
  getSnapshot,
}: {
  channel: CdgSyncChannel;
  getSnapshot: () => {
    status: CdgSyncStatusPayload;
    frame: CdgSyncFramePayload | null;
  };
}): () => void {
  const onMessage = (event: MessageEvent<CdgSyncMessage>) => {
    if (event.data.type !== "request-sync") {
      return;
    }

    const { status, frame } = getSnapshot();
    postCdgStatus(channel, status);
    if (frame) {
      postCdgFrame(channel, frame);
    }
  };

  channel.addEventListener("message", onMessage);

  return () => {
    channel.removeEventListener("message", onMessage);
  };
}

export function startCdgSyncReceiver({
  channel,
  onFrame,
  onClear,
  onStatus,
}: {
  channel: CdgSyncChannel;
  onFrame: (payload: CdgSyncFramePayload) => void;
  onClear: () => void;
  onStatus: (payload: CdgSyncStatusPayload) => void;
}): () => void {
  const onMessage = (event: MessageEvent<CdgSyncMessage>) => {
    switch (event.data.type) {
      case "frame":
        onFrame(event.data.payload);
        break;
      case "clear":
        onClear();
        break;
      case "status":
        onStatus(event.data.payload);
        break;
      default:
        break;
    }
  };

  channel.addEventListener("message", onMessage);
  channel.postMessage({ type: "request-sync" });

  return () => {
    channel.removeEventListener("message", onMessage);
  };
}
