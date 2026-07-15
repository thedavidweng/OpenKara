import { useEffect, useRef } from "react";
import {
  selectSyncDisplayPositionMs,
  usePlayerStore,
} from "@/stores/player-store";
import { useCdgStore } from "@/stores/cdg-store";
import { useLibraryStore } from "@/stores/library-store";
import { drawFrame, clearFrame } from "@/lib/cdg-canvas-painter";
import {
  getCdgSyncChannel,
  postCdgClear,
  postCdgFrame,
  postCdgStatus,
  startCdgSyncRequestListener,
  type CdgSyncFramePayload,
  type CdgSyncStatusPayload,
} from "@/lib/cdg-sync-channel";
import { ensureArrayBuffer, parseCdgFrameResponse } from "@/lib/cdg-protocol";
import { songHasCdgMedia } from "@/lib/song-media";
import * as api from "@/lib/tauri";

/**
 * Target cadence for CDG frame fetches. We no longer rely on JS timers here,
 * because macOS can throttle them in occluded windows; instead we map backend
 * playback-position events into 33ms buckets and fetch once per bucket.
 *
 * RATIONALE: This is part of the second-window CDG fix, not redundant code.
 * When the audience window covers most of the main window, macOS can throttle
 * the main window's JS timers toward slideshow cadence. The main window must
 * therefore advance CDG from Rust playback-position events, then publish those
 * frames to the second window over BroadcastChannel.
 */
const MIN_INTERVAL_MS = 33;

let lastFrame: CdgSyncFramePayload | null = null;
let lastStatus: CdgSyncStatusPayload = {
  songId: null,
  hasCdg: false,
};

function emitCdgFrame(payload: CdgSyncFramePayload): void {
  lastFrame = payload;
  postCdgFrame(getCdgSyncChannel(), payload);
}

function emitCdgClear(): void {
  lastFrame = null;
  postCdgClear(getCdgSyncChannel());
}

function emitCdgStatus(songId: string | null, hasCdg: boolean): void {
  lastStatus = { songId, hasCdg };
  postCdgStatus(getCdgSyncChannel(), lastStatus);
}

export function getCdgSyncBucket(positionMs: number): number {
  return Math.floor(Math.max(0, positionMs) / MIN_INTERVAL_MS);
}

export function startCdgPositionSync(
  tick: () => void,
  subscribe: (
    listener: (positionMs: number, previousPositionMs: number) => void,
  ) => () => void,
): () => void {
  return subscribe((positionMs, previousPositionMs) => {
    if (getCdgSyncBucket(positionMs) !== getCdgSyncBucket(previousPositionMs)) {
      tick();
    }
  });
}

/**
 * Shared in-flight coordinator for probe + hot-loop CDG frame IPC.
 * At most one `getCdgFrame` is outstanding for the local timeline; a newer
 * desired request is coalesced and issued when the in-flight one completes.
 * A monotonic serial invalidates late results after song/generation change.
 */
export type CdgFrameRequest = {
  songId: string;
  transportGeneration: number;
  positionMs: number;
  lastFrameVersion: number;
  /** Monotonic serial at request enqueue time. */
  serial: number;
};

export type CdgFrameCoordinator = {
  /** Enqueue a desired frame request; returns false if dropped as identical/noop. */
  request: (req: Omit<CdgFrameRequest, "serial">) => void;
  /** Invalidate outstanding work (song change / unmount). */
  invalidate: () => void;
  /** Test helper: is a request currently in flight? */
  isInFlight: () => boolean;
  /** Test helper: current serial. */
  currentSerial: () => number;
};

export function createCdgFrameCoordinator(deps: {
  getCdgFrame: typeof api.getCdgFrame;
  onFrame: (args: {
    songId: string;
    transportGeneration: number;
    frameVersion: number;
    rgba: Uint8ClampedArray | Uint8Array;
  }) => void;
  onProbeResolved: (args: {
    songId: string;
    transportGeneration: number;
    hasFrame: boolean;
  }) => void;
  onError: (args: { songId: string; transportGeneration: number }) => void;
  /** Optional: is this request still relevant? */
  isCurrent: (req: CdgFrameRequest) => boolean;
}): CdgFrameCoordinator {
  let serial = 0;
  let inFlight = false;
  let pending: CdgFrameRequest | null = null;

  const pump = () => {
    if (inFlight || !pending) return;
    const req = pending;
    pending = null;
    inFlight = true;
    deps
      .getCdgFrame(
        req.songId,
        req.transportGeneration,
        req.positionMs,
        req.lastFrameVersion,
      )
      .then((result) => {
        if (req.serial !== serial || !deps.isCurrent(req)) {
          return;
        }
        const buffer = ensureArrayBuffer(result);
        const envelope = parseCdgFrameResponse(buffer);
        if (envelope && envelope.hasRgba && envelope.rgba) {
          deps.onFrame({
            songId: req.songId,
            transportGeneration: envelope.transportGeneration,
            frameVersion: envelope.frameVersion,
            rgba: envelope.rgba,
          });
          deps.onProbeResolved({
            songId: req.songId,
            transportGeneration: req.transportGeneration,
            hasFrame: true,
          });
        } else {
          deps.onProbeResolved({
            songId: req.songId,
            transportGeneration: req.transportGeneration,
            hasFrame: false,
          });
        }
      })
      .catch(() => {
        if (req.serial !== serial || !deps.isCurrent(req)) {
          return;
        }
        deps.onError({
          songId: req.songId,
          transportGeneration: req.transportGeneration,
        });
      })
      .finally(() => {
        inFlight = false;
        // Drop if invalidated while in flight.
        if (pending && pending.serial !== serial) {
          pending = null;
        }
        pump();
      });
  };

  return {
    request: (partial) => {
      serial += 1;
      pending = { ...partial, serial };
      pump();
    },
    invalidate: () => {
      serial += 1;
      pending = null;
    },
    isInFlight: () => inFlight,
    currentSerial: () => serial,
  };
}

export function useCdgSync(enabled = true): void {
  const songId = usePlayerStore((s) => s.snapshot?.song_id ?? null);
  const transportGeneration = usePlayerStore(
    (s) => s.snapshot?.transport_generation ?? 0,
  );
  const currentSong = useLibraryStore(
    (s) => s.songs.find((song) => song.hash === songId) ?? null,
  );
  const setSong = useCdgStore((s) => s.setSong);
  const clear = useCdgStore((s) => s.clear);
  const setFrameVersion = useCdgStore((s) => s.setFrameVersion);
  const currentSongHasCdg = songHasCdgMedia(currentSong);

  const coordinatorRef = useRef<CdgFrameCoordinator | null>(null);

  // Build / rebuild coordinator when enabled; invalidate on cleanup.
  useEffect(() => {
    if (!enabled) {
      coordinatorRef.current = null;
      return;
    }

    const coordinator = createCdgFrameCoordinator({
      getCdgFrame: api.getCdgFrame,
      isCurrent: (req) => {
        const snap = usePlayerStore.getState().snapshot;
        return (
          req.songId === (snap?.song_id ?? null) &&
          req.transportGeneration === (snap?.transport_generation ?? 0)
        );
      },
      onFrame: ({
        songId: sid,
        transportGeneration: gen,
        frameVersion,
        rgba,
      }) => {
        setFrameVersion(frameVersion, gen);
        drawFrame(rgba);
        emitCdgFrame({
          rgba,
          frameVersion,
          transportGeneration: gen,
        });
        // Ensure store marks CDG present after a successful frame.
        if (
          useCdgStore.getState().songId !== sid ||
          !useCdgStore.getState().hasCdg
        ) {
          setSong(sid, true);
        }
      },
      onProbeResolved: ({ songId: sid, hasFrame }) => {
        if (!hasFrame) {
          // Soft-confirm status without clearing an already-drawn frame.
          emitCdgStatus(sid, true);
        } else {
          emitCdgStatus(sid, true);
        }
      },
      onError: ({ songId: sid }) => {
        setSong(sid, false);
        clearFrame();
        emitCdgStatus(sid, false);
      },
    });
    coordinatorRef.current = coordinator;

    return () => {
      coordinator.invalidate();
      if (coordinatorRef.current === coordinator) {
        coordinatorRef.current = null;
      }
    };
  }, [enabled, setFrameVersion, setSong]);

  useEffect(() => {
    if (!enabled) return;

    const channel = getCdgSyncChannel();
    if (!channel) {
      return;
    }

    return startCdgSyncRequestListener({
      channel,
      getSnapshot: () => ({
        status: lastStatus,
        frame: lastFrame,
      }),
    });
  }, [enabled]);

  // Song detection: probe whether the new track has CDG graphics.
  // Shares the single coordinator with the hot path so probe + tick cannot
  // issue concurrent getCdgFrame calls.
  useEffect(() => {
    if (!enabled) return;

    if (!songId) {
      coordinatorRef.current?.invalidate();
      clear();
      clearFrame();
      emitCdgClear();
      emitCdgStatus(null, false);
      return;
    }

    if (!currentSongHasCdg) {
      coordinatorRef.current?.invalidate();
      clear();
      clearFrame();
      emitCdgClear();
      emitCdgStatus(songId, false);
      return;
    }

    const probePositionMs = selectSyncDisplayPositionMs(
      usePlayerStore.getState(),
    );
    const currentCdgSongId = useCdgStore.getState().songId;

    if (currentCdgSongId !== songId) {
      // Clear immediately on song change so the audience window cannot keep
      // showing the previous song while the new track's first frame arrives.
      setSong(songId, true);
      clearFrame();
      emitCdgClear();
      emitCdgStatus(songId, true);
    }

    coordinatorRef.current?.request({
      songId,
      transportGeneration,
      positionMs: probePositionMs,
      lastFrameVersion: 0,
    });
  }, [clear, currentSongHasCdg, enabled, setSong, songId, transportGeneration]);

  // RATIONALE: Do not replace this with setInterval/requestAnimationFrame.
  // The real regression was macOS throttling front-end scheduling in windows
  // that are heavily occluded by the audience display. Keeping the fetch loop
  // tied to Rust playback-position events is what preserves smooth CDG in both
  // windows.
  useEffect(() => {
    if (!enabled) return;

    const stopSync = startCdgPositionSync(
      () => {
        const state = usePlayerStore.getState();
        const { snapshot } = state;
        const { hasCdg, frameVersion } = useCdgStore.getState();

        if (!hasCdg || !snapshot?.is_playing) {
          return;
        }
        const positionMs = selectSyncDisplayPositionMs(state);
        const requestSongId = snapshot?.song_id;
        if (!requestSongId) return;
        const requestGeneration = snapshot?.transport_generation ?? 0;

        // PERF: The hot frame path stays out of React state. The IPC returns a
        // raw ArrayBuffer (no base64), and drawFrame() paints it to a pre-
        // allocated ImageData — no string decoding, no per-frame allocation.
        // Concurrency: shared coordinator serializes with the probe path.
        coordinatorRef.current?.request({
          songId: requestSongId,
          transportGeneration: requestGeneration,
          positionMs,
          lastFrameVersion: frameVersion,
        });
      },
      (listener) =>
        usePlayerStore.subscribe((state, previousState) => {
          listener(
            selectSyncDisplayPositionMs(state),
            selectSyncDisplayPositionMs(previousState),
          );
        }),
    );

    return () => {
      stopSync();
    };
  }, [enabled]);
}
