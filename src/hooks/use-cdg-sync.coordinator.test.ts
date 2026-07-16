// @vitest-environment node

import { describe, expect, test, vi } from "vitest";
import { createCdgFrameCoordinator } from "./use-cdg-sync";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Minimal binary CDG envelope: 32-byte header with RGBA flag + 4 rgba bytes. */
function frameEnvelope(
  transportGeneration: number,
  frameVersion: number,
): ArrayBuffer {
  const header = new ArrayBuffer(32);
  const view = new DataView(header);
  view.setUint32(0, 0x43444746, true); // 'CDGF' magic-ish; parser may not check
  // Match parseCdgFrameResponse layout used in production.
  // We'll use the real parser expectations from cdg-protocol.
  const buf = new ArrayBuffer(32 + 4);
  const u8 = new Uint8Array(buf);
  // Magic / version fields — fill using the same helpers as other tests if needed.
  // For coordinator tests we can mock parse via returning empty if parse fails;
  // instead build using the test helper pattern from render tests.
  const dv = new DataView(buf);
  // protocol: see parseCdgFrameResponse — set hasRgba bit etc.
  // Simpler: mock getCdgFrame to return empty ArrayBuffer and only test concurrency.
  void transportGeneration;
  void frameVersion;
  void u8;
  void dv;
  return buf;
}

describe("createCdgFrameCoordinator", () => {
  test("serializes probe and tick: at most one getCdgFrame in flight", async () => {
    const first = deferred<ArrayBuffer>();
    const second = deferred<ArrayBuffer>();
    let calls = 0;
    const getCdgFrame = vi.fn((_sid, _gen, _pos, _ver) => {
      calls += 1;
      if (calls === 1) return first.promise;
      return second.promise;
    });

    const onFrame = vi.fn();
    const onProbeResolved = vi.fn();
    const onError = vi.fn();

    const coordinator = createCdgFrameCoordinator({
      getCdgFrame: getCdgFrame as never,
      onFrame,
      onProbeResolved,
      onError,
      isCurrent: () => true,
    });

    coordinator.request({
      songId: "song-1",
      transportGeneration: 1,
      positionMs: 0,
      lastFrameVersion: 0,
    });
    coordinator.request({
      songId: "song-1",
      transportGeneration: 1,
      positionMs: 100,
      lastFrameVersion: 1,
    });

    // Second request is coalesced; only one IPC while first is pending.
    expect(getCdgFrame).toHaveBeenCalledTimes(1);
    expect(coordinator.isInFlight()).toBe(true);

    first.resolve(new ArrayBuffer(0));
    // Drain promise microtasks: then → finally → pump → second getCdgFrame.
    for (let i = 0; i < 10; i++) {
      await Promise.resolve();
    }

    // After first completes, coalesced second is issued.
    expect(getCdgFrame).toHaveBeenCalledTimes(2);

    second.resolve(new ArrayBuffer(0));
    for (let i = 0; i < 10; i++) {
      await Promise.resolve();
    }
    expect(coordinator.isInFlight()).toBe(false);
  });

  test("late response after invalidate is ignored", async () => {
    const pending = deferred<ArrayBuffer>();
    const getCdgFrame = vi.fn(() => pending.promise);
    const onFrame = vi.fn();
    const onProbeResolved = vi.fn();
    const onError = vi.fn();

    const coordinator = createCdgFrameCoordinator({
      getCdgFrame: getCdgFrame as never,
      onFrame,
      onProbeResolved,
      onError,
      isCurrent: () => true,
    });

    coordinator.request({
      songId: "song-1",
      transportGeneration: 1,
      positionMs: 0,
      lastFrameVersion: 0,
    });
    const serialAtRequest = coordinator.currentSerial();
    coordinator.invalidate();
    expect(coordinator.currentSerial()).toBeGreaterThan(serialAtRequest);

    pending.resolve(new ArrayBuffer(0));
    await Promise.resolve();
    await Promise.resolve();

    expect(onFrame).not.toHaveBeenCalled();
    expect(onProbeResolved).not.toHaveBeenCalled();
    expect(onError).not.toHaveBeenCalled();
  });

  test("stale reverse-order resolve: older request does not apply after newer serial", async () => {
    const older = deferred<ArrayBuffer>();
    const newer = deferred<ArrayBuffer>();
    let n = 0;
    const getCdgFrame = vi.fn(() => {
      n += 1;
      return n === 1 ? older.promise : newer.promise;
    });
    const onProbeResolved = vi.fn();
    const onError = vi.fn();
    const onFrame = vi.fn();

    const coordinator = createCdgFrameCoordinator({
      getCdgFrame: getCdgFrame as never,
      onFrame,
      onProbeResolved,
      onError,
      isCurrent: () => true,
    });

    coordinator.request({
      songId: "song-1",
      transportGeneration: 1,
      positionMs: 0,
      lastFrameVersion: 0,
    });
    // Force a new serial before older resolves by invalidating mid-flight,
    // then issue a new request.
    coordinator.invalidate();
    coordinator.request({
      songId: "song-2",
      transportGeneration: 2,
      positionMs: 50,
      lastFrameVersion: 0,
    });

    // Resolve older first (stale).
    older.resolve(new ArrayBuffer(0));
    await Promise.resolve();
    await Promise.resolve();
    expect(onProbeResolved).not.toHaveBeenCalled();

    newer.resolve(new ArrayBuffer(0));
    await Promise.resolve();
    await Promise.resolve();
    // Newer may resolve with empty frame → onProbeResolved once.
    expect(onProbeResolved).toHaveBeenCalledTimes(1);
    expect(onProbeResolved).toHaveBeenCalledWith({
      songId: "song-2",
      transportGeneration: 2,
      hasCdg: false,
      hasFrame: false,
    });
  });

  test("isCurrent false drops result even with matching serial", async () => {
    const pending = deferred<ArrayBuffer>();
    const getCdgFrame = vi.fn(() => pending.promise);
    const onProbeResolved = vi.fn();
    let current = true;

    const coordinator = createCdgFrameCoordinator({
      getCdgFrame: getCdgFrame as never,
      onFrame: vi.fn(),
      onProbeResolved,
      onError: vi.fn(),
      isCurrent: () => current,
    });

    coordinator.request({
      songId: "song-1",
      transportGeneration: 1,
      positionMs: 0,
      lastFrameVersion: 0,
    });
    current = false;
    pending.resolve(new ArrayBuffer(0));
    await Promise.resolve();
    await Promise.resolve();
    expect(onProbeResolved).not.toHaveBeenCalled();
  });
});

// silence unused helper in this file
void frameEnvelope;
