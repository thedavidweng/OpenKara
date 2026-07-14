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
    // The flat-line fallback should have triggered additional redraws
    // (clearRect + fillRect for the baseline).
    expect(canvasMock.ctx.clearRect.mock.calls.length).toBeGreaterThan(
      initialCount,
    );
    expect(canvasMock.ctx.fillRect).toHaveBeenCalled();
  });
});
