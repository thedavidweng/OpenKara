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
  const pendingRef = useRef(false);
  const currentSongHasCdg = songHasCdgMedia(currentSong);

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
  useEffect(() => {
    if (!enabled) return;

    if (!songId) {
      clear();
      clearFrame();
      emitCdgClear();
      emitCdgStatus(null, false);
      return;
    }

    if (!currentSongHasCdg) {
      clear();
      clearFrame();
      emitCdgClear();
      emitCdgStatus(songId, false);
      return;
    }

    let cancelled = false;
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

    api
      .getCdgFrame(songId, transportGeneration, probePositionMs, 0)
      .then((result) => {
        if (cancelled) return;
        const buffer = ensureArrayBuffer(result);
        const envelope = parseCdgFrameResponse(buffer);

        if (envelope && envelope.hasRgba && envelope.rgba) {
          setSong(songId, true);
          setFrameVersion(envelope.frameVersion, envelope.transportGeneration);
          drawFrame(envelope.rgba);
          emitCdgFrame({
            rgba: envelope.rgba,
            frameVersion: envelope.frameVersion,
            transportGeneration: envelope.transportGeneration,
          });
          emitCdgStatus(songId, true);
          return;
        }

        emitCdgStatus(songId, true);
      })
      .catch(() => {
        if (cancelled) return;
        setSong(songId, false);
        clearFrame();
        emitCdgStatus(songId, false);
      });

    return () => {
      cancelled = true;
    };
  }, [
    clear,
    currentSongHasCdg,
    enabled,
    setFrameVersion,
    setSong,
    songId,
    transportGeneration,
  ]);

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

        if (!hasCdg || !snapshot?.is_playing || pendingRef.current) {
          return;
        }
        pendingRef.current = true;
        const positionMs = selectSyncDisplayPositionMs(state);
        // F5: Capture songId and generation at request time so stale frames are discarded.
        const requestSongId = snapshot?.song_id;
        const requestGeneration = snapshot?.transport_generation ?? 0;

        // PERF: The hot frame path stays out of React state. The IPC returns a
        // raw ArrayBuffer (no base64), and drawFrame() paints it to a pre-
        // allocated ImageData — no string decoding, no per-frame allocation.
        api
          .getCdgFrame(
            requestSongId ?? "",
            requestGeneration,
            positionMs,
            frameVersion,
          )
          .then((result) => {
            // F5: Discard stale frames if the song or generation changed during the IPC call.
            const currentState = usePlayerStore.getState();
            if (
              requestSongId !== currentState.snapshot?.song_id ||
              requestGeneration !==
                (currentState.snapshot?.transport_generation ?? 0)
            ) {
              return;
            }
            const buffer = ensureArrayBuffer(result);
            const envelope = parseCdgFrameResponse(buffer);
            if (envelope && envelope.hasRgba && envelope.rgba) {
              setFrameVersion(
                envelope.frameVersion,
                envelope.transportGeneration,
              );
              drawFrame(envelope.rgba);
              emitCdgFrame({
                rgba: envelope.rgba,
                frameVersion: envelope.frameVersion,
                transportGeneration: envelope.transportGeneration,
              });
            }
          })
          .catch(() => {
            // Silently ignore CDG frame errors — non-critical for playback.
          })
          .finally(() => {
            pendingRef.current = false;
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
  }, [enabled, setFrameVersion]);
}
