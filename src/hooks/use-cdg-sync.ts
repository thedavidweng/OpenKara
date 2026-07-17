import { useEffect, useRef } from "react";
import {
  selectSyncDisplayPositionMs,
  usePlayerStore,
} from "@/stores/player-store";
import { useCdgStore } from "@/stores/cdg-store";
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
import type { CdgAvailability, CdgErrorCode } from "@/lib/tauri/cdg";
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
 *
 * Concurrency model — two independent staleness mechanisms:
 * - `serial` advances ONLY on `invalidate()` (song change / unmount cleanup),
 *   NOT on every position tick. It drops late results whose serial no longer
 *   matches, so an in-flight response arriving after an invalidate is ignored.
 * - `isCurrent(req)` checks the request against the live player snapshot
 *   (song id + transport generation) and drops results whose song/generation
 *   no longer matches, even when the serial is unchanged. This catches
 *   song/generation transitions that did not route through `invalidate()`.
 * Because `serial` is not bumped on `request()`, a newer request for the same
 * song does NOT invalidate an in-flight response — under slow IPC this avoids
 * dropping every frame when requests arrive faster than responses.
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
  /**
   * Enqueue a desired frame request. The request is coalesced if another
   * is already in flight — only the latest pending request is pumped when
   * the in-flight one completes. Returns `void`; callers cannot rely on a
   * return value to detect drops. Use `isInFlight()` for test assertions.
   */
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
  /**
   * Query the backend CDG status for a song/generation. Used to distinguish a
   * genuine audio-only song (availability "none") from a backend CDG error
   * state (empty/invalid/unreadable/broken ZIP) when the frame probe returns a
   * 0-byte response.
   */
  getCdgStatus: typeof api.getCdgStatus;
  onFrame: (args: {
    songId: string;
    transportGeneration: number;
    frameVersion: number;
    /** Protocol decoder yields `Uint8Array` (not ClampedArray). */
    rgba: Uint8Array;
  }) => void;
  onProbeResolved: (args: {
    songId: string;
    transportGeneration: number;
    /** Whether the backend has an active CDG slot for this song. */
    hasCdg: boolean;
    hasFrame: boolean;
    /**
     * Backend CDG availability, reported when the frame probe consulted
     * `getCdgStatus` (i.e. on a 0-byte response). Absent on the hasCdg=true
     * and IPC-failure fallback paths.
     */
    availability?: CdgAvailability;
    /**
     * Backend CDG error code, reported when `availability` is "error".
     */
    errorCode?: CdgErrorCode | null;
  }) => void;
  onError: (args: { songId: string; transportGeneration: number }) => void;
  /**
   * Is this request still relevant? Checks the request against the live player
   * snapshot (song id + transport generation) and returns false once the
   * song/generation has moved on. This is independent of `serial`: it catches
   * song/generation transitions that did not route through `invalidate()`,
   * while `serial` only handles invalidate-driven drops.
   */
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

    // Called when all follow-up work for this request is done. Clears the
    // in-flight flag and pumps the next coalesced request. Using a helper
    // (instead of .finally()) keeps inFlight true while a getCdgStatus
    // follow-up is outstanding, preserving the "at most one getCdgFrame in
    // flight" serialization invariant.
    const complete = () => {
      inFlight = false;
      // Drop if invalidated while in flight.
      if (pending && pending.serial !== serial) {
        pending = null;
      }
      pump();
    };

    deps
      .getCdgFrame(
        req.songId,
        req.transportGeneration,
        req.positionMs,
        req.lastFrameVersion,
      )
      .then((result) => {
        // #113: Check both the serial (for invalidate-driven drops) and
        // isCurrent (for song/generation changes). The serial is only
        // incremented by invalidate(), NOT by request(), so a newer request
        // for the same song does NOT cause the in-flight response to be
        // dropped. Under slow IPC this prevents every frame from being
        // dropped when requests arrive faster than responses.
        if (req.serial !== serial || !deps.isCurrent(req)) {
          complete();
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
            hasCdg: true,
            hasFrame: true,
          });
          complete();
        } else if (envelope) {
          // CDG is active but the caller already has the current frame
          // (header-only response, no RGBA payload).
          deps.onProbeResolved({
            songId: req.songId,
            transportGeneration: req.transportGeneration,
            hasCdg: true,
            hasFrame: false,
          });
          complete();
        } else {
          // 0-byte response: the backend has no active CDG decoder for this
          // song/generation. This covers three cases — a genuine audio-only
          // song, a stale song/generation, and a backend CDG error state
          // (empty/invalid/unreadable/broken ZIP). The frame probe alone
          // cannot distinguish them, so consult getCdgStatus and forward the
          // backend availability/errorCode to the UI. RATIONALE: the previous
          // code treated every 0-byte response as hasCdg=false, which silently
          // hid backend CDG errors as audio-only and never reported the
          // documented errorCode to the UI.
          deps
            .getCdgStatus(req.songId, req.transportGeneration)
            .then((status) => {
              if (req.serial !== serial || !deps.isCurrent(req)) {
                complete();
                return;
              }
              deps.onProbeResolved({
                songId: req.songId,
                transportGeneration: req.transportGeneration,
                hasCdg: false,
                hasFrame: false,
                availability: status.availability,
                errorCode: status.errorCode,
              });
              complete();
            })
            .catch(() => {
              // Status query failed — fall back to audio-only treatment so a
              // status IPC failure does not block the hot loop.
              if (req.serial !== serial || !deps.isCurrent(req)) {
                complete();
                return;
              }
              deps.onProbeResolved({
                songId: req.songId,
                transportGeneration: req.transportGeneration,
                hasCdg: false,
                hasFrame: false,
              });
              complete();
            });
        }
      })
      .catch(() => {
        if (req.serial !== serial || !deps.isCurrent(req)) {
          complete();
          return;
        }
        deps.onError({
          songId: req.songId,
          transportGeneration: req.transportGeneration,
        });
        complete();
      });
  };

  return {
    request: (partial) => {
      // #113: Do NOT increment serial on request — only on invalidate.
      // This ensures in-flight responses are not dropped when a newer
      // request for the same song is enqueued. The pending request
      // inherits the current serial, so it will be dropped if an
      // invalidate() occurs before it is pumped.
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
  const setSong = useCdgStore((s) => s.setSong);
  const setStatus = useCdgStore((s) => s.setStatus);
  const clear = useCdgStore((s) => s.clear);
  const setFrameVersion = useCdgStore((s) => s.setFrameVersion);

  const coordinatorRef = useRef<CdgFrameCoordinator | null>(null);

  // Build / rebuild coordinator when enabled; invalidate on cleanup.
  useEffect(() => {
    if (!enabled) {
      coordinatorRef.current = null;
      return;
    }

    const coordinator = createCdgFrameCoordinator({
      getCdgFrame: api.getCdgFrame,
      getCdgStatus: api.getCdgStatus,
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
      onProbeResolved: ({ songId: sid, hasCdg, availability, errorCode }) => {
        if (!hasCdg) {
          // No active CDG decoder for this song/generation. Forward the
          // backend availability/errorCode to the store so the UI can
          // surface error states (empty/invalid/unreadable/broken ZIP CDG)
          // instead of silently hiding them as audio-only. RATIONALE: the
          // previous code treated every 0-byte probe response as
          // hasCdg=false, so a song with a broken ZIP CDG was
          // indistinguishable from an audio-only track and the documented
          // errorCode was never reported to the UI. Only availability
          // "none" represents a genuine audio-only song; "error" carries an
          // errorCode the UI should display.
          setStatus(availability ?? "none", errorCode ?? null);
          setSong(sid, false);
          clearFrame();
          emitCdgClear();
          emitCdgStatus(sid, false);
          return;
        }
        // CDG is active; soft-confirm status without clearing an
        // already-drawn frame. Clear any stale error code left over from a
        // previous failed load so the UI does not keep showing it.
        setStatus("ready", null);
        emitCdgStatus(sid, true);
      },
      onError: ({ songId: sid }) => {
        setSong(sid, false);
        setStatus("none", null);
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
  }, [enabled, setFrameVersion, setSong, setStatus]);

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

    // RATIONALE: Do NOT skip the probe based on a frontend songHasCdgMedia()
    // check. The backend loads implicit sidecar CDG files via
    // audio_path.with_extension("cdg") when song.cdg_path is absent, so a
    // song with a colocated .cdg sidecar but no explicit cdg_path would be
    // wrongly cleared and reported hasCdg=false. Instead, always probe and
    // let the backend's 0-byte response determine CDG availability.
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
  }, [clear, enabled, setSong, songId, transportGeneration]);

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
