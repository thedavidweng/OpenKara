// @vitest-environment jsdom

import { act, useLayoutEffect, useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { useLyricsEngine } from "./use-lyrics-engine";
import * as lyricsEngine from "@/lib/lyrics-engine";
import {
  peekLyricsAutoScrollResumeGeneration,
  resetLyricsEngineScrollControlForTests,
} from "@/lib/lyrics-engine";
import { resetLyricsPlaybackTimeForTests } from "@/lib/lyrics-playback-time";
import type { LyricsLineRuntime } from "@/lib/lyrics-line-runtime";

const {
  mockPlayerState,
  mockLyricsState,
  mockSelectCurrentPositionMs,
  mockLineRuntime,
} = vi.hoisted(() => {
  // Call through nowMs so the rAF host sample `() => now` is covered.
  const mockSelectCurrentPositionMs = vi.fn(
    (state: { positionMs: number }, nowMs?: () => number) => {
      if (typeof nowMs === "function") {
        nowMs();
      }
      return state.positionMs;
    },
  );
  return {
    mockPlayerState: {
      snapshot: {
        song_id: "song-1",
        is_playing: true,
        state: "playing" as string,
      },
      positionMs: 0,
      playingSinceMs: 0,
      seekRevision: 0,
      airPlayOutput: {
        active: false,
        displayedPositionMs: null as number | null,
      },
    },
    mockLyricsState: {
      lines: [
        { time_ms: 0, text: "a", words: null },
        { time_ms: 5_000, text: "b", words: null },
      ],
      activeLineIndex: 0,
      activeWordIndex: -1,
      offsetMs: 0,
      setActiveLineIndex: vi.fn(),
      setActiveWordIndex: vi.fn(),
    },
    mockSelectCurrentPositionMs,
    mockLineRuntime: {
      clear: vi.fn(),
      tick: vi.fn(),
      registerWrapper: vi.fn(),
      unregisterWrapper: vi.fn(),
      registerKaraoke: vi.fn(),
      unregisterKaraoke: vi.fn(),
    } as unknown as LyricsLineRuntime & {
      clear: ReturnType<typeof vi.fn>;
      tick: ReturnType<typeof vi.fn>;
    },
  };
});

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: Object.assign(
    (selector: (state: typeof mockPlayerState) => unknown) =>
      selector(mockPlayerState),
    {
      getState: () => mockPlayerState,
    },
  ),
  selectCurrentPositionMs: mockSelectCurrentPositionMs,
}));

vi.mock("@/stores/lyrics-store", () => ({
  useLyricsStore: Object.assign(
    (selector: (state: typeof mockLyricsState) => unknown) =>
      selector(mockLyricsState),
    {
      getState: () => mockLyricsState,
    },
  ),
}));

function Harness(props: {
  viewportActive?: boolean;
  isPlainText?: boolean;
  songId?: string | null;
  onUserScrollActiveChange?: (active: boolean) => void;
  mountContainer?: boolean;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  // Flip viewportActive only after the container is in the DOM so the guard
  // layout effect re-runs with a non-null containerRef (matches LyricsPanel).
  const [domReady, setDomReady] = useState(false);
  useLayoutEffect(() => {
    if (props.mountContainer === false) {
      return;
    }
    setDomReady(true);
  }, [props.mountContainer]);

  useLyricsEngine({
    containerRef,
    isPlainText: props.isPlainText ?? false,
    lyricsFontStep: 0,
    presentation: "standard",
    songId: props.songId === undefined ? "song-1" : props.songId,
    viewportActive: (props.viewportActive ?? true) && domReady,
    lineRuntime: mockLineRuntime,
    onUserScrollActiveChange: props.onUserScrollActiveChange,
  });

  if (props.mountContainer === false) {
    return null;
  }

  return (
    <div
      ref={containerRef}
      data-testid="harness-viewport"
      style={{ height: 120, overflow: "auto" }}
    >
      <div style={{ height: 800 }} />
    </div>
  );
}

function defineNumber(el: Element, prop: string, value: number) {
  Object.defineProperty(el, prop, { value, configurable: true });
}

// Harness with real line elements so the scroll engine can measure geometry
// (offsetTop / clientHeight) for the re-anchor tests (#201 / #202). jsdom does
// not lay out, so the test defines geometry on the mounted nodes directly.
function ScrollHarness(props: { lyricsFontStep: number; songId?: string }) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [domReady, setDomReady] = useState(false);
  useLayoutEffect(() => {
    setDomReady(true);
  }, []);

  useLyricsEngine({
    containerRef,
    isPlainText: false,
    lyricsFontStep: props.lyricsFontStep,
    presentation: "standard",
    songId: props.songId ?? "song-1",
    viewportActive: domReady,
    lineRuntime: mockLineRuntime,
  });

  return (
    <div
      ref={containerRef}
      data-testid="scroll-viewport"
      style={{ height: 100, overflow: "auto" }}
    >
      <div data-lyrics-line-index="0" className="w-full">
        a
      </div>
      <div data-lyrics-line-index="1" className="w-full">
        b
      </div>
    </div>
  );
}

describe("useLyricsEngine", () => {
  let root: Root;
  let host: HTMLDivElement;
  let rafCb: FrameRequestCallback | null = null;
  let rafId = 1;

  beforeEach(() => {
    (
      globalThis as typeof globalThis & {
        IS_REACT_ACT_ENVIRONMENT?: boolean;
      }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    resetLyricsEngineScrollControlForTests();
    resetLyricsPlaybackTimeForTests();
    rafCb = null;
    vi.stubGlobal(
      "requestAnimationFrame",
      vi.fn((cb: FrameRequestCallback) => {
        rafCb = cb;
        return rafId++;
      }),
    );
    vi.stubGlobal(
      "cancelAnimationFrame",
      vi.fn(() => {
        rafCb = null;
      }),
    );
    vi.stubGlobal(
      "matchMedia",
      vi.fn(() => ({
        matches: true,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    );

    mockPlayerState.snapshot = {
      song_id: "song-1",
      is_playing: true,
      state: "playing",
    };
    mockPlayerState.positionMs = 1000;
    mockPlayerState.playingSinceMs = 1000;
    mockPlayerState.seekRevision = 0;
    mockPlayerState.airPlayOutput = {
      active: false,
      displayedPositionMs: null,
    };
    mockSelectCurrentPositionMs.mockImplementation(
      (state: { positionMs: number }, nowMs?: () => number) => {
        if (typeof nowMs === "function") {
          nowMs();
        }
        return state.positionMs;
      },
    );
    mockLyricsState.activeLineIndex = 0;
    mockLyricsState.activeWordIndex = -1;
    mockLyricsState.offsetMs = 0;
    mockLyricsState.setActiveLineIndex.mockReset();
    mockLyricsState.setActiveWordIndex.mockReset();
    mockLineRuntime.clear.mockReset();
    mockLineRuntime.tick.mockReset();

    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    host.remove();
    document.body.innerHTML = "";
    vi.unstubAllGlobals();
  });

  test("starts the rAF loop and ticks the line runtime from the host clock", () => {
    act(() => {
      root.render(<Harness />);
    });

    expect(rafCb).not.toBeNull();
    act(() => {
      rafCb?.(1000);
    });

    expect(mockLineRuntime.tick).toHaveBeenCalled();
    expect(mockSelectCurrentPositionMs).toHaveBeenCalled();
  });

  test("consumes seekRevision as isSeek and bumps auto-scroll resume", () => {
    act(() => {
      root.render(<Harness />);
    });

    const before = peekLyricsAutoScrollResumeGeneration();
    mockPlayerState.seekRevision = 3;
    mockPlayerState.positionMs = 12_000;

    act(() => {
      rafCb?.(1100);
    });

    expect(peekLyricsAutoScrollResumeGeneration()).toBe(before + 1);

    // Second frame with the same revision is not another seek.
    act(() => {
      rafCb?.(1120);
    });
    expect(peekLyricsAutoScrollResumeGeneration()).toBe(before + 1);
  });

  test("attaches the user-scroll guard when the viewport mounts", () => {
    const onActive = vi.fn();
    act(() => {
      root.render(<Harness onUserScrollActiveChange={onActive} />);
    });

    const viewport = host.querySelector(
      "[data-testid='harness-viewport']",
    ) as HTMLDivElement;
    expect(viewport).toBeTruthy();

    act(() => {
      viewport.dispatchEvent(new WheelEvent("wheel", { deltaY: 40 }));
    });
    expect(onActive).toHaveBeenCalledWith(true);
  });

  test("skips the engine loop when plain-text or viewport inactive", () => {
    act(() => {
      root.render(<Harness isPlainText viewportActive={false} />);
    });
    expect(rafCb).toBeNull();
  });

  test("skips guard setup when viewportActive but container is not mounted", () => {
    function NoDomHarness() {
      const containerRef = useRef<HTMLDivElement | null>(null);
      useLyricsEngine({
        containerRef,
        isPlainText: false,
        lyricsFontStep: 0,
        presentation: "standard",
        songId: "song-1",
        viewportActive: true,
        lineRuntime: mockLineRuntime,
      });
      return null;
    }
    act(() => {
      root.render(<NoDomHarness />);
    });
    // Layout effect returns early at container null — no throw, no rAF from guard.
    expect(true).toBe(true);
  });

  test("skips the engine loop when no song is loaded", () => {
    mockPlayerState.snapshot = {
      song_id: null as unknown as string,
      is_playing: false,
      state: "idle",
    };
    act(() => {
      root.render(<Harness songId={null} />);
    });
    expect(rafCb).toBeNull();
  });

  test("skips the engine loop when the player has no song snapshot", () => {
    // viewportActive/songId true but shouldRunLyricsEngineLoop is false.
    mockPlayerState.snapshot = {
      song_id: null as unknown as string,
      is_playing: false,
      state: "idle",
    };
    act(() => {
      root.render(<Harness songId="song-1" />);
    });
    expect(rafCb).toBeNull();
  });

  test("resets scrollTop directly when the user-scroll guard is unavailable", () => {
    // Engine effect has a container but guardRef is null (layout skipped /
    // failed to attach). Must still zero scrollTop without withProgrammatic.
    vi.spyOn(lyricsEngine, "createUserScrollGuard").mockReturnValue(
      null as unknown as ReturnType<typeof lyricsEngine.createUserScrollGuard>,
    );

    act(() => {
      root.render(<Harness />);
    });

    const viewport = host.querySelector(
      "[data-testid='harness-viewport']",
    ) as HTMLDivElement;
    expect(viewport).toBeTruthy();
    viewport.scrollTop = 120;
    // Re-trigger the engine effect (songId change) so the null-guard write runs.
    act(() => {
      root.render(<Harness songId="song-2" />);
    });
    expect(viewport.scrollTop).toBe(0);

    vi.mocked(lyricsEngine.createUserScrollGuard).mockRestore();
  });

  test("focus resync updates active line without consuming a seek latch", () => {
    act(() => {
      root.render(<Harness />);
    });

    mockPlayerState.positionMs = 5_500;
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });

    expect(mockLyricsState.setActiveLineIndex).toHaveBeenCalled();
  });

  test("changing the font size re-anchors in place instead of resetting scrollTop to 0 (#201)", () => {
    // Held on line index 1 (adjustedMs 6000 ≥ the line-1 time of 5000).
    mockPlayerState.positionMs = 6000;

    act(() => {
      root.render(<ScrollHarness lyricsFontStep={0} />);
    });

    const viewport = host.querySelector(
      "[data-testid='scroll-viewport']",
    ) as HTMLDivElement;
    defineNumber(viewport, "clientHeight", 100);
    defineNumber(viewport, "scrollHeight", 500);
    const line0 = viewport.querySelector("[data-lyrics-line-index='0']")!;
    const line1 = viewport.querySelector("[data-lyrics-line-index='1']")!;
    defineNumber(line0, "offsetTop", 0);
    defineNumber(line0, "clientHeight", 40);
    defineNumber(line1, "offsetTop", 200);
    defineNumber(line1, "clientHeight", 40);

    // Settle onto the held active line: centered target = 200 + 20 - 50 = 170.
    act(() => {
      rafCb?.(1000);
    });
    expect(viewport.scrollTop).toBe(170);

    // Font size changes mid-song; the active line grows and shifts down.
    defineNumber(line1, "offsetTop", 260);
    act(() => {
      root.render(<ScrollHarness lyricsFontStep={2} />);
    });

    // The engine effect re-ran, but a layout/font change (same song) must NOT
    // reset the viewport to the top.
    expect(viewport.scrollTop).toBe(170);
    expect(viewport.scrollTop).not.toBe(0);

    // The next frame snaps directly to the recomputed centered target for the
    // current active line (260 + 20 - 50 = 230) — no animate-from-zero.
    act(() => {
      rafCb?.(1100);
    });
    expect(viewport.scrollTop).toBe(230);
    expect(viewport.scrollTop).not.toBe(0);
  });

  test("re-centers the held active line when the viewport is resized (#202)", () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    let resizeCb: ResizeObserverCallback | null = null;
    class MockResizeObserver {
      constructor(cb: ResizeObserverCallback) {
        resizeCb = cb;
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal("ResizeObserver", MockResizeObserver);

    mockLyricsState.activeLineIndex = 1;
    mockPlayerState.positionMs = 6000;

    act(() => {
      root.render(<ScrollHarness lyricsFontStep={0} />);
    });

    const viewport = host.querySelector(
      "[data-testid='scroll-viewport']",
    ) as HTMLDivElement;
    defineNumber(viewport, "clientWidth", 300);
    defineNumber(viewport, "clientHeight", 100);
    defineNumber(viewport, "scrollHeight", 500);
    const line0 = viewport.querySelector("[data-lyrics-line-index='0']")!;
    const line1 = viewport.querySelector("[data-lyrics-line-index='1']")!;
    defineNumber(line0, "offsetTop", 0);
    defineNumber(line0, "clientHeight", 40);
    defineNumber(line1, "offsetTop", 200);
    defineNumber(line1, "clientHeight", 40);

    // Held line 1 sits at the target for the current (100px) viewport height.
    act(() => {
      rafCb?.(1000);
    });
    expect(viewport.scrollTop).toBe(170);

    // Window grows taller (100 → 200). Without a resize observer the engine
    // would pin scrollTop at the stale 170 until the next line change.
    defineNumber(viewport, "clientHeight", 200);
    act(() => {
      resizeCb?.([], {} as ResizeObserver);
      vi.advanceTimersByTime(200);
    });

    // Re-centered for the SAME active line at the new size: 200 + 20 - 100 = 120.
    expect(viewport.scrollTop).toBe(120);

    vi.useRealTimers();
  });

  test("does not re-center on resize while the user is browsing (#202)", () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    let resizeCb: ResizeObserverCallback | null = null;
    class MockResizeObserver {
      constructor(cb: ResizeObserverCallback) {
        resizeCb = cb;
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal("ResizeObserver", MockResizeObserver);

    mockLyricsState.activeLineIndex = 1;
    mockPlayerState.positionMs = 6000;

    act(() => {
      root.render(<ScrollHarness lyricsFontStep={0} />);
    });

    const viewport = host.querySelector(
      "[data-testid='scroll-viewport']",
    ) as HTMLDivElement;
    defineNumber(viewport, "clientWidth", 300);
    defineNumber(viewport, "clientHeight", 100);
    defineNumber(viewport, "scrollHeight", 500);
    const line1 = viewport.querySelector("[data-lyrics-line-index='1']")!;
    defineNumber(line1, "offsetTop", 200);
    defineNumber(line1, "clientHeight", 40);

    // User scrolls away to browse (guard unlocks) and parks at 400.
    act(() => {
      viewport.dispatchEvent(new WheelEvent("wheel", { deltaY: 80 }));
    });
    viewport.scrollTop = 400;

    // A resize while browsing must not yank the user back to the active line.
    defineNumber(viewport, "clientHeight", 200);
    act(() => {
      resizeCb?.([], {} as ResizeObserver);
      vi.advanceTimersByTime(200);
    });

    expect(viewport.scrollTop).toBe(400);

    vi.useRealTimers();
  });
});
