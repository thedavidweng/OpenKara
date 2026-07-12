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

describe("PeakMeter", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockGetAudioPeaks.mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
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
    // The canvas backing store should have been sized (DPR-aware).
    expect(canvas.width).toBeGreaterThan(0);
    expect(canvas.height).toBeGreaterThan(0);
  });
});
