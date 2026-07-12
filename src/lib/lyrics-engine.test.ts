// @vitest-environment jsdom

import { beforeEach, describe, expect, test, vi } from "vitest";

vi.mock("@/stores/player-store", () => ({
  usePlayerStore: { getState: vi.fn() },
  selectCurrentPositionMs: vi.fn(),
}));

vi.mock("@/stores/lyrics-store", () => ({
  useLyricsStore: { getState: vi.fn() },
}));

import { Spring } from "@/lib/spring";
import { usePlayerStore, selectCurrentPositionMs } from "@/stores/player-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import {
  computeLineChangeLyricsScrollTop,
  createUserScrollGuard,
  endLyricsAutoScrollUnlockSuppress,
  isLyricsPlaybackSeekJump,
  peekLyricsAutoScrollResumeGeneration,
  readLyricsAdjustedPlaybackMs,
  requestLyricsAutoScrollResume,
  resetLyricsEngineScrollControlForTests,
  syncLyricsActiveLine,
  tickLyricsEngineScroll,
  USER_SCROLL_PAUSE_MS,
} from "./lyrics-engine";
import { resetLyricsPlaybackTimeForTests } from "./lyrics-playback-time";

const PAUSE_MS = USER_SCROLL_PAUSE_MS;

function makeContainer(): HTMLDivElement {
  return document.createElement("div");
}

beforeEach(() => {
  // Module-level resume generation / unlock suppress / seek latch leak across
  // cases and can make a later test pass via the wrong (explicit-resume) path.
  resetLyricsEngineScrollControlForTests();
  resetLyricsPlaybackTimeForTests();
});

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

  test("unlocks on wheel and re-locks after the idle timeout", () => {
    const container = makeContainer();
    const onActiveChange = vi.fn();
    const onIdleRelock = vi.fn();
    const guard = createUserScrollGuard(container, PAUSE_MS, {
      onActiveChange,
      onIdleRelock,
    });

    container.dispatchEvent(new WheelEvent("wheel", { deltaY: 40 }));
    expect(guard.isActive()).toBe(true);
    expect(onActiveChange).toHaveBeenCalledWith(true);

    vi.advanceTimersByTime(PAUSE_MS);
    expect(guard.isActive()).toBe(false);
    expect(onActiveChange).toHaveBeenCalledWith(false);
    expect(onIdleRelock).toHaveBeenCalledTimes(1);

    guard.destroy();
  });

  test("idle relock requests auto-scroll resume so viewport re-anchors", () => {
    // REGRESSION: clearing unlocked alone hid the Follow button while the
    // spring stayed parked at the user's browse offset until the next line
    // change (long lines / instrumental gaps). Idle must bump resume gen.
    const container = makeContainer();
    const before = peekLyricsAutoScrollResumeGeneration();
    const guard = createUserScrollGuard(container, PAUSE_MS);

    container.dispatchEvent(new WheelEvent("wheel", { deltaY: 40 }));
    expect(guard.isActive()).toBe(true);

    vi.advanceTimersByTime(PAUSE_MS);
    expect(guard.isActive()).toBe(false);
    expect(peekLyricsAutoScrollResumeGeneration()).toBe(before + 1);

    guard.destroy();
  });

  test("unlocks on native scroll events but ignores programmatic writes", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS);

    guard.withProgrammatic(() => {
      container.scrollTop = 120;
      container.dispatchEvent(new Event("scroll"));
    });
    expect(guard.isActive()).toBe(false);

    container.scrollTop = 200;
    container.dispatchEvent(new Event("scroll"));
    expect(guard.isActive()).toBe(true);

    guard.destroy();
  });

  test("clear re-locks immediately (Follow / resetScroll)", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS);

    container.dispatchEvent(new WheelEvent("wheel", { deltaY: 40 }));
    expect(guard.isActive()).toBe(true);

    guard.clear();
    expect(guard.isActive()).toBe(false);

    guard.destroy();
  });

  test("does not unlock from touchstart or while resume is suppressed", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS);

    requestLyricsAutoScrollResume();
    container.dispatchEvent(new Event("touchstart"));
    container.scrollTop = 80;
    container.dispatchEvent(new Event("scroll"));
    expect(guard.isActive()).toBe(false);

    // Engine finished the resume snap.
    endLyricsAutoScrollUnlockSuppress();

    container.dispatchEvent(new WheelEvent("wheel", { deltaY: 20 }));
    expect(guard.isActive()).toBe(true);

    guard.destroy();
  });
});

describe("isLyricsPlaybackSeekJump", () => {
  test("ignores the first sample and natural frame advances", () => {
    expect(isLyricsPlaybackSeekJump(null, 0, 0.016)).toBe(false);
    expect(isLyricsPlaybackSeekJump(1000, 1016, 0.016)).toBe(false);
  });

  test("detects discontinuous seek jumps", () => {
    expect(isLyricsPlaybackSeekJump(1000, 15000, 0.016)).toBe(true);
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
  function makeScrollFixture() {
    const container = document.createElement("div");
    Object.defineProperty(container, "clientHeight", { value: 100 });
    Object.defineProperty(container, "scrollHeight", { value: 500 });

    const line0 = document.createElement("div");
    line0.dataset.lyricsLineIndex = "0";
    Object.defineProperty(line0, "offsetTop", { value: 0 });
    Object.defineProperty(line0, "clientHeight", { value: 40 });

    const line1 = document.createElement("div");
    line1.dataset.lyricsLineIndex = "1";
    Object.defineProperty(line1, "offsetTop", { value: 200 });
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
      prevAdjustedMsRef: { current: 0 as number | null },
      lastResumeGenerationRef: { current: 0 },
    };

    return { container, scrollSpring, scrollState };
  }

  test("does not retarget mid-line when layout measurements jitter", () => {
    // REGRESSION: per-character emphasis / font-weight swaps reflow the active
    // line while it stays current. Recomputing scrollTop every time the pixel
    // target drifted made the spring chase a moving target; pausing froze the
    // DOM and "snapped" back to the right place. Anchor once per active line.
    const container = document.createElement("div");
    Object.defineProperty(container, "clientHeight", { value: 100 });
    Object.defineProperty(container, "scrollHeight", {
      value: 800,
      configurable: true,
    });

    const line0 = document.createElement("div");
    line0.dataset.lyricsLineIndex = "0";
    Object.defineProperty(line0, "offsetTop", { value: 0, configurable: true });
    Object.defineProperty(line0, "clientHeight", {
      value: 40,
      configurable: true,
    });

    const line1 = document.createElement("div");
    line1.dataset.lyricsLineIndex = "1";
    Object.defineProperty(line1, "offsetTop", {
      value: 200,
      configurable: true,
    });
    Object.defineProperty(line1, "clientHeight", {
      value: 40,
      configurable: true,
    });

    container.append(line0, line1);

    const scrollSpring = new Spring(0, {
      stiffness: 170,
      damping: 28,
      mass: 1,
    });
    const scrollState = {
      scrollSpring,
      targetScrollTopRef: { current: null as number | null },
      prevActiveIndexRef: { current: -1 },
      prevAdjustedMsRef: { current: null as number | null },
      lastResumeGenerationRef: { current: 0 },
    };

    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 1000 }],
      adjustedMs: 1000,
      scrollState,
      userScrollGuard: null,
      reducedMotion: true,
      dt: 0.016,
    });

    expect(scrollState.prevActiveIndexRef.current).toBe(1);
    expect(scrollState.targetScrollTopRef.current).toBe(170);
    expect(container.scrollTop).toBe(170);

    // Simulate mid-line reflow (emphasis / weight) shifting geometry.
    Object.defineProperty(line1, "offsetTop", {
      value: 230,
      configurable: true,
    });
    Object.defineProperty(line1, "clientHeight", {
      value: 64,
      configurable: true,
    });

    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 1000 }],
      adjustedMs: 1100,
      scrollState,
      userScrollGuard: null,
      reducedMotion: true,
      dt: 0.016,
    });

    expect(scrollState.prevActiveIndexRef.current).toBe(1);
    expect(scrollState.targetScrollTopRef.current).toBe(170);
    expect(container.scrollTop).toBe(170);
  });

  test("re-anchors when the active line changes even if the pixel target matches", () => {
    const container = document.createElement("div");
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
      prevAdjustedMsRef: { current: null as number | null },
      lastResumeGenerationRef: { current: 0 },
    };

    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 1000 }],
      adjustedMs: 0,
      scrollState,
      userScrollGuard: null,
      reducedMotion: true,
      dt: 0.016,
    });

    expect(scrollState.prevActiveIndexRef.current).toBe(0);
    expect(container.scrollTop).toBe(0);

    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 1000 }],
      adjustedMs: 1000,
      scrollState,
      userScrollGuard: null,
      reducedMotion: true,
      dt: 0.016,
    });

    expect(scrollState.prevActiveIndexRef.current).toBe(1);
    expect(container.scrollTop).toBe(0);
  });

  test("tracks viewport scrollTop while the user-scroll guard is active", () => {
    const { container, scrollSpring, scrollState } = makeScrollFixture();

    container.scrollTop = 180;

    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }],
      adjustedMs: 0,
      scrollState,
      userScrollGuard: {
        isActive: () => true,
        clear: () => {},
        withProgrammatic: (fn) => fn(),
        destroy: () => {},
      },
      reducedMotion: false,
      dt: 0.016,
    });

    expect(container.scrollTop).toBe(180);
    expect(scrollSpring.getPosition()).toBe(180);
    expect(scrollState.targetScrollTopRef.current).toBe(180);
  });

  test("explicit isSeek resets scroll without waiting for a clock jump", () => {
    const { container, scrollSpring, scrollState } = makeScrollFixture();
    let guardActive = true;
    const guard = {
      isActive: () => guardActive,
      clear: () => {
        guardActive = false;
      },
      withProgrammatic: (fn: () => void) => fn(),
      destroy: () => {},
    };

    scrollState.prevAdjustedMsRef.current = 1000;
    container.scrollTop = 40;
    scrollSpring.jumpTo(40);
    scrollState.targetScrollTopRef.current = 40;
    scrollState.prevActiveIndexRef.current = 0;

    // Host marked seek; clock has not jumped yet (async transport in flight).
    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 15000 }],
      adjustedMs: 1000,
      isSeek: true,
      scrollState,
      userScrollGuard: guard,
      reducedMotion: false,
      dt: 0.016,
    });

    expect(guardActive).toBe(false);
    expect(container.scrollTop).toBe(0);
  });

  test("seek jump clears the user-scroll guard and snaps to the new line", () => {
    const { container, scrollSpring, scrollState } = makeScrollFixture();
    let guardActive = true;
    const guard = {
      isActive: () => guardActive,
      clear: () => {
        guardActive = false;
      },
      withProgrammatic: (fn: () => void) => fn(),
      destroy: () => {},
    };

    scrollState.prevAdjustedMsRef.current = 1000;
    container.scrollTop = 40;
    scrollSpring.jumpTo(40);
    scrollState.targetScrollTopRef.current = 40;
    scrollState.prevActiveIndexRef.current = 0;

    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 15000 }],
      adjustedMs: 15000,
      scrollState,
      userScrollGuard: guard,
      reducedMotion: false,
      dt: 0.016,
    });

    expect(guardActive).toBe(false);
    expect(scrollState.prevActiveIndexRef.current).toBe(1);
    expect(container.scrollTop).toBe(170);
    expect(scrollSpring.getPosition()).toBe(170);
  });

  test("explicit resume from line click clears guard and keeps writing scrollTop", () => {
    const { container, scrollSpring, scrollState } = makeScrollFixture();
    let guardActive = true;
    const guard = {
      isActive: () => guardActive,
      clear: () => {
        guardActive = false;
      },
      withProgrammatic: (fn: () => void) => fn(),
      destroy: () => {},
    };

    scrollState.prevAdjustedMsRef.current = 1000;
    container.scrollTop = 0;
    scrollSpring.jumpTo(0);
    scrollState.prevActiveIndexRef.current = 0;

    requestLyricsAutoScrollResume();

    // Clock has not jumped yet (seek still in flight) — resume must still work.
    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 15000 }],
      adjustedMs: 1000,
      scrollState,
      userScrollGuard: guard,
      reducedMotion: false,
      dt: 0.016,
    });

    expect(guardActive).toBe(false);
    expect(container.scrollTop).toBe(0);

    // Seek lands; keep auto-scrolling on later line changes.
    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 15000 }],
      adjustedMs: 15000,
      scrollState,
      userScrollGuard: guard,
      reducedMotion: false,
      dt: 0.016,
    });

    expect(container.scrollTop).toBe(170);
    expect(scrollSpring.getPosition()).toBe(170);

    // Settled spring must still re-assert scrollTop if the browser moves it.
    container.scrollTop = 0;
    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 15000 }],
      adjustedMs: 15016,
      scrollState,
      userScrollGuard: null,
      reducedMotion: false,
      dt: 0.016,
    });
    expect(container.scrollTop).toBe(170);
  });

  test("does not overwrite scrollTop while the user has unlocked follow", () => {
    const { container, scrollSpring, scrollState } = makeScrollFixture();
    scrollSpring.jumpTo(0);
    scrollState.prevAdjustedMsRef.current = 0;
    container.scrollTop = 180;

    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 15000 }],
      adjustedMs: 15000,
      scrollState,
      userScrollGuard: {
        isActive: () => true,
        clear: () => {},
        withProgrammatic: (fn) => fn(),
        destroy: () => {},
      },
      reducedMotion: false,
      dt: 0.016,
    });

    // Unlocked: leave the user's scroll position alone even if the playing
    // line would target a different offset.
    expect(container.scrollTop).toBe(180);
  });

  test("idle relock resume snaps scrollTop back while the active line is unchanged", () => {
    // Long active line (same index across the idle window): after the user
    // browses away and the pause expires, resume must re-anchor even though
    // activeIndex === prevActiveIndexRef.
    const { container, scrollSpring, scrollState } = makeScrollFixture();
    const lines = [{ time_ms: 0 }, { time_ms: 30_000 }];

    // Playing line 0; spring already settled on the correct target.
    scrollSpring.jumpTo(0);
    scrollState.targetScrollTopRef.current = 0;
    scrollState.prevActiveIndexRef.current = 0;
    scrollState.prevAdjustedMsRef.current = 1000;
    scrollState.lastResumeGenerationRef.current =
      peekLyricsAutoScrollResumeGeneration();
    container.scrollTop = 0;

    // User browses far away while still on line 0.
    container.scrollTop = 180;
    scrollSpring.jumpTo(180);
    scrollState.targetScrollTopRef.current = 180;

    let guardActive = true;
    const guard = {
      isActive: () => guardActive,
      clear: () => {
        guardActive = false;
      },
      withProgrammatic: (fn: () => void) => fn(),
      destroy: () => {},
    };

    // Idle timeout → re-lock + resume generation (what createUserScrollGuard does).
    guardActive = false;
    requestLyricsAutoScrollResume();

    tickLyricsEngineScroll({
      container,
      lines,
      adjustedMs: 2000, // still line 0
      scrollState,
      userScrollGuard: guard,
      reducedMotion: true, // snap without spring settle loop
      dt: 0.016,
    });

    expect(guardActive).toBe(false);
    expect(container.scrollTop).toBe(0);
    expect(scrollSpring.getPosition()).toBe(0);
  });

  test("continues scrolling after a seek snap on subsequent line changes", () => {
    const container = document.createElement("div");
    Object.defineProperty(container, "clientHeight", { value: 100 });
    Object.defineProperty(container, "scrollHeight", {
      value: 800,
      configurable: true,
    });

    const line0 = document.createElement("div");
    line0.dataset.lyricsLineIndex = "0";
    Object.defineProperty(line0, "offsetTop", { value: 0 });
    Object.defineProperty(line0, "clientHeight", { value: 40 });

    const line1 = document.createElement("div");
    line1.dataset.lyricsLineIndex = "1";
    Object.defineProperty(line1, "offsetTop", { value: 200 });
    Object.defineProperty(line1, "clientHeight", { value: 40 });

    const line2 = document.createElement("div");
    line2.dataset.lyricsLineIndex = "2";
    Object.defineProperty(line2, "offsetTop", { value: 350 });
    Object.defineProperty(line2, "clientHeight", { value: 40 });

    container.append(line0, line1, line2);

    const scrollSpring = new Spring(0, {
      stiffness: 170,
      damping: 28,
      mass: 1,
    });
    const scrollState = {
      scrollSpring,
      targetScrollTopRef: { current: null as number | null },
      prevActiveIndexRef: { current: -1 },
      prevAdjustedMsRef: { current: 0 as number | null },
      lastResumeGenerationRef: { current: 0 },
    };

    // Seek jump to line 1
    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 1000 }, { time_ms: 2000 }],
      adjustedMs: 1000,
      scrollState,
      userScrollGuard: null,
      reducedMotion: false,
      dt: 0.016,
    });
    expect(container.scrollTop).toBe(170);
    expect(scrollState.prevActiveIndexRef.current).toBe(1);

    // Natural advance still on line 1
    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 1000 }, { time_ms: 2000 }],
      adjustedMs: 1016,
      scrollState,
      userScrollGuard: null,
      reducedMotion: false,
      dt: 0.016,
    });
    expect(scrollState.prevActiveIndexRef.current).toBe(1);

    // Next line — must keep auto-scrolling after the seek
    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 1000 }, { time_ms: 2000 }],
      adjustedMs: 2000,
      scrollState,
      userScrollGuard: null,
      reducedMotion: true,
      dt: 0.016,
    });

    expect(scrollState.prevActiveIndexRef.current).toBe(2);
    expect(container.scrollTop).toBe(320);
  });
});

describe("syncLyricsActiveLine", () => {
  test("updates the active line while playing", () => {
    const setActiveLineIndex = vi.fn();
    vi.mocked(useLyricsStore.getState).mockReturnValue({
      lines: [
        { time_ms: 0, text: "Intro" },
        { time_ms: 1000, text: "Line one" },
        { time_ms: 2000, text: "Line two" },
        { time_ms: 3000, text: "Line three" },
      ],
      offsetMs: 0,
      setActiveLineIndex,
    } as unknown as ReturnType<(typeof useLyricsStore)["getState"]>);
    vi.mocked(usePlayerStore.getState).mockReturnValue({
      snapshot: { song_id: "song-1", is_playing: true },
      positionMs: 2500,
      playingSinceMs: 1000,
    } as ReturnType<(typeof usePlayerStore)["getState"]>);
    vi.mocked(selectCurrentPositionMs).mockReturnValue(2500);

    const ref = { current: -1 };
    syncLyricsActiveLine(ref, 2500);

    expect(setActiveLineIndex).toHaveBeenCalledWith(2);
    expect(ref.current).toBe(2);
  });
});

describe("readLyricsAdjustedPlaybackMs", () => {
  test("returns positionMs - offsetMs when not in AirPlay mode", () => {
    vi.mocked(usePlayerStore.getState).mockReturnValue({
      snapshot: null,
      positionMs: 5000,
      playingSinceMs: null,
      airPlayOutput: { active: false, displayedPositionMs: null },
    } as ReturnType<(typeof usePlayerStore)["getState"]>);
    vi.mocked(selectCurrentPositionMs).mockReturnValue(5000);
    vi.mocked(useLyricsStore.getState).mockReturnValue({
      offsetMs: 200,
    } as ReturnType<(typeof useLyricsStore)["getState"]>);

    expect(readLyricsAdjustedPlaybackMs()).toBe(4800);
  });
});
