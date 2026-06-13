// @vitest-environment jsdom

import { beforeEach, describe, expect, test, vi } from "vitest";
import { act, createElement, useRef } from "react";
import { createRoot } from "react-dom/client";
import {
  computeContinuousLyricsScrollTop,
  createUserScrollGuard,
  useLyricsAutoScroll,
} from "./use-lyrics-auto-scroll";

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

describe("computeContinuousLyricsScrollTop", () => {
  test("interpolates between two lyric anchors", () => {
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
      computeContinuousLyricsScrollTop(
        container,
        [{ time_ms: 0 }, { time_ms: 1000 }],
        0,
      ),
    ).toBe(0);
    expect(
      computeContinuousLyricsScrollTop(
        container,
        [{ time_ms: 0 }, { time_ms: 1000 }],
        500,
      ),
    ).toBe(35);
    expect(
      computeContinuousLyricsScrollTop(
        container,
        [{ time_ms: 0 }, { time_ms: 1000 }],
        1000,
      ),
    ).toBe(70);
  });
});

describe("useLyricsAutoScroll", () => {
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
