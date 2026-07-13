import { describe, expect, test, vi } from "vitest";
import { ensureArrayBuffer } from "@/lib/cdg-protocol";
import { getCdgSyncBucket, startCdgPositionSync } from "./use-cdg-sync";

describe("ensureArrayBuffer", () => {
  test("returns ArrayBuffer as-is", () => {
    const buf = new ArrayBuffer(4);
    expect(ensureArrayBuffer(buf)).toBe(buf);
  });

  test("converts Uint8Array view to a standalone ArrayBuffer", () => {
    const source = new Uint8Array([1, 2, 3, 4]);
    const result = ensureArrayBuffer(source);

    expect(result).toBeInstanceOf(ArrayBuffer);
    expect(result.byteLength).toBe(4);
    expect(new Uint8Array(result)).toEqual(new Uint8Array([1, 2, 3, 4]));
    // Must be a copy, not a view into the original buffer
    expect(result).not.toBe(source.buffer);
  });

  test("converts Uint8Array with byteOffset to a standalone ArrayBuffer", () => {
    const backing = new Uint8Array([0, 0, 1, 2, 3, 0, 0]);
    const view = new Uint8Array(backing.buffer, 2, 3); // [1, 2, 3]
    const result = ensureArrayBuffer(view);

    expect(result).toBeInstanceOf(ArrayBuffer);
    expect(result.byteLength).toBe(3);
    expect(new Uint8Array(result)).toEqual(new Uint8Array([1, 2, 3]));
  });

  test("converts number array to ArrayBuffer", () => {
    const result = ensureArrayBuffer([10, 20, 30]);

    expect(result).toBeInstanceOf(ArrayBuffer);
    expect(result.byteLength).toBe(3);
    expect(new Uint8Array(result)).toEqual(new Uint8Array([10, 20, 30]));
  });

  test("converts empty number array to empty ArrayBuffer", () => {
    const result = ensureArrayBuffer([]);

    expect(result).toBeInstanceOf(ArrayBuffer);
    expect(result.byteLength).toBe(0);
  });

  test("returns empty ArrayBuffer for null", () => {
    const result = ensureArrayBuffer(null);
    expect(result).toBeInstanceOf(ArrayBuffer);
    expect(result.byteLength).toBe(0);
  });

  test("returns empty ArrayBuffer for undefined", () => {
    const result = ensureArrayBuffer(undefined);
    expect(result).toBeInstanceOf(ArrayBuffer);
    expect(result.byteLength).toBe(0);
  });

  test("returns empty ArrayBuffer for a string", () => {
    const result = ensureArrayBuffer("hello");
    expect(result).toBeInstanceOf(ArrayBuffer);
    expect(result.byteLength).toBe(0);
  });

  test("returns empty ArrayBuffer for a plain object", () => {
    const result = ensureArrayBuffer({ foo: "bar" });
    expect(result).toBeInstanceOf(ArrayBuffer);
    expect(result.byteLength).toBe(0);
  });
});

describe("getCdgSyncBucket", () => {
  test("returns 0 for position 0", () => {
    expect(getCdgSyncBucket(0)).toBe(0);
  });

  test("returns 0 for positions within the first bucket (0-32ms)", () => {
    expect(getCdgSyncBucket(1)).toBe(0);
    expect(getCdgSyncBucket(16)).toBe(0);
    expect(getCdgSyncBucket(32)).toBe(0);
  });

  test("returns 1 for position at the bucket boundary (33ms)", () => {
    expect(getCdgSyncBucket(33)).toBe(1);
  });

  test("returns 1 for positions within the second bucket (33-65ms)", () => {
    expect(getCdgSyncBucket(34)).toBe(1);
    expect(getCdgSyncBucket(50)).toBe(1);
    expect(getCdgSyncBucket(65)).toBe(1);
  });

  test("returns 2 for position 66ms", () => {
    expect(getCdgSyncBucket(66)).toBe(2);
  });

  test("clamps negative positions to bucket 0", () => {
    expect(getCdgSyncBucket(-1)).toBe(0);
    expect(getCdgSyncBucket(-100)).toBe(0);
  });

  test("handles large positions correctly", () => {
    expect(getCdgSyncBucket(330)).toBe(10);
    expect(getCdgSyncBucket(3300)).toBe(100);
  });
});

describe("startCdgPositionSync", () => {
  test("ticks only when playback crosses a new CDG sync bucket", () => {
    const tick = vi.fn();
    let listener:
      | ((positionMs: number, previousPositionMs: number) => void)
      | null = null;

    const emitPosition = (positionMs: number, previousPositionMs: number) => {
      expect(listener).not.toBeNull();
      listener!(positionMs, previousPositionMs);
    };

    const stop = startCdgPositionSync(tick, (nextListener) => {
      listener = nextListener;
      return () => {
        listener = null;
      };
    });

    emitPosition(10, 0);
    emitPosition(20, 10);
    emitPosition(34, 20);
    emitPosition(40, 34);
    emitPosition(67, 40);

    expect(tick).toHaveBeenCalledTimes(2);

    stop();
    expect(listener).toBeNull();
  });

  test("does not tick when position stays within the same bucket", () => {
    const tick = vi.fn();
    let listener:
      | ((positionMs: number, previousPositionMs: number) => void)
      | null = null;

    const stop = startCdgPositionSync(tick, (nextListener) => {
      listener = nextListener;
      return () => {
        listener = null;
      };
    });

    listener!(5, 0);
    listener!(10, 5);
    listener!(20, 10);
    listener!(30, 20);

    expect(tick).not.toHaveBeenCalled();

    stop();
  });

  test("ticks on each bucket boundary crossing", () => {
    const tick = vi.fn();
    let listener:
      | ((positionMs: number, previousPositionMs: number) => void)
      | null = null;

    const stop = startCdgPositionSync(tick, (nextListener) => {
      listener = nextListener;
      return () => {
        listener = null;
      };
    });

    // bucket 0 -> 1
    listener!(33, 32);
    // bucket 1 -> 2
    listener!(66, 65);
    // bucket 2 -> 3
    listener!(99, 98);

    expect(tick).toHaveBeenCalledTimes(3);

    stop();
  });

  test("returns an unsubscribe function that stops ticking", () => {
    const tick = vi.fn();
    let listener:
      | ((positionMs: number, previousPositionMs: number) => void)
      | null = null;

    const stop = startCdgPositionSync(tick, (nextListener) => {
      listener = nextListener;
      return () => {
        listener = null;
      };
    });

    listener!(33, 0);
    expect(tick).toHaveBeenCalledTimes(1);

    stop();
    expect(listener).toBeNull();

    // Re-subscribe with a new listener after stop to verify cleanup worked
    // The old listener reference should be nulled out
  });
});

// ─── F5: CDG frame IPC songId/generation guard ─────────────────────────

describe("F5: CDG frame IPC validates against current song before drawFrame", () => {
  test("hot frame path captures songId and generation at request time and compares before draw", async () => {
    const { default: src } = await import("./use-cdg-sync.ts?raw");

    // In the hot frame path (the second useEffect that calls getCdgFrame
    // in a loop), the fix must:
    // 1. Capture the current songId and transport generation before the IPC call
    // 2. After the IPC resolves, compare against current songId and generation
    // 3. Skip drawFrame/emitCdgFrame if song or generation changed

    // Find the hot frame getCdgFrame call (the one inside startCdgPositionSync)
    const hotFrameSection = src.slice(src.indexOf("startCdgPositionSync"));

    // The songId and generation must be captured before getCdgFrame
    expect(hotFrameSection).toContain("requestSongId");
    expect(hotFrameSection).toContain("requestGeneration");

    // After the IPC resolves, there must be a songId and generation comparison
    const afterIpc = hotFrameSection.slice(
      hotFrameSection.indexOf(".getCdgFrame("),
    );

    // The guard should check that the current song and generation still match
    expect(afterIpc).toContain("snapshot?.song_id");
    expect(afterIpc).toContain("transport_generation");
  });
});
