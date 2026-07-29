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

export type CdgFrameRequest = {
  songId: string;
  transportGeneration: number;
  positionMs: number;
  lastFrameVersion: number;
  serial: number;
};

export type CdgFrameCoordinator = {
  request: (req: Omit<CdgFrameRequest, "serial">) => void;
  invalidate: () => void;
  isInFlight: () => boolean;
  currentSerial: () => number;
};

export function createCdgFrameCoordinator(deps: {
  getCdgFrame: typeof api.getCdgFrame;
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
    hasCdg: boolean;
    hasFrame: boolean;
    availability?: CdgAvailability;
    errorCode?: CdgErrorCode | null;
  }) => void;
  onError: (args: { songId: string; transportGeneration: number }) => void;
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

    const complete = () => {
      inFlight = false;
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
          deps.onProbeResolved({
            songId: req.songId,
            transportGeneration: req.transportGeneration,
            hasCdg: true,
            hasFrame: false,
          });
          complete();
        } else {
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
        if (
          useCdgStore.getState().songId !== sid ||
          !useCdgStore.getState().hasCdg
        ) {
          setSong(sid, true);
        }
      },
      onProbeResolved: ({ songId: sid, hasCdg, availability, errorCode }) => {
        if (!hasCdg) {
          if (availability === "loading") {
            setStatus("loading", null);
            return;
          }
          setStatus(availability ?? "none", errorCode ?? null);
          setSong(sid, false);
          clearFrame();
          emitCdgClear();
          emitCdgStatus(sid, false);
          return;
        }
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

    const probePositionMs = selectSyncDisplayPositionMs(
      usePlayerStore.getState(),
    );
    const currentCdgSongId = useCdgStore.getState().songId;

    if (currentCdgSongId !== songId) {
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
