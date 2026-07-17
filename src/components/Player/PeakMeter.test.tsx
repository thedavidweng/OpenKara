// @vitest-environment jsdom

import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render } from "@testing-library/react";

// Mock the Tauri invoke bridge before importing the component.
const mockGetAudioPeaks = vi.fn();
vi.mock("@/lib/tauri/playback", () => ({
  getAudioPeaks: () => mockGetAudioPeaks(),
}));

import { PeakMeter } from "./PeakMeter";

function getCanvas(container: HTMLElement): HTMLCanvasElement {
  const canvas = container.querySelector("[data-peak-meter]");
  if (!canvas) throw new Error("canvas not found");
  return canvas as HTMLCanvasElement;
}

// Mock canvas 2D context so the drawing path is exercised in jsdom.
function mockCanvasContext() {
  const ctx = {
    setTransform: vi.fn(),
    clearRect: vi.fn(),
    fillRect: vi.fn(),
    fillStyle: "",
    canvas: null as HTMLCanvasElement | null,
  };
  const origGetContext = HTMLCanvasElement.prototype.getContext;
  const getContextMock = vi.fn().mockReturnValue(ctx);
  HTMLCanvasElement.prototype.getContext =
    getContextMock as typeof HTMLCanvasElement.prototype.getContext;
  return {
    ctx,
    restore: () => {
      HTMLCanvasElement.prototype.getContext = origGetContext;
    },
  };
}

describe("PeakMeter", () => {
  let canvasMock: ReturnType<typeof mockCanvasContext>;

  beforeEach(() => {
    vi.useFakeTimers();
    mockGetAudioPeaks.mockReset();
    canvasMock = mockCanvasContext();
  });

  afterEach(() => {
    vi.useRealTimers();
    canvasMock.restore();
  });

  it("renders a canvas element with data-peak-meter attribute", () => {
    mockGetAudioPeaks.mockResolvedValue({ writeIndex: 0, peaks: [] });
    const { container } = render(<PeakMeter width={120} height={24} />);
    const canvas = getCanvas(container);
    expect(canvas).toBeTruthy();
    expect(canvas.tagName).toBe("CANVAS");
  });

  it("renders with default dimensions", () => {
    mockGetAudioPeaks.mockResolvedValue({ writeIndex: 0, peaks: [] });
    const { container } = render(<PeakMeter />);
    const canvas = getCanvas(container);
    expect(canvas.style.width).toBe("240px");
    expect(canvas.style.height).toBe("40px");
  });

  it("polls getAudioPeaks on mount", async () => {
    mockGetAudioPeaks.mockResolvedValue({ writeIndex: 0, peaks: [] });
    render(<PeakMeter />);
    await vi.advanceTimersByTimeAsync(100);
    expect(mockGetAudioPeaks).toHaveBeenCalled();
  });

  it("continues polling at 30 Hz", async () => {
    mockGetAudioPeaks.mockResolvedValue({ writeIndex: 0, peaks: [] });
    render(<PeakMeter />);
    await vi.advanceTimersByTimeAsync(100);
    const initialCount = mockGetAudioPeaks.mock.calls.length;
    await vi.advanceTimersByTimeAsync(34);
    expect(mockGetAudioPeaks.mock.calls.length).toBeGreaterThan(initialCount);
  });

  it("handles backend errors gracefully", async () => {
    mockGetAudioPeaks.mockRejectedValue(new Error("backend unavailable"));
    render(<PeakMeter />);
    await vi.advanceTimersByTimeAsync(100);
    expect(mockGetAudioPeaks).toHaveBeenCalled();
  });

  it("draws peaks when data is available", async () => {
    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 1,
      peaks: [[0.5, 0.7]],
    });
    const { container } = render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(100);
    const canvas = getCanvas(container);
    expect(canvas.width).toBeGreaterThan(0);
    expect(canvas.height).toBeGreaterThan(0);
    // The drawing path should have called canvas context methods.
    expect(canvasMock.ctx.fillRect).toHaveBeenCalled();
  });

  it("draws flat baseline when no peaks are available", async () => {
    mockGetAudioPeaks.mockResolvedValue({ writeIndex: 0, peaks: [] });
    render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(100);
    // Even with no peaks, the baseline fillRect should be called.
    expect(canvasMock.ctx.fillRect).toHaveBeenCalled();
  });

  it("skips redraw when writeIndex has not changed", async () => {
    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 5,
      peaks: [[0.3, 0.4]],
    });
    render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(100);
    const firstCallCount = canvasMock.ctx.fillRect.mock.calls.length;
    // Advance past one poll cycle — writeIndex is the same so no redraw.
    await vi.advanceTimersByTimeAsync(34);
    const secondCallCount = canvasMock.ctx.fillRect.mock.calls.length;
    expect(secondCallCount).toBe(firstCallCount);
  });

  it("redraws when writeIndex changes", async () => {
    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 5,
      peaks: [[0.3, 0.4]],
    });
    render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(100);
    const firstCallCount = canvasMock.ctx.fillRect.mock.calls.length;
    // Change the mock to return a new writeIndex.
    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 6,
      peaks: [
        [0.3, 0.4],
        [0.5, 0.6],
      ],
    });
    await vi.advanceTimersByTimeAsync(34);
    const secondCallCount = canvasMock.ctx.fillRect.mock.calls.length;
    expect(secondCallCount).toBeGreaterThan(firstCallCount);
  });

  it("renders multiple peak bars", async () => {
    const peaks: Array<[number, number]> = [];
    for (let i = 0; i < 10; i++) {
      peaks.push([i * 0.1, 1.0 - i * 0.1]);
    }
    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 10,
      peaks,
    });
    render(<PeakMeter width={300} height={40} />);
    await vi.advanceTimersByTimeAsync(100);
    // Each peak pair produces 2 fillRect calls (left + right).
    expect(canvasMock.ctx.fillRect.mock.calls.length).toBeGreaterThanOrEqual(
      20,
    );
  });

  it("cleans up timer and cancels polling on unmount", async () => {
    mockGetAudioPeaks.mockResolvedValue({ writeIndex: 0, peaks: [] });
    const { unmount } = render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(100);
    const callsBefore = mockGetAudioPeaks.mock.calls.length;
    unmount();
    // Advance past several poll cycles — no more calls should happen.
    await vi.advanceTimersByTimeAsync(200);
    expect(mockGetAudioPeaks.mock.calls.length).toBe(callsBefore);
  });

  it("coalesces rapid ticks into at most one concurrent IPC call", async () => {
    // A slow backend: every getAudioPeaks() stays pending until we resolve it.
    type Resolver = (value: {
      writeIndex: number;
      peaks: Array<[number, number]>;
    }) => void;
    const resolvers: Resolver[] = [];
    mockGetAudioPeaks.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolvers.push(resolve);
        }),
    );

    render(<PeakMeter width={120} height={24} />);
    // Mount poll starts one in-flight call.
    await vi.advanceTimersByTimeAsync(0);
    expect(resolvers.length).toBe(1);

    // Advance several timer ticks while the first call is still unresolved.
    // The single-flight guard must coalesce these into rerunRequested, not
    // start additional concurrent IPC calls.
    await vi.advanceTimersByTimeAsync(34);
    await vi.advanceTimersByTimeAsync(34);
    await vi.advanceTimersByTimeAsync(34);
    expect(resolvers.length).toBe(1);

    // Resolving the in-flight call clears it; the coalesced rerun fires once.
    resolvers[0]!({ writeIndex: 1, peaks: [[0.5, 0.5]] });
    await vi.advanceTimersByTimeAsync(0);
    expect(resolvers.length).toBe(2);

    // Further ticks while the follow-up is in flight again coalesce.
    await vi.advanceTimersByTimeAsync(34);
    await vi.advanceTimersByTimeAsync(34);
    expect(resolvers.length).toBe(2);
  });

  it("draws once for a delayed response rather than perpetually discarding it", async () => {
    // When IPC takes longer than a tick, the single follow-up poll must still
    // draw its result once — it is not invalidated by a newer tick because no
    // newer tick starts a concurrent request.
    type Resolver = (value: {
      writeIndex: number;
      peaks: Array<[number, number]>;
    }) => void;
    const resolvers: Resolver[] = [];
    mockGetAudioPeaks.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolvers.push(resolve);
        }),
    );

    render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(0);
    expect(resolvers.length).toBe(1);

    // Ticks arrive while the call is pending — coalesced, no new calls.
    await vi.advanceTimersByTimeAsync(34);
    await vi.advanceTimersByTimeAsync(34);
    expect(resolvers.length).toBe(1);

    const beforeDraw = canvasMock.ctx.fillRect.mock.calls.length;
    // The delayed response resolves and must draw (writeIndex advanced).
    resolvers[0]!({ writeIndex: 7, peaks: [[0.8, 0.8]] });
    await vi.advanceTimersByTimeAsync(0);
    expect(canvasMock.ctx.fillRect.mock.calls.length).toBeGreaterThan(
      beforeDraw,
    );
  });

  it("does not draw a response that resolves after unmount", async () => {
    type Resolver = (value: {
      writeIndex: number;
      peaks: Array<[number, number]>;
    }) => void;
    const resolvers: Resolver[] = [];
    mockGetAudioPeaks.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolvers.push(resolve);
        }),
    );

    const { unmount } = render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(0);
    expect(resolvers.length).toBe(1);

    unmount();
    const beforeDraw = canvasMock.ctx.fillRect.mock.calls.length;

    // A late resolution after unmount must not draw.
    resolvers[0]!({ writeIndex: 9, peaks: [[0.9, 0.9]] });
    await vi.advanceTimersByTimeAsync(0);
    await Promise.resolve();
    await Promise.resolve();
    expect(canvasMock.ctx.fillRect.mock.calls.length).toBe(beforeDraw);
  });

  it("an older write index cannot replace a newer canvas state", async () => {
    // The freshness predicate is monotonic (>): a response whose writeIndex is
    // less than the last drawn index must not redraw, even if it is the active
    // generation (e.g. a reordered or wrapped backend response).
    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 10,
      peaks: [[0.5, 0.5]],
    });
    render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(100);
    const drawnAt10 = canvasMock.ctx.fillRect.mock.calls.length;

    // A subsequent poll returns an older writeIndex — must not redraw.
    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 3,
      peaks: [[0.1, 0.1]],
    });
    await vi.advanceTimersByTimeAsync(34);
    expect(canvasMock.ctx.fillRect.mock.calls.length).toBe(drawnAt10);
  });

  it("falls back to flat-line when peaks go stale after playback stops", async () => {
    // Simulate playback that was active (writeIndex > 0, non-empty peaks) but
    // has now stopped — the backend keeps returning the same snapshot.
    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 5,
      peaks: [[0.3, 0.4]],
    });
    render(<PeakMeter width={120} height={24} />);
    // Let the initial poll land and record the advance time.
    await vi.advanceTimersByTimeAsync(100);
    const initialCount = canvasMock.ctx.clearRect.mock.calls.length;
    // Advance well past the 500 ms staleness grace period.
    await vi.advanceTimersByTimeAsync(600);
    // The flat-line fallback should have drawn exactly once more
    // (clearRect + fillRect for the baseline), not on every subsequent tick.
    const afterFlatLine = canvasMock.ctx.clearRect.mock.calls.length;
    expect(afterFlatLine).toBeGreaterThan(initialCount);
    expect(canvasMock.ctx.fillRect).toHaveBeenCalled();

    // Advance further — no additional redraws should occur while idle.
    await vi.advanceTimersByTimeAsync(1000);
    expect(canvasMock.ctx.clearRect.mock.calls.length).toBe(afterFlatLine);
  });
});
