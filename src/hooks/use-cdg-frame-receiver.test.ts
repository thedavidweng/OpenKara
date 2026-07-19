import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type {
  CdgSyncChannel,
  CdgSyncFramePayload,
  CdgSyncStatusPayload,
} from "@/lib/cdg-sync-channel";

/** Helper: create a minimal frame payload for tests. */
function makeFramePayload(frameVersion: number = 1): CdgSyncFramePayload {
  return {
    rgba: new Uint8Array(4),
    frameVersion,
    transportGeneration: 1,
  };
}

describe("createCoalescingPainter", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  test("loads without importing the tauri display-frame polling API", async () => {
    vi.resetModules();
    vi.doMock("@/lib/tauri", () => {
      throw new Error("use-cdg-frame-receiver must stay message-driven");
    });

    const module = await import("./use-cdg-frame-receiver");

    expect(typeof module.createCoalescingPainter).toBe("function");
  });

  test("coalesces multiple frames into the latest macrotask paint", async () => {
    vi.resetModules();
    const { createCoalescingPainter } =
      await import("./use-cdg-frame-receiver");
    const paint = vi.fn();

    const painter = createCoalescingPainter<string>(paint);

    painter.enqueue("frame-1");
    painter.enqueue("frame-2");

    expect(paint).not.toHaveBeenCalled();

    vi.runAllTimers();

    expect(paint).toHaveBeenCalledTimes(1);
    expect(paint).toHaveBeenCalledWith("frame-2");
  });

  test("cancel clears the pending frame and timer", async () => {
    vi.resetModules();
    const { createCoalescingPainter } =
      await import("./use-cdg-frame-receiver");
    const paint = vi.fn();

    const painter = createCoalescingPainter<string>(paint);

    painter.enqueue("frame-1");
    painter.cancel();

    vi.runAllTimers();

    expect(paint).not.toHaveBeenCalled();
  });

  test("cancel is safe to call when nothing is enqueued", async () => {
    vi.resetModules();
    const { createCoalescingPainter } =
      await import("./use-cdg-frame-receiver");
    const paint = vi.fn();

    const painter = createCoalescingPainter<string>(paint);

    // Should not throw
    painter.cancel();

    expect(paint).not.toHaveBeenCalled();
  });

  test("double enqueue only paints the latest frame", async () => {
    vi.resetModules();
    const { createCoalescingPainter } =
      await import("./use-cdg-frame-receiver");
    const paint = vi.fn();

    const painter = createCoalescingPainter<number>(paint);

    painter.enqueue(1);
    painter.enqueue(2);
    painter.enqueue(3);

    vi.runAllTimers();

    expect(paint).toHaveBeenCalledTimes(1);
    expect(paint).toHaveBeenCalledWith(3);
  });

  test("enqueue after flush schedules a new paint", async () => {
    vi.resetModules();
    const { createCoalescingPainter } =
      await import("./use-cdg-frame-receiver");
    const paint = vi.fn();

    const painter = createCoalescingPainter<string>(paint);

    painter.enqueue("first");
    vi.runAllTimers();

    expect(paint).toHaveBeenCalledTimes(1);
    expect(paint).toHaveBeenCalledWith("first");

    painter.enqueue("second");
    vi.runAllTimers();

    expect(paint).toHaveBeenCalledTimes(2);
    expect(paint).toHaveBeenCalledWith("second");
  });

  test("enqueue after cancel schedules a new paint", async () => {
    vi.resetModules();
    const { createCoalescingPainter } =
      await import("./use-cdg-frame-receiver");
    const paint = vi.fn();

    const painter = createCoalescingPainter<string>(paint);

    painter.enqueue("cancelled");
    painter.cancel();

    painter.enqueue("new-frame");
    vi.runAllTimers();

    expect(paint).toHaveBeenCalledTimes(1);
    expect(paint).toHaveBeenCalledWith("new-frame");
  });
});

// ─── startCdgBroadcastFrameReceiver ─────────────────────────

describe("startCdgBroadcastFrameReceiver", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  test("onClear cancels pending coalesced frame and calls onClear", async () => {
    vi.resetModules();

    let capturedFrameHandler: ((payload: CdgSyncFramePayload) => void) | null =
      null;
    let capturedClearHandler: (() => void) | null = null;

    vi.doMock("@/lib/cdg-sync-channel", () => ({
      startCdgSyncReceiver: (opts: {
        onFrame: (payload: CdgSyncFramePayload) => void;
        onClear: () => void;
      }) => {
        capturedFrameHandler = opts.onFrame;
        capturedClearHandler = opts.onClear;
        return vi.fn();
      },
    }));

    const { startCdgBroadcastFrameReceiver } =
      await import("./use-cdg-frame-receiver");

    const onFrame = vi.fn();
    const onClear = vi.fn();
    const stop = startCdgBroadcastFrameReceiver({
      channel: {} as CdgSyncChannel,
      onFrame,
      onClear,
      onStatus: vi.fn(),
    });

    // Enqueue a frame
    capturedFrameHandler!(makeFramePayload());
    // Immediately clear before timer fires
    capturedClearHandler!();

    vi.runAllTimers();

    // Frame should have been cancelled by clear
    expect(onFrame).not.toHaveBeenCalled();
    expect(onClear).toHaveBeenCalledOnce();

    stop();
  });

  test("onStatus passes through to the caller", async () => {
    vi.resetModules();

    let capturedStatusHandler:
      | ((payload: CdgSyncStatusPayload) => void)
      | null = null;

    vi.doMock("@/lib/cdg-sync-channel", () => ({
      startCdgSyncReceiver: (opts: {
        onStatus: (payload: CdgSyncStatusPayload) => void;
      }) => {
        capturedStatusHandler = opts.onStatus;
        return vi.fn();
      },
    }));

    const { startCdgBroadcastFrameReceiver } =
      await import("./use-cdg-frame-receiver");

    const onStatus = vi.fn();
    const stop = startCdgBroadcastFrameReceiver({
      channel: {} as CdgSyncChannel,
      onFrame: vi.fn(),
      onClear: vi.fn(),
      onStatus,
    });

    const statusPayload: CdgSyncStatusPayload = {
      songId: "abc",
      hasCdg: true,
    };
    capturedStatusHandler!(statusPayload);

    expect(onStatus).toHaveBeenCalledWith(statusPayload);

    stop();
  });

  test("stop() cancels pending paint and stops the receiver", async () => {
    vi.resetModules();

    let capturedFrameHandler: ((payload: CdgSyncFramePayload) => void) | null =
      null;
    const stopReceiver = vi.fn();

    vi.doMock("@/lib/cdg-sync-channel", () => ({
      startCdgSyncReceiver: (opts: {
        onFrame: (payload: CdgSyncFramePayload) => void;
      }) => {
        capturedFrameHandler = opts.onFrame;
        return stopReceiver;
      },
    }));

    const { startCdgBroadcastFrameReceiver } =
      await import("./use-cdg-frame-receiver");

    const onFrame = vi.fn();
    const stop = startCdgBroadcastFrameReceiver({
      channel: {} as CdgSyncChannel,
      onFrame,
      onClear: vi.fn(),
      onStatus: vi.fn(),
    });

    // Enqueue a frame, then immediately stop
    capturedFrameHandler!(makeFramePayload());
    stop();

    vi.runAllTimers();

    // Frame should have been cancelled and receiver stopped
    expect(onFrame).not.toHaveBeenCalled();
    expect(stopReceiver).toHaveBeenCalledOnce();
  });

  test("frames coalesce correctly across clear then re-enqueue", async () => {
    vi.resetModules();

    let capturedFrameHandler: ((payload: CdgSyncFramePayload) => void) | null =
      null;
    let capturedClearHandler: (() => void) | null = null;

    vi.doMock("@/lib/cdg-sync-channel", () => ({
      startCdgSyncReceiver: (opts: {
        onFrame: (payload: CdgSyncFramePayload) => void;
        onClear: () => void;
      }) => {
        capturedFrameHandler = opts.onFrame;
        capturedClearHandler = opts.onClear;
        return vi.fn();
      },
    }));

    const { startCdgBroadcastFrameReceiver } =
      await import("./use-cdg-frame-receiver");

    const onFrame = vi.fn();
    const stop = startCdgBroadcastFrameReceiver({
      channel: {} as CdgSyncChannel,
      onFrame,
      onClear: vi.fn(),
      onStatus: vi.fn(),
    });

    // Enqueue, clear, then enqueue again
    const frame1 = makeFramePayload(1);
    const frame2 = makeFramePayload(2);
    capturedFrameHandler!(frame1);
    capturedClearHandler!();
    capturedFrameHandler!(frame2);

    vi.runAllTimers();

    // Only the second frame should paint (after clear)
    expect(onFrame).toHaveBeenCalledOnce();
    expect(onFrame).toHaveBeenCalledWith(frame2);

    stop();
  });
});
