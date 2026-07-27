import { useEffect } from "react";
import { useCdgStore } from "@/stores/cdg-store";
import { drawFrame, clearFrame } from "@/lib/cdg-canvas-painter";
import {
  getCdgSyncChannel,
  startCdgSyncReceiver,
  type CdgSyncChannel,
  type CdgSyncFramePayload,
  type CdgSyncStatusPayload,
} from "@/lib/cdg-sync-channel";

export function createCoalescingPainter<T>(paint: (frame: T) => void): {
  enqueue: (frame: T) => void;
  cancel: () => void;
} {
  let latestFrame: T | null = null;
  let timerId: ReturnType<typeof setTimeout> | null = null;

  const flush = () => {
    timerId = null;
    if (latestFrame !== null) {
      paint(latestFrame);
      latestFrame = null;
    }
  };

  return {
    enqueue: (frame: T) => {
      latestFrame = frame;
      if (timerId === null) {
        timerId = setTimeout(flush, 0);
      }
    },
    cancel: () => {
      if (timerId !== null) {
        clearTimeout(timerId);
        timerId = null;
      }
      latestFrame = null;
    },
  };
}

export function startCdgBroadcastFrameReceiver({
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
  const painter = createCoalescingPainter(onFrame);
  const stopReceiver = startCdgSyncReceiver({
    channel,
    onFrame: (payload) => {
      painter.enqueue(payload);
    },
    onClear: () => {
      painter.cancel();
      onClear();
    },
    onStatus,
  });

  return () => {
    painter.cancel();
    stopReceiver();
  };
}

export function useCdgFrameReceiver(): void {
  const setSong = useCdgStore((s) => s.setSong);
  const setFrameVersion = useCdgStore((s) => s.setFrameVersion);
  const clear = useCdgStore((s) => s.clear);

  useEffect(() => {
    const channel = getCdgSyncChannel();
    if (!channel) {
      return;
    }

    return startCdgBroadcastFrameReceiver({
      channel,
      onFrame: ({ rgba, frameVersion, transportGeneration }) => {
        setFrameVersion(frameVersion, transportGeneration);
        drawFrame(rgba);
      },
      onClear: () => {
        clear();
        clearFrame();
      },
      onStatus: ({ songId, hasCdg }) => {
        if (songId !== null) {
          setSong(songId, hasCdg);
        } else {
          clear();
        }
      },
    });
  }, [clear, setFrameVersion, setSong]);
}
