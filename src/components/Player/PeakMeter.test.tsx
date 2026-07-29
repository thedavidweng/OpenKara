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
    expect(canvasMock.ctx.fillRect).toHaveBeenCalled();
  });

  it("draws flat baseline when no peaks are available", async () => {
    mockGetAudioPeaks.mockResolvedValue({ writeIndex: 0, peaks: [] });
    render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(100);
    expect(canvasMock.ctx.fillRect).toHaveBeenCalled();
  });

  it("skips redraw when writeIndex has not changed within the grace period", async () => {
    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 5,
      peaks: [[0.3, 0.4]],
    });
    render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(100);
    const firstCallCount = canvasMock.ctx.fillRect.mock.calls.length;
    await vi.advanceTimersByTimeAsync(34);
    const secondCallCount = canvasMock.ctx.fillRect.mock.calls.length;
    expect(secondCallCount).toBe(firstCallCount);
  });

  it("falls back to flat baseline when writeIndex freezes with non-empty peaks", async () => {
    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 5,
      peaks: [[0.5, 0.5]],
    });
    render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(100);
    canvasMock.ctx.fillRect.mockClear();
    canvasMock.ctx.clearRect.mockClear();

    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 5,
      peaks: [[0.5, 0.5]],
    });
    await vi.advanceTimersByTimeAsync(600);
    expect(canvasMock.ctx.clearRect).toHaveBeenCalled();
    expect(canvasMock.ctx.fillRect).toHaveBeenCalled();
  });

  it("redraws when writeIndex changes", async () => {
    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 5,
      peaks: [[0.3, 0.4]],
    });
    render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(100);
    const firstCallCount = canvasMock.ctx.fillRect.mock.calls.length;
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
    await vi.advanceTimersByTimeAsync(200);
    expect(mockGetAudioPeaks.mock.calls.length).toBe(callsBefore);
  });

  it("coalesces rapid ticks into at most one concurrent IPC call", async () => {
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

    await vi.advanceTimersByTimeAsync(34);
    await vi.advanceTimersByTimeAsync(34);
    await vi.advanceTimersByTimeAsync(34);
    expect(resolvers.length).toBe(1);

    resolvers[0]!({ writeIndex: 1, peaks: [[0.5, 0.5]] });
    await vi.advanceTimersByTimeAsync(0);
    expect(resolvers.length).toBe(2);

    await vi.advanceTimersByTimeAsync(34);
    await vi.advanceTimersByTimeAsync(34);
    expect(resolvers.length).toBe(2);
  });

  it("draws once for a delayed response rather than perpetually discarding it", async () => {
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

    await vi.advanceTimersByTimeAsync(34);
    await vi.advanceTimersByTimeAsync(34);
    expect(resolvers.length).toBe(1);

    const beforeDraw = canvasMock.ctx.fillRect.mock.calls.length;
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

    resolvers[0]!({ writeIndex: 9, peaks: [[0.9, 0.9]] });
    await vi.advanceTimersByTimeAsync(0);
    await Promise.resolve();
    await Promise.resolve();
    expect(canvasMock.ctx.fillRect.mock.calls.length).toBe(beforeDraw);
  });

  it("an older write index cannot replace a newer canvas state", async () => {
    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 10,
      peaks: [[0.5, 0.5]],
    });
    render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(100);
    const drawnAt10 = canvasMock.ctx.fillRect.mock.calls.length;

    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 3,
      peaks: [[0.1, 0.1]],
    });
    await vi.advanceTimersByTimeAsync(34);
    expect(canvasMock.ctx.fillRect.mock.calls.length).toBe(drawnAt10);
  });

  it("falls back to flat-line when peaks go stale after playback stops", async () => {
    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 5,
      peaks: [[0.3, 0.4]],
    });
    render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(100);
    const initialCount = canvasMock.ctx.clearRect.mock.calls.length;
    await vi.advanceTimersByTimeAsync(1000);
    const afterFlatLine = canvasMock.ctx.clearRect.mock.calls.length;
    expect(afterFlatLine).toBeGreaterThan(initialCount);
    expect(canvasMock.ctx.fillRect).toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1000);
    expect(canvasMock.ctx.clearRect.mock.calls.length).toBe(afterFlatLine);
  });

  it("flat-lines when mounted with non-empty peaks whose writeIndex never advances", async () => {
    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 0,
      peaks: [[0.3, 0.4]],
    });
    render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(100);
    const initialCount = canvasMock.ctx.clearRect.mock.calls.length;
    await vi.advanceTimersByTimeAsync(1000);
    const afterFlatLine = canvasMock.ctx.clearRect.mock.calls.length;
    expect(afterFlatLine).toBeGreaterThan(initialCount);
    expect(canvasMock.ctx.fillRect).toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1000);
    expect(canvasMock.ctx.clearRect.mock.calls.length).toBe(afterFlatLine);
  });

  it("animates a smooth decay to flat-line instead of an abrupt jump", async () => {
    mockGetAudioPeaks.mockResolvedValue({
      writeIndex: 5,
      peaks: [[0.8, 0.8]],
    });
    render(<PeakMeter width={120} height={24} />);
    await vi.advanceTimersByTimeAsync(100);
    const afterInitial = canvasMock.ctx.clearRect.mock.calls.length;

    await vi.advanceTimersByTimeAsync(500);
    const duringDecay = canvasMock.ctx.clearRect.mock.calls.length;
    expect(duringDecay).toBeGreaterThan(afterInitial);

    await vi.advanceTimersByTimeAsync(500);
    const afterFlatLine = canvasMock.ctx.clearRect.mock.calls.length;
    expect(afterFlatLine).toBeGreaterThan(duringDecay);

    await vi.advanceTimersByTimeAsync(1000);
    expect(canvasMock.ctx.clearRect.mock.calls.length).toBe(afterFlatLine);
  });
});
