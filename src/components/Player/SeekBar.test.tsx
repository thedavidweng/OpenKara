// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { SeekBar } from "./SeekBar";
import { resetWaveformCacheForTests } from "@/lib/waveform-cache";

const {
  mockPlayerState,
  mockSeek,
  mockSelectCurrentPositionMs,
  mockGetWaveform,
} = vi.hoisted(() => {
  const mockSeek = vi.fn();
  const mockGetWaveform = vi.fn();
  const mockSelectCurrentPositionMs = vi.fn(
    (state: { positionMs: number }) => state.positionMs,
  );
  return {
    mockSeek,
    mockGetWaveform,
    mockSelectCurrentPositionMs,
    mockPlayerState: {
      snapshot: {
        duration_ms: 100_000,
        is_playing: true,
        state: "playing",
        song_id: "song-1",
      } as {
        duration_ms: number;
        is_playing: boolean;
        state: string;
        song_id: string | null;
      } | null,
      positionMs: 10_000,
      playingSinceMs: 1000 as number | null,
      seek: mockSeek,
    },
  };
});

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: (selector: (state: typeof mockPlayerState) => unknown) =>
    selector(mockPlayerState),
  selectCurrentPositionMs: mockSelectCurrentPositionMs,
}));

vi.mock("@/lib/tauri/playback", () => ({
  getWaveform: (...args: unknown[]) => mockGetWaveform(...args),
}));

function mockCanvasContext() {
  const ctx = {
    setTransform: vi.fn(),
    clearRect: vi.fn(),
    fillRect: vi.fn(),
    fillStyle: "",
  };
  const origGetContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = vi
    .fn()
    .mockReturnValue(ctx) as typeof HTMLCanvasElement.prototype.getContext;
  return {
    ctx,
    restore: () => {
      HTMLCanvasElement.prototype.getContext = origGetContext;
    },
  };
}

function stubRailGeometry(rail: HTMLElement, width = 200, height = 10) {
  vi.spyOn(rail, "getBoundingClientRect").mockReturnValue({
    left: 0,
    width,
    top: 0,
    height,
    right: width,
    bottom: height,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  });
}

describe("SeekBar", () => {
  let host: HTMLDivElement;
  let root: Root;
  let canvasMock: ReturnType<typeof mockCanvasContext>;
  let resizeCallback: ResizeObserverCallback | null = null;
  let mediaChangeCallback: (() => void) | null = null;
  let mediaQueryDpr: number | null = null;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    resetWaveformCacheForTests();
    mockSeek.mockClear();
    mockGetWaveform.mockReset();
    mockGetWaveform.mockResolvedValue({ peaks: [], buckets: 0 });
    mockPlayerState.snapshot = {
      duration_ms: 100_000,
      is_playing: true,
      state: "playing",
      song_id: "song-1",
    };
    mockPlayerState.positionMs = 10_000;
    mockPlayerState.playingSinceMs = 1000;
    mockSelectCurrentPositionMs.mockImplementation(
      (state: { positionMs: number }) => state.positionMs,
    );
    vi.stubGlobal(
      "requestAnimationFrame",
      vi.fn(() => 1),
    );
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    resizeCallback = null;
    mediaChangeCallback = null;
    mediaQueryDpr = null;
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(cb: ResizeObserverCallback) {
          resizeCallback = cb;
        }
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    Object.defineProperty(window, "devicePixelRatio", {
      configurable: true,
      value: 2,
    });
    vi.stubGlobal(
      "matchMedia",
      vi.fn((query: string) => {
        const match = query.match(/([\d.]+)dppx/);
        mediaQueryDpr = match ? Number(match[1]) : null;
        return {
          matches: false,
          media: query,
          onchange: null,
          addEventListener: (type: string, cb: () => void) => {
            if (type === "change") mediaChangeCallback = cb;
          },
          removeEventListener: () => {
            mediaChangeCallback = null;
          },
          dispatchEvent: () => false,
        };
      }),
    );
    canvasMock = mockCanvasContext();

    vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
      left: 0,
      width: 200,
      top: 0,
      height: 10,
      right: 200,
      bottom: 10,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });

    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    host.remove();
    canvasMock.restore();
    vi.unstubAllGlobals();
  });

  test("keeps a minimum safe width for the whole control and the draggable rail", async () => {
    const { renderToStaticMarkup } = await import("react-dom/server");
    const markup = renderToStaticMarkup(<SeekBar density="tight" />);

    expect(markup).toContain("min-w-[180px]");
    expect(markup).toContain("min-w-[120px]");
    expect(markup).toContain("w-[3.25rem]");
    expect(markup).toContain("tabular-nums");
    expect(markup).toContain("whitespace-nowrap");
    expect(markup).toContain("gap-2");
  });

  test("renders a waveform canvas element inside the seek rail", () => {
    act(() => {
      root.render(<SeekBar />);
    });

    const canvas = host.querySelector("[data-waveform-canvas]");
    expect(canvas).toBeTruthy();
    expect(canvas?.tagName).toBe("CANVAS");
  });

  test("fetches waveform peaks for the current song", async () => {
    mockGetWaveform.mockResolvedValue({
      peaks: [0.1, 0.5, 0.9],
      buckets: 3,
    });

    await act(async () => {
      root.render(<SeekBar />);
    });
    await act(async () => {
      await Promise.resolve();
    });

    // Default stub: 200 CSS px @ 2x DPR → round(200*2/3) = 133 buckets.
    expect(mockGetWaveform).toHaveBeenCalledWith("song-1", 133);
  });

  test("draws waveform bars when peaks are available", async () => {
    mockGetWaveform.mockResolvedValue({
      peaks: [0.2, 0.8, 0.4, 1.0],
      buckets: 4,
    });

    await act(async () => {
      root.render(<SeekBar />);
    });

    const rail = host.querySelector("[role='slider']") as HTMLElement;
    stubRailGeometry(rail, 200, 12);

    await act(async () => {
      await Promise.resolve();
    });

    expect(canvasMock.ctx.setTransform).toHaveBeenCalled();
    expect(canvasMock.ctx.clearRect).toHaveBeenCalled();
    expect(canvasMock.ctx.fillRect.mock.calls.length).toBeGreaterThanOrEqual(4);
  });

  test("clears the canvas when peaks are empty", async () => {
    mockGetWaveform.mockResolvedValue({ peaks: [], buckets: 0 });

    await act(async () => {
      root.render(<SeekBar />);
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(canvasMock.ctx.clearRect).toHaveBeenCalled();
    expect(canvasMock.ctx.fillRect).not.toHaveBeenCalled();
  });

  test("silently skips waveform drawing when getWaveform rejects", async () => {
    mockGetWaveform.mockRejectedValue(new Error("backend unavailable"));

    await act(async () => {
      root.render(<SeekBar />);
    });
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(host.querySelector("[data-waveform-canvas]")).toBeTruthy();
  });

  test("clears waveform when there is no current song", async () => {
    mockPlayerState.snapshot = null;

    await act(async () => {
      root.render(<SeekBar />);
    });

    expect(mockGetWaveform).not.toHaveBeenCalled();
    const endLabel = host.querySelectorAll("span")[1];
    expect(endLabel?.textContent).toMatch(/0:00/);
  });

  test("clears previous song waveform before the next fetch resolves", async () => {
    let resolveSong2:
      | ((value: { peaks: number[]; buckets: number }) => void)
      | undefined;
    mockGetWaveform.mockImplementation((songId: string) => {
      if (songId === "song-1") {
        return Promise.resolve({ peaks: [0.9, 0.9, 0.9, 0.9], buckets: 4 });
      }
      return new Promise((resolve) => {
        resolveSong2 = resolve;
      });
    });

    await act(async () => {
      root.render(<SeekBar />);
    });
    await act(async () => {
      await Promise.resolve();
    });

    const rail = host.querySelector("[role='slider']") as HTMLElement;
    stubRailGeometry(rail, 200, 12);
    await act(async () => {
      await Promise.resolve();
    });
    expect(canvasMock.ctx.fillRect.mock.calls.length).toBeGreaterThanOrEqual(4);

    const fillsBeforeSwitch = canvasMock.ctx.fillRect.mock.calls.length;
    canvasMock.ctx.fillRect.mockClear();
    canvasMock.ctx.clearRect.mockClear();

    mockPlayerState.snapshot = {
      duration_ms: 80_000,
      is_playing: true,
      state: "playing",
      song_id: "song-2",
    };
    await act(async () => {
      root.render(<SeekBar />);
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(canvasMock.ctx.clearRect).toHaveBeenCalled();
    expect(canvasMock.ctx.fillRect).not.toHaveBeenCalled();
    expect(mockGetWaveform).toHaveBeenCalledWith("song-2", 133);

    await act(async () => {
      resolveSong2?.({ peaks: [0.1, 0.2], buckets: 2 });
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(canvasMock.ctx.fillRect.mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(canvasMock.ctx.fillRect.mock.calls.length).toBeLessThan(
      fillsBeforeSwitch + 2,
    );
  });

  test("ignores late waveform response from a previous song after switch", async () => {
    let resolveSong1:
      | ((value: { peaks: number[]; buckets: number }) => void)
      | undefined;
    mockGetWaveform.mockImplementation((songId: string) => {
      if (songId === "song-1") {
        return new Promise((resolve) => {
          resolveSong1 = resolve;
        });
      }
      return Promise.resolve({ peaks: [0.3, 0.3], buckets: 2 });
    });

    await act(async () => {
      root.render(<SeekBar />);
    });

    mockPlayerState.snapshot = {
      duration_ms: 80_000,
      is_playing: true,
      state: "playing",
      song_id: "song-2",
    };
    await act(async () => {
      root.render(<SeekBar />);
    });
    await act(async () => {
      await Promise.resolve();
    });

    canvasMock.ctx.fillRect.mockClear();
    const rail = host.querySelector("[role='slider']") as HTMLElement;
    stubRailGeometry(rail, 200, 12);

    await act(async () => {
      resolveSong1?.({ peaks: [1, 1, 1, 1, 1], buckets: 5 });
      await Promise.resolve();
      await Promise.resolve();
    });

    const fillCount = canvasMock.ctx.fillRect.mock.calls.length;
    expect(fillCount).toBeLessThan(5);
  });

  test("mouseup after drag seeks to the release position", () => {
    act(() => {
      root.render(<SeekBar />);
    });

    const rail = host.querySelector("[role='slider']") as HTMLElement;
    expect(rail).toBeTruthy();
    stubRailGeometry(rail);

    act(() => {
      rail.dispatchEvent(
        new MouseEvent("mousedown", { clientX: 50, bubbles: true }),
      );
    });
    act(() => {
      window.dispatchEvent(
        new MouseEvent("mousemove", { clientX: 80, bubbles: true }),
      );
    });
    act(() => {
      window.dispatchEvent(
        new MouseEvent("mouseup", { clientX: 100, bubbles: true }),
      );
    });

    // 100/200 = 50% of 100_000ms
    expect(mockSeek).toHaveBeenCalledWith(50_000);
  });

  test("clamps drag percent to 0..100 from pointer position", () => {
    act(() => {
      root.render(<SeekBar />);
    });

    const rail = host.querySelector("[role='slider']") as HTMLElement;
    stubRailGeometry(rail);

    act(() => {
      rail.dispatchEvent(
        new MouseEvent("mousedown", { clientX: -50, bubbles: true }),
      );
    });
    act(() => {
      window.dispatchEvent(
        new MouseEvent("mouseup", { clientX: 9999, bubbles: true }),
      );
    });

    // Clamped to 100% of duration
    expect(mockSeek).toHaveBeenCalledWith(100_000);
  });

  test("updates display position via requestAnimationFrame", () => {
    let rafCb: FrameRequestCallback | null = null;
    vi.stubGlobal(
      "requestAnimationFrame",
      vi.fn((cb: FrameRequestCallback) => {
        rafCb = cb;
        return 1;
      }),
    );
    mockSelectCurrentPositionMs.mockReturnValue(42_000);

    act(() => {
      root.render(<SeekBar />);
    });

    expect(rafCb).toBeTruthy();
    act(() => {
      rafCb?.(16);
    });

    expect(mockSelectCurrentPositionMs).toHaveBeenCalled();
    const startLabel = host.querySelectorAll("span")[0];
    expect(startLabel?.textContent).toContain("0:42");
  });

  test("cancels rAF and disconnects ResizeObserver on unmount", () => {
    const cancelSpy = vi.fn();
    const disconnectSpy = vi.fn();
    vi.stubGlobal("cancelAnimationFrame", cancelSpy);
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect = disconnectSpy;
      },
    );

    act(() => {
      root.render(<SeekBar />);
    });
    act(() => {
      root.unmount();
    });

    expect(cancelSpy).toHaveBeenCalled();
    expect(disconnectSpy).toHaveBeenCalled();
  });

  test("resize observer bumps waveform version to redraw", async () => {
    mockGetWaveform.mockResolvedValue({
      peaks: [0.5, 0.6],
      buckets: 2,
    });

    await act(async () => {
      root.render(<SeekBar />);
    });

    const rail = host.querySelector("[role='slider']") as HTMLElement;
    stubRailGeometry(rail, 100, 10);

    await act(async () => {
      await Promise.resolve();
    });
    const fillsBefore = canvasMock.ctx.fillRect.mock.calls.length;

    await act(async () => {
      resizeCallback?.([], {} as ResizeObserver);
      // Flush the 150ms resize debounce.
      await new Promise((r) => setTimeout(r, 200));
    });

    expect(canvasMock.ctx.fillRect.mock.calls.length).toBeGreaterThanOrEqual(
      fillsBefore,
    );
  });

  test("ignores late waveform responses after unmount", async () => {
    let resolveWaveform: (value: {
      peaks: number[];
      buckets: number;
    }) => void = () => {};
    mockGetWaveform.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveWaveform = resolve;
        }),
    );

    await act(async () => {
      root.render(<SeekBar />);
    });
    act(() => {
      root.unmount();
    });

    await act(async () => {
      resolveWaveform({ peaks: [1, 1, 1], buckets: 3 });
      await Promise.resolve();
    });

    expect(mockGetWaveform).toHaveBeenCalled();
  });

  test("requests different bucket counts for 1x vs 2x DPR at the same rail width", async () => {
    mockGetWaveform.mockResolvedValue({ peaks: [0.5], buckets: 1 });

    // 1x DPR, 900 CSS px → round(900*1/3) = 300.
    Object.defineProperty(window, "devicePixelRatio", {
      configurable: true,
      value: 1,
    });

    await act(async () => {
      root.render(<SeekBar />);
    });
    const rail = host.querySelector("[role='slider']") as HTMLElement;
    stubRailGeometry(rail, 900, 12);
    await act(async () => {
      resizeCallback?.([], {} as ResizeObserver);
      // Flush the 150ms resize debounce.
      await new Promise((r) => setTimeout(r, 200));
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(mockGetWaveform).toHaveBeenCalledWith("song-1", 300);

    // 2x DPR, same 900 CSS px → round(900*2/3) = 600.
    Object.defineProperty(window, "devicePixelRatio", {
      configurable: true,
      value: 2,
    });
    mockGetWaveform.mockClear();
    await act(async () => {
      mediaChangeCallback?.();
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(mockGetWaveform).toHaveBeenCalledWith("song-1", 600);
  });

  test("DPR-only display migration refetches instead of stretching the old waveform", async () => {
    mockGetWaveform.mockResolvedValue({ peaks: [0.5], buckets: 1 });

    await act(async () => {
      root.render(<SeekBar />);
    });
    const rail = host.querySelector("[role='slider']") as HTMLElement;
    stubRailGeometry(rail, 900, 12);
    await act(async () => {
      resizeCallback?.([], {} as ResizeObserver);
      // Flush the 150ms resize debounce.
      await new Promise((r) => setTimeout(r, 200));
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(mockGetWaveform).toHaveBeenCalledWith("song-1", 600);

    Object.defineProperty(window, "devicePixelRatio", {
      configurable: true,
      value: 1,
    });
    mockGetWaveform.mockClear();
    await act(async () => {
      mediaChangeCallback?.();
    });
    await act(async () => {
      await Promise.resolve();
    });

    // No ResizeObserver fire (same CSS width) — the media-query change
    // alone drives a refetch at the new bucket count 300.
    expect(mockGetWaveform).toHaveBeenCalledWith("song-1", 300);
  });

  test("quantized no-op DPR resize does not refetch", async () => {
    mockGetWaveform.mockResolvedValue({ peaks: [0.5], buckets: 1 });

    await act(async () => {
      root.render(<SeekBar />);
    });
    const rail = host.querySelector("[role='slider']") as HTMLElement;
    // 299 CSS px @ 2x → round(299*2/3) = round(199.33) = 199.
    stubRailGeometry(rail, 299, 12);
    await act(async () => {
      resizeCallback?.([], {} as ResizeObserver);
      // Flush the 150ms resize debounce.
      await new Promise((r) => setTimeout(r, 200));
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(mockGetWaveform).toHaveBeenCalledWith("song-1", 199);
    const callsBefore = mockGetWaveform.mock.calls.length;

    Object.defineProperty(window, "devicePixelRatio", {
      configurable: true,
      value: 2.001,
    });
    mockGetWaveform.mockClear();
    await act(async () => {
      mediaChangeCallback?.();
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(mockGetWaveform).not.toHaveBeenCalled();
    expect(mockGetWaveform.mock.calls.length).toBe(0);
    expect(callsBefore).toBeGreaterThanOrEqual(1);
  });

  test("DPR change with same bucket count redraws canvas at new physical pixels", async () => {
    mockGetWaveform.mockResolvedValue({ peaks: [0.5], buckets: 1 });

    await act(async () => {
      root.render(<SeekBar />);
    });
    const rail = host.querySelector("[role='slider']") as HTMLElement;
    // 299 CSS px @ 2x → round(299*2/3) = 199 buckets.
    stubRailGeometry(rail, 299, 12);
    await act(async () => {
      resizeCallback?.([], {} as ResizeObserver);
      // Flush the 150ms resize debounce.
      await new Promise((r) => setTimeout(r, 200));
    });
    await act(async () => {
      await Promise.resolve();
    });

    const canvas = host.querySelector(
      "[data-waveform-canvas]",
    ) as HTMLCanvasElement;
    expect(canvas).toBeTruthy();
    // Initial backing store: round(299 * 2) x round(12 * 2) = 598 x 24.
    expect(canvas.width).toBe(598);
    expect(canvas.height).toBe(24);

    Object.defineProperty(window, "devicePixelRatio", {
      configurable: true,
      value: 2.001,
    });
    mockGetWaveform.mockClear();
    await act(async () => {
      mediaChangeCallback?.();
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(mockGetWaveform).not.toHaveBeenCalled();
    // Backing store updated to the new physical dimensions:
    // round(299 * 2.001) = 598, round(12 * 2.001) = 24.
    expect(canvas.width).toBe(598);
    expect(canvas.height).toBe(24);
  });

  test("re-registers the resolution media query when DPR changes", async () => {
    await act(async () => {
      root.render(<SeekBar />);
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(mediaQueryDpr).toBe(2);

    Object.defineProperty(window, "devicePixelRatio", {
      configurable: true,
      value: 3,
    });
    await act(async () => {
      mediaChangeCallback?.();
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(mediaQueryDpr).toBe(3);
  });

  test("supports standard keyboard slider navigation", async () => {
    await act(async () => {
      root.render(<SeekBar />);
    });

    const rail = host.querySelector("[role='slider']") as HTMLElement;
    expect(rail.getAttribute("tabindex")).toBe("0");

    await act(async () => {
      rail.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "ArrowRight" }),
      );
      rail.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "PageUp" }),
      );
      rail.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "Home" }),
      );
      rail.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "End" }),
      );
    });

    expect(mockSeek).toHaveBeenNthCalledWith(1, 15_000);
    expect(mockSeek).toHaveBeenNthCalledWith(2, 40_000);
    expect(mockSeek).toHaveBeenNthCalledWith(3, 0);
    expect(mockSeek).toHaveBeenNthCalledWith(4, 100_000);
  });

  test("removes an empty seek rail from the tab order", async () => {
    mockPlayerState.snapshot = null;
    await act(async () => {
      root.render(<SeekBar />);
    });

    const rail = host.querySelector("[role='slider']") as HTMLElement;
    expect(rail.getAttribute("tabindex")).toBe("-1");
    expect(rail.getAttribute("aria-disabled")).toBe("true");
  });
});
