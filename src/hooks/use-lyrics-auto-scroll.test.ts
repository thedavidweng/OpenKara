// @vitest-environment jsdom

import { beforeEach, describe, expect, test, vi } from "vitest";
import { act, createElement, useRef } from "react";
import { createRoot } from "react-dom/client";
import {
  computeLineChangeLyricsScrollTop,
  createUserScrollGuard,
  useLyricsAutoScroll,
} from "./use-lyrics-auto-scroll";

const { mockLyricsState, mockAdjustedPlaybackMs } = vi.hoisted(() => ({
  mockLyricsState: {
    lines: [] as { time_ms: number }[],
  },
  mockAdjustedPlaybackMs: vi.fn(() => 0),
}));

vi.mock("@/stores/lyrics-store", () => ({
  useLyricsStore: {
    getState: () => mockLyricsState,
  },
}));

vi.mock("@/lib/lyrics-playback-clock", () => ({
  readLyricsAdjustedPlaybackMs: mockAdjustedPlaybackMs,
}));

const PAUSE_MS = 3000;

function makeContainer(): HTMLDivElement {
  return document.createElement("div");
}

describe("createUserScrollGuard", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  test("is inactive before any user interaction", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS);

    expect(guard.isActive()).toBe(false);

    guard.destroy();
  });

  test("becomes active immediately after a wheel event", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS);

    container.dispatchEvent(new Event("wheel"));

    expect(guard.isActive()).toBe(true);

    guard.destroy();
  });

  test("becomes active immediately after a touchstart event", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS);

    container.dispatchEvent(new Event("touchstart"));

    expect(guard.isActive()).toBe(true);

    guard.destroy();
  });

  test("remains active while the pause window is still open", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS);

    container.dispatchEvent(new Event("wheel"));
    vi.advanceTimersByTime(PAUSE_MS - 1);

    expect(guard.isActive()).toBe(true);

    guard.destroy();
  });

  test("deactivates automatically after the full pause duration elapses", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS);

    container.dispatchEvent(new Event("wheel"));
    vi.advanceTimersByTime(PAUSE_MS);

    expect(guard.isActive()).toBe(false);

    guard.destroy();
  });

  test("resets the pause timer when the user scrolls again before it expires", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS);

    container.dispatchEvent(new Event("wheel"));
    vi.advanceTimersByTime(PAUSE_MS - 500);
    container.dispatchEvent(new Event("wheel"));
    vi.advanceTimersByTime(PAUSE_MS - 1);

    expect(guard.isActive()).toBe(true);

    vi.advanceTimersByTime(1);
    expect(guard.isActive()).toBe(false);

    guard.destroy();
  });

  test("stops responding to events after destroy()", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS);

    guard.destroy();
    container.dispatchEvent(new Event("wheel"));

    expect(guard.isActive()).toBe(false);
  });

  test("clears a pending resume timer on destroy()", () => {
    const clearTimeout = vi.fn(globalThis.clearTimeout.bind(globalThis));
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS, {
      setTimeout: globalThis.setTimeout,
      clearTimeout,
    });

    container.dispatchEvent(new Event("wheel"));
    guard.destroy();

    expect(clearTimeout).toHaveBeenCalled();
  });

  test("resets to inactive immediately on destroy() even if window was open", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS);

    container.dispatchEvent(new Event("wheel"));
    expect(guard.isActive()).toBe(true);

    guard.destroy();
    expect(guard.isActive()).toBe(false);
  });

  test("touchstart also resets the timer when fired after wheel", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS);

    container.dispatchEvent(new Event("wheel"));
    vi.advanceTimersByTime(PAUSE_MS - 500);
    container.dispatchEvent(new Event("touchstart"));
    vi.advanceTimersByTime(PAUSE_MS - 1);

    expect(guard.isActive()).toBe(true);

    vi.advanceTimersByTime(1);
    expect(guard.isActive()).toBe(false);

    guard.destroy();
  });
});

describe("computeLineChangeLyricsScrollTop", () => {
  test("keeps the current lyric anchor stable for the whole active line", () => {
    const container = document.createElement("div");
    Object.defineProperty(container, "clientHeight", { value: 100 });
    Object.defineProperty(container, "scrollHeight", { value: 500 });

    const line0 = document.createElement("div");
    line0.dataset.lyricsLineIndex = "0";
    Object.defineProperty(line0, "offsetTop", { value: 0 });
    Object.defineProperty(line0, "clientHeight", { value: 40 });

    const line1 = document.createElement("div");
    line1.dataset.lyricsLineIndex = "1";
    Object.defineProperty(line1, "offsetTop", { value: 100 });
    Object.defineProperty(line1, "clientHeight", { value: 40 });

    container.append(line0, line1);

    expect(
      computeLineChangeLyricsScrollTop(
        container,
        [{ time_ms: 0 }, { time_ms: 1000 }],
        0,
      ),
    ).toBe(0);
    expect(
      computeLineChangeLyricsScrollTop(
        container,
        [{ time_ms: 0 }, { time_ms: 1000 }],
        500,
      ),
    ).toBe(0);
    expect(
      computeLineChangeLyricsScrollTop(
        container,
        [{ time_ms: 0 }, { time_ms: 1000 }],
        1000,
      ),
    ).toBe(70);
  });
});

describe("useLyricsAutoScroll", () => {
  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
  });

  test("resumes a distant lyric seek from the current viewport after manual scrolling", async () => {
    vi.useFakeTimers();
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = vi.fn((callback: FrameRequestCallback) => {
      rafCallbacks.push(callback);
      return rafCallbacks.length;
    });
    window.cancelAnimationFrame = vi.fn();
    mockLyricsState.lines = [
      { time_ms: 0 },
      { time_ms: 1000 },
      { time_ms: 2000 },
    ];
    mockAdjustedPlaybackMs.mockReturnValue(0);

    function TestHarness() {
      const containerRef = useRef<HTMLDivElement | null>(null);

      useLyricsAutoScroll(containerRef, false, 0, "standard", "song-1");

      return createElement(
        "div",
        {
          ref: (node: HTMLDivElement | null) => {
            containerRef.current = node;
            if (!node) return;
            Object.defineProperty(node, "clientHeight", {
              configurable: true,
              value: 100,
            });
            Object.defineProperty(node, "scrollHeight", {
              configurable: true,
              value: 2500,
            });
          },
        },
        [0, 1000, 2000].map((top, index) =>
          createElement(
            "div",
            {
              key: index,
              ref: (node: HTMLDivElement | null) => {
                if (!node) return;
                Object.defineProperty(node, "offsetTop", {
                  configurable: true,
                  value: top,
                });
                Object.defineProperty(node, "clientHeight", {
                  configurable: true,
                  value: 40,
                });
              },
              "data-lyrics-line-index": String(index),
            },
            `Line ${index}`,
          ),
        ),
      );
    }

    await act(async () => {
      root.render(createElement(TestHarness));
    });

    const container = host.firstElementChild as HTMLDivElement;
    await act(async () => {
      rafCallbacks.shift()?.(16);
    });

    await act(async () => {
      container.dispatchEvent(new Event("wheel"));
      container.scrollTop = 1800;
      mockAdjustedPlaybackMs.mockReturnValue(2000);
      rafCallbacks.shift()?.(32);
    });

    await act(async () => {
      vi.advanceTimersByTime(PAUSE_MS);
      rafCallbacks.shift()?.(48);
    });

    expect(container.scrollTop).toBeGreaterThanOrEqual(1800);

    window.requestAnimationFrame = originalRaf;
    window.cancelAnimationFrame = originalCancelRaf;
    root.unmount();
    host.remove();
    mockLyricsState.lines = [];
    vi.useRealTimers();
  });

  test("does not keep drifting while playback remains within the same lyric line", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const rafCallbacks: FrameRequestCallback[] = [];
    const originalRaf = window.requestAnimationFrame;
    const originalCancelRaf = window.cancelAnimationFrame;
    window.requestAnimationFrame = vi.fn((callback: FrameRequestCallback) => {
      rafCallbacks.push(callback);
      return rafCallbacks.length;
    });
    window.cancelAnimationFrame = vi.fn();
    mockLyricsState.lines = [{ time_ms: 0 }, { time_ms: 1000 }];
    mockAdjustedPlaybackMs.mockReturnValue(0);

    function TestHarness() {
      const containerRef = useRef<HTMLDivElement | null>(null);

      useLyricsAutoScroll(containerRef, false, 0, "standard", "song-1");

      return createElement(
        "div",
        {
          ref: (node: HTMLDivElement | null) => {
            containerRef.current = node;
            if (!node) return;
            Object.defineProperty(node, "clientHeight", {
              configurable: true,
              value: 100,
            });
            Object.defineProperty(node, "scrollHeight", {
              configurable: true,
              value: 500,
            });
          },
        },
        createElement(
          "div",
          {
            ref: (node: HTMLDivElement | null) => {
              if (!node) return;
              Object.defineProperty(node, "offsetTop", {
                configurable: true,
                value: 0,
              });
              Object.defineProperty(node, "clientHeight", {
                configurable: true,
                value: 40,
              });
            },
            "data-lyrics-line-index": "0",
          },
          "Line 0",
        ),
        createElement(
          "div",
          {
            ref: (node: HTMLDivElement | null) => {
              if (!node) return;
              Object.defineProperty(node, "offsetTop", {
                configurable: true,
                value: 100,
              });
              Object.defineProperty(node, "clientHeight", {
                configurable: true,
                value: 40,
              });
            },
            "data-lyrics-line-index": "1",
          },
          "Line 1",
        ),
      );
    }

    await act(async () => {
      root.render(createElement(TestHarness));
    });

    const container = host.firstElementChild as HTMLDivElement;
    await act(async () => {
      rafCallbacks.shift()?.(16);
    });
    const scrollAfterFirstTick = container.scrollTop;

    mockAdjustedPlaybackMs.mockReturnValue(500);
    await act(async () => {
      rafCallbacks.shift()?.(32);
    });

    expect(container.scrollTop).toBe(scrollAfterFirstTick);

    window.requestAnimationFrame = originalRaf;
    window.cancelAnimationFrame = originalCancelRaf;
    root.unmount();
    host.remove();
    mockLyricsState.lines = [];
  });

  test("rebinds continuous scroll when lyric layout changes", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);

    function TestHarness({ layoutVersion }: { layoutVersion: string }) {
      const containerRef = useRef<HTMLDivElement | null>(null);

      useLyricsAutoScroll(
        containerRef,
        false,
        0,
        "standard",
        "song-1",
        layoutVersion,
      );

      return createElement(
        "div",
        {
          ref: (node: HTMLDivElement | null) => {
            containerRef.current = node;
            if (!node) return;
            Object.defineProperty(node, "clientHeight", {
              configurable: true,
              value: 100,
            });
            Object.defineProperty(node, "scrollHeight", {
              configurable: true,
              value: 500,
            });
          },
        },
        createElement("div", { "data-lyrics-line-index": "0" }, "Line"),
      );
    }

    await act(async () => {
      root.render(
        createElement(TestHarness, { layoutVersion: "romanized-off" }),
      );
    });

    await act(async () => {
      root.render(
        createElement(TestHarness, { layoutVersion: "romanized-on" }),
      );
    });

    root.unmount();
    host.remove();
  });
});
