// @vitest-environment jsdom

import { beforeEach, describe, expect, test, vi } from "vitest";
import { Spring } from "@/lib/spring";
import {
  computeLineChangeLyricsScrollTop,
  createUserScrollGuard,
  tickLyricsEngineScroll,
  USER_SCROLL_PAUSE_MS,
} from "./lyrics-engine";

const PAUSE_MS = USER_SCROLL_PAUSE_MS;

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

describe("tickLyricsEngineScroll", () => {
  test("re-anchors when the active line changes even if the pixel target matches", () => {
    const container = document.createElement("div");
    const scrollContent = document.createElement("div");
    Object.defineProperty(container, "clientHeight", { value: 100 });
    Object.defineProperty(container, "scrollHeight", { value: 500 });

    const line0 = document.createElement("div");
    line0.dataset.lyricsLineIndex = "0";
    Object.defineProperty(line0, "offsetTop", { value: 0 });
    Object.defineProperty(line0, "clientHeight", { value: 40 });

    const line1 = document.createElement("div");
    line1.dataset.lyricsLineIndex = "1";
    Object.defineProperty(line1, "offsetTop", { value: 5 });
    Object.defineProperty(line1, "clientHeight", { value: 40 });

    container.append(line0, line1);

    const scrollSpring = new Spring(0, {
      stiffness: 170,
      damping: 28,
      mass: 1,
    });
    const scrollState = {
      scrollSpring,
      targetScrollTopRef: { current: 0 as number | null },
      prevActiveIndexRef: { current: 0 },
    };

    tickLyricsEngineScroll({
      container,
      scrollContent,
      lines: [{ time_ms: 0 }, { time_ms: 1000 }],
      adjustedMs: 0,
      scrollState,
      userScrollGuard: null,
      reducedMotion: true,
      dt: 0.016,
    });

    expect(scrollState.prevActiveIndexRef.current).toBe(0);
    expect(scrollContent.style.transform).toBe("translateY(0px)");

    tickLyricsEngineScroll({
      container,
      scrollContent,
      lines: [{ time_ms: 0 }, { time_ms: 1000 }],
      adjustedMs: 1000,
      scrollState,
      userScrollGuard: null,
      reducedMotion: true,
      dt: 0.016,
    });

    expect(scrollState.prevActiveIndexRef.current).toBe(1);
    expect(scrollContent.style.transform).toBe("translateY(0px)");
  });

  test("bakes manual scroll into transform while the user-scroll guard is active", () => {
    const container = document.createElement("div");
    const scrollContent = document.createElement("div");
    Object.defineProperty(container, "clientHeight", { value: 100 });
    Object.defineProperty(container, "scrollHeight", { value: 500 });

    const line0 = document.createElement("div");
    line0.dataset.lyricsLineIndex = "0";
    Object.defineProperty(line0, "offsetTop", { value: 0 });
    Object.defineProperty(line0, "clientHeight", { value: 40 });
    container.append(line0);

    const scrollSpring = new Spring(0, {
      stiffness: 170,
      damping: 28,
      mass: 1,
    });
    const scrollState = {
      scrollSpring,
      targetScrollTopRef: { current: 0 as number | null },
      prevActiveIndexRef: { current: -1 },
    };

    container.scrollTop = 180;

    tickLyricsEngineScroll({
      container,
      scrollContent,
      lines: [{ time_ms: 0 }],
      adjustedMs: 0,
      scrollState,
      userScrollGuard: { isActive: () => true },
      reducedMotion: false,
      dt: 0.016,
    });

    expect(container.scrollTop).toBe(0);
    expect(scrollSpring.getPosition()).toBe(180);
    expect(scrollContent.style.transform).toBe("translateY(-180px)");
  });
});
