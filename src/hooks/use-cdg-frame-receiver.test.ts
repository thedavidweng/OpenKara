import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

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
