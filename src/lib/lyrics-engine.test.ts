// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { LyricsScrollControl } from "@/lib/lyrics-session";
import { Spring } from "@/lib/spring";
import {
  beginUserLineSnap,
  COAST_SETTLE_MS,
  computeLineChangeLyricsScrollTop,
  createUserScrollGuard,
  isLyricsPlaybackSeekJump,
  tickLyricsEngineScroll,
  USER_SCROLL_PAUSE_MS,
} from "./lyrics-engine";

const PAUSE_MS = USER_SCROLL_PAUSE_MS;

function makeContainer(): HTMLDivElement {
  return document.createElement("div");
}

let scrollControl: LyricsScrollControl;

beforeEach(() => {
  scrollControl = new LyricsScrollControl();
});

describe("createUserScrollGuard", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  test("is inactive before any user interaction", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS, { scrollControl });

    expect(guard.isActive()).toBe(false);

    guard.destroy();
  });

  test("default global timers are bound (no browser Illegal invocation)", () => {
    vi.useRealTimers();
    const container = makeContainer();
    const onIdleRelock = vi.fn();
    const guard = createUserScrollGuard(container, 30, {
      scrollControl,
      onIdleRelock,
    });

    expect(() => {
      container.dispatchEvent(new WheelEvent("wheel", { deltaY: 40 }));
    }).not.toThrow();
    expect(guard.isActive()).toBe(true);

    return new Promise<void>((resolve) => {
      setTimeout(() => {
        expect(guard.isActive()).toBe(false);
        expect(onIdleRelock).toHaveBeenCalledTimes(1);
        guard.destroy();
        resolve();
      }, 80);
    });
  });

  test("unlocks on wheel and re-locks after the idle timeout", () => {
    const container = makeContainer();
    const onActiveChange = vi.fn();
    const onIdleRelock = vi.fn();
    const guard = createUserScrollGuard(container, PAUSE_MS, {
      scrollControl,
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
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS, { scrollControl });

    container.dispatchEvent(new WheelEvent("wheel", { deltaY: 40 }));
    expect(guard.isActive()).toBe(true);

    vi.advanceTimersByTime(PAUSE_MS);
    expect(guard.isActive()).toBe(false);
    expect(scrollControl.peekResumeGeneration()).toBe(1);

    guard.destroy();
  });

  test("ignores layout-driven scroll events and unlocks during a pointer scroll", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS, { scrollControl });

    guard.withProgrammatic(() => {
      container.scrollTop = 120;
      container.dispatchEvent(new Event("scroll"));
    });
    expect(guard.isActive()).toBe(false);

    container.scrollTop = 200;
    container.dispatchEvent(new Event("scroll"));
    expect(guard.isActive()).toBe(false);

    container.dispatchEvent(new Event("pointerdown"));
    container.scrollTop = 260;
    container.dispatchEvent(new Event("scroll"));
    expect(guard.isActive()).toBe(true);
    window.dispatchEvent(new Event("pointerup"));

    guard.destroy();
  });

  test("unlocks on touchmove without treating touchstart as a scroll", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS, { scrollControl });

    container.dispatchEvent(new Event("touchstart"));
    expect(guard.isActive()).toBe(false);

    container.dispatchEvent(new Event("touchmove"));
    expect(guard.isActive()).toBe(true);

    guard.destroy();
  });

  test("clear re-locks immediately (Follow / resetScroll)", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS, { scrollControl });

    container.dispatchEvent(new WheelEvent("wheel", { deltaY: 40 }));
    expect(guard.isActive()).toBe(true);

    guard.clear();
    expect(guard.isActive()).toBe(false);

    guard.destroy();
  });

  test("does not unlock from touch input while resume is suppressed", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS, { scrollControl });

    scrollControl.requestResume();
    container.dispatchEvent(new Event("touchstart"));
    container.dispatchEvent(new Event("touchmove"));
    container.scrollTop = 80;
    container.dispatchEvent(new Event("scroll"));
    expect(guard.isActive()).toBe(false);

    scrollControl.endUnlockSuppress();

    container.dispatchEvent(new WheelEvent("wheel", { deltaY: 20 }));
    expect(guard.isActive()).toBe(true);

    guard.destroy();
  });

  test("ignores zero-delta wheel and no-op scroll noise", () => {
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS, { scrollControl });

    container.dispatchEvent(new WheelEvent("wheel", { deltaY: 0, deltaX: 0 }));
    expect(guard.isActive()).toBe(false);

    container.scrollTop = 0;
    container.dispatchEvent(new Event("scroll"));
    expect(guard.isActive()).toBe(false);

    const onActiveChange = vi.fn();
    const guard2 = createUserScrollGuard(container, PAUSE_MS, {
      scrollControl,
      onActiveChange,
    });
    guard2.clear();
    expect(onActiveChange).not.toHaveBeenCalled();
    guard2.destroy();

    guard.destroy();
  });

  test("re-arms idle when wheel continues after an earlier unlock", () => {
    const onIdleRelock = vi.fn();
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS, {
      scrollControl,
      onIdleRelock,
    });

    container.dispatchEvent(new WheelEvent("wheel", { deltaY: 20 }));
    vi.advanceTimersByTime(PAUSE_MS - 100);
    container.dispatchEvent(new WheelEvent("wheel", { deltaY: 20 }));
    vi.advanceTimersByTime(PAUSE_MS - 100);
    expect(guard.isActive()).toBe(true);
    expect(onIdleRelock).not.toHaveBeenCalled();

    vi.advanceTimersByTime(100);
    expect(guard.isActive()).toBe(false);
    expect(onIdleRelock).toHaveBeenCalledTimes(1);

    guard.destroy();
  });

  test("coasts into a snap sample after the flick settles", () => {
    const onCoast = vi.fn();
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS, {
      scrollControl,
      onCoast,
    });

    container.scrollTop = 140;
    container.dispatchEvent(new WheelEvent("wheel", { deltaY: 40 }));
    expect(onCoast).not.toHaveBeenCalled();

    vi.advanceTimersByTime(COAST_SETTLE_MS);
    expect(onCoast).toHaveBeenCalledWith({
      scrollTop: 140,
      velocityPxPerSec: expect.any(Number),
    });

    guard.destroy();
  });

  test("turns a discrete wheel notch into a one-line step", () => {
    const onDiscreteStep = vi.fn();
    const container = makeContainer();
    const guard = createUserScrollGuard(container, PAUSE_MS, {
      scrollControl,
      onDiscreteStep,
    });

    const event = new WheelEvent("wheel", {
      deltaY: 3,
      deltaMode: WheelEvent.DOM_DELTA_LINE,
      cancelable: true,
    });
    container.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    expect(onDiscreteStep).toHaveBeenCalledWith(1);

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
      userSnapTopRef: { current: null as number | null },
    };

    return { container, scrollSpring, scrollState };
  }

  test("does not retarget mid-line when layout measurements jitter", () => {
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
      scrollControl,
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
      scrollControl,
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
      scrollControl,
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
      scrollControl,
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
      scrollControl,
      userScrollGuard: {
        isActive: () => true,
        clear: () => {},
        withProgrammatic: (fn) => fn(),
        unlockWithIdleRelock: () => {},
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
      unlockWithIdleRelock: () => {},
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
      adjustedMs: 1000,
      isSeek: true,
      scrollState,
      scrollControl,
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
      unlockWithIdleRelock: () => {},
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
      scrollControl,
      userScrollGuard: guard,
      reducedMotion: false,
      dt: 0.016,
    });

    expect(guardActive).toBe(false);
    expect(scrollState.prevActiveIndexRef.current).toBe(1);
    expect(container.scrollTop).toBe(170);
    expect(scrollSpring.getPosition()).toBe(170);
  });

  test("audience mode seek snaps to clicked line then unlocks with idle re-lock", () => {
    const { container, scrollSpring, scrollState } = makeScrollFixture();
    let guardActive = false;
    let unlockCalled = false;
    const guard = {
      isActive: () => guardActive,
      clear: () => {
        guardActive = false;
      },
      withProgrammatic: (fn: () => void) => fn(),
      unlockWithIdleRelock: () => {
        guardActive = true;
        unlockCalled = true;
      },
      destroy: () => {},
    };

    scrollState.prevAdjustedMsRef.current = 1000;
    container.scrollTop = 0;
    scrollSpring.jumpTo(0);
    scrollState.prevActiveIndexRef.current = 0;

    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 15000 }],
      adjustedMs: 15000,
      isSeek: true,
      scrollState,
      scrollControl,
      userScrollGuard: guard,
      reducedMotion: false,
      dt: 0.016,
      audienceMode: true,
    });

    expect(container.scrollTop).toBe(170);
    expect(scrollSpring.getPosition()).toBe(170);
    expect(unlockCalled).toBe(true);
    expect(guardActive).toBe(true);
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
      unlockWithIdleRelock: () => {},
      destroy: () => {},
    };

    scrollState.prevAdjustedMsRef.current = 1000;
    container.scrollTop = 0;
    scrollSpring.jumpTo(0);
    scrollState.prevActiveIndexRef.current = 0;

    scrollControl.requestResume();

    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 15000 }],
      adjustedMs: 1000,
      scrollState,
      scrollControl,
      userScrollGuard: guard,
      reducedMotion: false,
      dt: 0.016,
    });

    expect(guardActive).toBe(false);
    expect(container.scrollTop).toBe(0);

    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 15000 }],
      adjustedMs: 15000,
      scrollState,
      scrollControl,
      userScrollGuard: guard,
      reducedMotion: false,
      dt: 0.016,
    });

    expect(container.scrollTop).toBe(170);
    expect(scrollSpring.getPosition()).toBe(170);

    container.scrollTop = 0;
    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 15000 }],
      adjustedMs: 15016,
      scrollState,
      scrollControl,
      userScrollGuard: null,
      reducedMotion: false,
      dt: 0.016,
    });
    expect(container.scrollTop).toBe(170);
  });

  test("springs to a user snap target while follow is still unlocked", () => {
    const { container, scrollSpring, scrollState } = makeScrollFixture();
    scrollState.userSnapTopRef = { current: 170 };
    scrollState.prevAdjustedMsRef.current = 15000;
    scrollState.prevActiveIndexRef.current = 1;
    scrollSpring.syncPosition(80);
    scrollSpring.setTarget(170);
    container.scrollTop = 80;

    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 15000 }],
      adjustedMs: 15000,
      scrollState,
      scrollControl,
      userScrollGuard: {
        isActive: () => true,
        clear: () => {},
        withProgrammatic: (fn) => fn(),
        unlockWithIdleRelock: () => {},
        destroy: () => {},
      },
      reducedMotion: true,
      dt: 0.016,
    });

    expect(container.scrollTop).toBe(170);
    expect(scrollSpring.getPosition()).toBe(170);
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
      scrollControl,
      userScrollGuard: {
        isActive: () => true,
        clear: () => {},
        withProgrammatic: (fn) => fn(),
        unlockWithIdleRelock: () => {},
        destroy: () => {},
      },
      reducedMotion: false,
      dt: 0.016,
    });

    expect(container.scrollTop).toBe(180);
  });

  test("beginUserLineSnap lands a flick on the projected line", () => {
    const { container, scrollSpring, scrollState } = makeScrollFixture();
    scrollState.userSnapTopRef = { current: null };
    beginUserLineSnap({
      container,
      scrollState,
      lineCount: 2,
      position: 40,
      velocityPxPerSec: 900,
      reducedMotion: false,
    });
    expect(scrollState.userSnapTopRef.current).toBe(170);
    expect(scrollSpring.getVelocity()).toBe(900);
    expect(scrollSpring.isSettled()).toBe(false);
  });

  test("idle relock resume snaps scrollTop back while the active line is unchanged", () => {
    const { container, scrollSpring, scrollState } = makeScrollFixture();
    const lines = [{ time_ms: 0 }, { time_ms: 30_000 }];

    scrollSpring.jumpTo(0);
    scrollState.targetScrollTopRef.current = 0;
    scrollState.prevActiveIndexRef.current = 0;
    scrollState.prevAdjustedMsRef.current = 1000;
    scrollState.lastResumeGenerationRef.current =
      scrollControl.peekResumeGeneration();
    container.scrollTop = 0;

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
      unlockWithIdleRelock: () => {},
      destroy: () => {},
    };

    guardActive = false;
    scrollControl.requestResume();

    tickLyricsEngineScroll({
      container,
      lines,
      adjustedMs: 2000, // still line 0
      scrollState,
      scrollControl,
      userScrollGuard: guard,
      reducedMotion: true, // snap without spring settle loop
      dt: 0.016,
    });

    expect(guardActive).toBe(false);
    expect(container.scrollTop).toBe(0);
    expect(scrollSpring.getPosition()).toBe(0);
  });

  test("anchors spring target once when the active line changes without a seek", () => {
    // Cover the non-reduced-motion line-change path (bindSpringToViewport +
    // setTarget) — seek jumps and reducedMotion take a different early return.
    const { container, scrollSpring, scrollState } = makeScrollFixture();
    scrollState.prevActiveIndexRef.current = 0;
    scrollState.prevAdjustedMsRef.current = 900;
    scrollState.targetScrollTopRef.current = 0;
    scrollSpring.jumpTo(0);
    container.scrollTop = 0;

    const setTargetSpy = vi.spyOn(scrollSpring, "setTarget");

    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 1000 }],
      adjustedMs: 1000,
      scrollState,
      scrollControl,
      userScrollGuard: null,
      reducedMotion: false,
      // Large enough that a 100ms advance is still "natural", not a seek jump.
      dt: 0.05,
    });

    expect(scrollState.prevActiveIndexRef.current).toBe(1);
    expect(scrollState.targetScrollTopRef.current).toBe(170);
    expect(setTargetSpy).toHaveBeenCalledWith(170);
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

    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 1000 }, { time_ms: 2000 }],
      adjustedMs: 1000,
      scrollState,
      scrollControl,
      userScrollGuard: null,
      reducedMotion: false,
      dt: 0.016,
    });
    expect(container.scrollTop).toBe(170);
    expect(scrollState.prevActiveIndexRef.current).toBe(1);

    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 1000 }, { time_ms: 2000 }],
      adjustedMs: 1016,
      scrollState,
      scrollControl,
      userScrollGuard: null,
      reducedMotion: false,
      dt: 0.016,
    });
    expect(scrollState.prevActiveIndexRef.current).toBe(1);

    tickLyricsEngineScroll({
      container,
      lines: [{ time_ms: 0 }, { time_ms: 1000 }, { time_ms: 2000 }],
      adjustedMs: 2000,
      scrollState,
      scrollControl,
      userScrollGuard: null,
      reducedMotion: true,
      dt: 0.016,
    });

    expect(scrollState.prevActiveIndexRef.current).toBe(2);
    expect(container.scrollTop).toBe(320);
  });
});
