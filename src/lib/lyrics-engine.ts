import {
  FOCUS_ALIGN_POSITION,
  collectLineSnapScrollTops,
  getScrollTopForLineIndex,
  resolveLyricScrollLanding,
  stepLineSnapScrollTop,
} from "@/components/Lyrics/lyrics-scroll";
import type { LyricsLineRuntime } from "@/lib/lyrics-line-runtime";
import type { LyricsScrollControl, LyricsSession } from "@/lib/lyrics-session";
import { findActiveLyricLineIndex } from "@/lib/lyrics-timing";
import type { Spring } from "@/lib/spring";

const USER_SCROLL_PAUSE_MS = 4000;
const SEEK_JUMP_MS = 400;
const COAST_SETTLE_MS = 64;

export { USER_SCROLL_PAUSE_MS, SEEK_JUMP_MS, COAST_SETTLE_MS };

export interface LyricScrollCoastSample {
  scrollTop: number;
  velocityPxPerSec: number;
}

export interface UserScrollGuard {
  isActive: () => boolean;
  clear: () => void;
  unlockWithIdleRelock: () => void;
  withProgrammatic: (fn: () => void) => void;
  destroy: () => void;
}

export function createUserScrollGuard(
  container: HTMLElement,
  pauseMs: number,
  options: {
    scrollControl: LyricsScrollControl;
    timers?: {
      setTimeout: typeof globalThis.setTimeout;
      clearTimeout: typeof globalThis.clearTimeout;
    };
    onActiveChange?: (active: boolean) => void;
    onIdleRelock?: () => void;
    onCoast?: (sample: LyricScrollCoastSample) => void;
    onDiscreteStep?: (direction: 1 | -1) => void;
    onUserGesture?: () => void;
  },
): UserScrollGuard {
  const scrollControl = options.scrollControl;
  const timers = options.timers ?? {
    setTimeout: globalThis.setTimeout.bind(globalThis),
    clearTimeout: globalThis.clearTimeout.bind(globalThis),
  };
  const onActiveChange = options.onActiveChange;
  const onIdleRelock =
    options.onIdleRelock ?? (() => scrollControl.requestResume());
  const onCoast = options.onCoast;
  const onDiscreteStep = options.onDiscreteStep;
  const onUserGesture = options.onUserGesture;

  let unlocked = false;
  let programmaticDepth = 0;
  let pointerDown = false;
  let lastProgrammaticScrollTop = container.scrollTop;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let coastTimer: ReturnType<typeof setTimeout> | null = null;
  let lastSampleTop = container.scrollTop;
  let lastSampleAt = 0;
  let velocityPxPerSec = 0;
  let coastConsumed = false;

  const setUnlocked = (next: boolean) => {
    if (unlocked === next) {
      return;
    }
    unlocked = next;
    onActiveChange?.(next);
  };

  const armIdleRelock = () => {
    if (timer !== null) timers.clearTimeout(timer);
    timer = timers.setTimeout(() => {
      timer = null;
      setUnlocked(false);
      onIdleRelock();
    }, pauseMs);
  };

  const unlockFromUser = () => {
    if (scrollControl.isUnlockSuppressed()) {
      return;
    }
    onUserGesture?.();
    coastConsumed = false;
    setUnlocked(true);
    armIdleRelock();
  };

  const clearCoast = () => {
    if (coastTimer !== null) {
      timers.clearTimeout(coastTimer);
      coastTimer = null;
    }
  };

  const emitCoast = () => {
    coastTimer = null;
    if (!unlocked || pointerDown || scrollControl.isUnlockSuppressed()) {
      return;
    }
    coastConsumed = true;
    onCoast?.({
      scrollTop: container.scrollTop,
      velocityPxPerSec,
    });
  };

  const scheduleCoast = () => {
    if (!onCoast) {
      return;
    }
    clearCoast();
    coastTimer = timers.setTimeout(emitCoast, COAST_SETTLE_MS);
  };

  const sampleVelocity = () => {
    const now =
      typeof performance !== "undefined" ? performance.now() : Date.now();
    const dtMs = lastSampleAt === 0 ? 0 : now - lastSampleAt;
    if (dtMs > 0 && dtMs < 120) {
      const instant = ((container.scrollTop - lastSampleTop) / dtMs) * 1000;
      velocityPxPerSec = velocityPxPerSec * 0.55 + instant * 0.45;
    }
    lastSampleTop = container.scrollTop;
    lastSampleAt = now;
  };

  const onScroll = () => {
    if (programmaticDepth > 0) {
      lastSampleTop = container.scrollTop;
      lastSampleAt =
        typeof performance !== "undefined" ? performance.now() : Date.now();
      return;
    }
    sampleVelocity();
    if (Math.abs(container.scrollTop - lastProgrammaticScrollTop) < 1) {
      return;
    }
    if (pointerDown) {
      unlockFromUser();
      return;
    }
    if (unlocked && !coastConsumed) {
      scheduleCoast();
    }
  };

  const onWheel = (event: WheelEvent) => {
    if (event.deltaY === 0 && event.deltaX === 0) {
      return;
    }
    const discrete =
      event.deltaMode === WheelEvent.DOM_DELTA_LINE ||
      event.deltaMode === WheelEvent.DOM_DELTA_PAGE;
    if (discrete && onDiscreteStep) {
      event.preventDefault();
      unlockFromUser();
      const direction: 1 | -1 = event.deltaY < 0 ? -1 : 1;
      onDiscreteStep(direction);
      return;
    }
    unlockFromUser();
    scheduleCoast();
  };

  const onTouchMove = () => {
    unlockFromUser();
  };

  const onPointerDown = () => {
    pointerDown = true;
    clearCoast();
  };

  const onPointerUp = () => {
    if (!pointerDown) {
      return;
    }
    pointerDown = false;
    if (unlocked && !coastConsumed) {
      scheduleCoast();
    }
  };

  container.addEventListener("scroll", onScroll, { passive: true });
  container.addEventListener("wheel", onWheel, { passive: false });
  container.addEventListener("touchmove", onTouchMove, { passive: true });
  container.addEventListener("pointerdown", onPointerDown, { passive: true });
  window.addEventListener("pointerup", onPointerUp, { passive: true });
  window.addEventListener("pointercancel", onPointerUp, { passive: true });

  return {
    isActive: () => unlocked,
    clear: () => {
      if (timer !== null) timers.clearTimeout(timer);
      timer = null;
      clearCoast();
      velocityPxPerSec = 0;
      coastConsumed = false;
      setUnlocked(false);
    },
    unlockWithIdleRelock: () => {
      unlockFromUser();
    },
    withProgrammatic: (fn) => {
      programmaticDepth += 1;
      try {
        fn();
        lastProgrammaticScrollTop = container.scrollTop;
      } finally {
        programmaticDepth -= 1;
      }
    },
    destroy: () => {
      container.removeEventListener("scroll", onScroll);
      container.removeEventListener("wheel", onWheel);
      container.removeEventListener("touchmove", onTouchMove);
      container.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("pointerup", onPointerUp);
      window.removeEventListener("pointercancel", onPointerUp);
      if (timer !== null) timers.clearTimeout(timer);
      clearCoast();
      pointerDown = false;
      unlocked = false;
      velocityPxPerSec = 0;
    },
  };
}

export function computeLineChangeLyricsScrollTop(
  container: HTMLElement,
  lines: { time_ms: number }[],
  adjustedMs: number,
  alignPosition?: number,
): number | null {
  if (lines.length === 0) {
    return null;
  }

  const activeIndex = findActiveLyricLineIndex(lines, adjustedMs);
  if (activeIndex < 0) {
    return getScrollTopForLineIndex(container, 0, alignPosition);
  }

  return getScrollTopForLineIndex(container, activeIndex, alignPosition);
}

export function isLyricsPlaybackSeekJump(
  previousAdjustedMs: number | null,
  adjustedMs: number,
  dtSeconds: number,
): boolean {
  if (previousAdjustedMs === null) {
    return false;
  }
  const delta = Math.abs(adjustedMs - previousAdjustedMs);
  // 2× realtime + slack for timer jitter / IPC catch-up.
  const maxNaturalAdvanceMs = dtSeconds * 1000 * 2 + 50;
  return delta > Math.max(SEEK_JUMP_MS, maxNaturalAdvanceMs);
}

export function beginUserLineSnap(input: {
  container: HTMLElement;
  scrollState: LyricsEngineScrollState;
  lineCount: number;
  alignPosition?: number;
  position: number;
  velocityPxPerSec: number;
  reducedMotion: boolean;
}): void {
  const snaps = collectLineSnapScrollTops(
    input.container,
    input.lineCount,
    input.alignPosition,
  );
  const landing = resolveLyricScrollLanding(
    snaps,
    input.position,
    input.velocityPxPerSec,
  );
  if (landing === null || !input.scrollState.userSnapTopRef) {
    return;
  }
  input.scrollState.userSnapTopRef.current = landing;
  if (
    input.reducedMotion ||
    (Math.abs(landing - input.position) < 0.5 &&
      Math.abs(input.velocityPxPerSec) < 12)
  ) {
    input.scrollState.scrollSpring.jumpTo(landing);
    return;
  }
  input.scrollState.scrollSpring.syncPosition(input.position);
  input.scrollState.scrollSpring.setVelocity(input.velocityPxPerSec);
  input.scrollState.scrollSpring.setTarget(landing);
}

export function beginUserLineStep(input: {
  container: HTMLElement;
  scrollState: LyricsEngineScrollState;
  lineCount: number;
  alignPosition?: number;
  position: number;
  direction: 1 | -1;
  reducedMotion: boolean;
}): void {
  const snaps = collectLineSnapScrollTops(
    input.container,
    input.lineCount,
    input.alignPosition,
  );
  const landing = stepLineSnapScrollTop(snaps, input.position, input.direction);
  if (landing === null || !input.scrollState.userSnapTopRef) {
    return;
  }
  input.scrollState.userSnapTopRef.current = landing;
  if (input.reducedMotion || Math.abs(landing - input.position) < 0.5) {
    input.scrollState.scrollSpring.jumpTo(landing);
    return;
  }
  input.scrollState.scrollSpring.syncPosition(input.position);
  input.scrollState.scrollSpring.setVelocity(0);
  input.scrollState.scrollSpring.setTarget(landing);
}

function bindSpringToViewport(
  scrollSpring: Spring,
  container: HTMLElement,
): void {
  const currentScrollTop = container.scrollTop;
  if (scrollSpring.getPosition() !== currentScrollTop) {
    scrollSpring.jumpTo(currentScrollTop);
  }
}

export interface LyricsEngineScrollState {
  scrollSpring: Spring;
  targetScrollTopRef: { current: number | null };
  prevActiveIndexRef: { current: number };
  prevAdjustedMsRef: { current: number | null };
  lastResumeGenerationRef: { current: number };
  userSnapTopRef?: { current: number | null };
}

export function tickLyricsEngineScroll(input: {
  container: HTMLElement;
  lines: { time_ms: number }[];
  adjustedMs: number;
  isSeek?: boolean;
  scrollState: LyricsEngineScrollState;
  scrollControl: LyricsScrollControl;
  userScrollGuard: UserScrollGuard | null;
  reducedMotion: boolean;
  dt: number;
  audienceMode?: boolean;
  alignPosition?: number;
}): void {
  const {
    container,
    lines,
    adjustedMs,
    isSeek = false,
    scrollState,
    scrollControl,
    userScrollGuard,
    reducedMotion,
    dt,
    audienceMode = false,
    alignPosition,
  } = input;
  const {
    scrollSpring,
    targetScrollTopRef,
    prevActiveIndexRef,
    prevAdjustedMsRef,
    lastResumeGenerationRef,
    userSnapTopRef,
  } = scrollState;

  const seekJump = isLyricsPlaybackSeekJump(
    prevAdjustedMsRef.current,
    adjustedMs,
    dt,
  );
  prevAdjustedMsRef.current = adjustedMs;

  const resumeGeneration = scrollControl.peekResumeGeneration();
  const explicitResume = resumeGeneration !== lastResumeGenerationRef.current;
  if (explicitResume) {
    lastResumeGenerationRef.current = resumeGeneration;
  }

  const shouldResetScroll = isSeek || seekJump || explicitResume;

  const audienceSeekUnlock = isSeek && audienceMode && userScrollGuard !== null;

  const writeScrollTop = (value: number) => {
    if (userScrollGuard) {
      userScrollGuard.withProgrammatic(() => {
        container.scrollTop = value;
      });
    } else {
      container.scrollTop = value;
    }
  };

  if (shouldResetScroll) {
    if (!audienceSeekUnlock) {
      userScrollGuard?.clear();
    }
    prevActiveIndexRef.current = -1;
    targetScrollTopRef.current = null;
    if (userSnapTopRef) {
      userSnapTopRef.current = null;
    }
  }

  if (!audienceSeekUnlock && userScrollGuard?.isActive()) {
    const userSnapTop = userSnapTopRef?.current ?? null;
    if (userSnapTop !== null) {
      if (reducedMotion) {
        scrollSpring.jumpTo(userSnapTop);
        writeScrollTop(userSnapTop);
        targetScrollTopRef.current = userSnapTop;
        return;
      }
      if (!scrollSpring.isSettled()) {
        scrollSpring.update(dt);
      }
      writeScrollTop(scrollSpring.getPosition());
      targetScrollTopRef.current = userSnapTop;
      return;
    }
    bindSpringToViewport(scrollSpring, container);
    targetScrollTopRef.current = container.scrollTop;
    return;
  }

  const activeIndex = findActiveLyricLineIndex(lines, adjustedMs);
  const target = computeLineChangeLyricsScrollTop(
    container,
    lines,
    adjustedMs,
    alignPosition,
  );

  if (target === null) {
    if (shouldResetScroll) {
      scrollControl.endUnlockSuppress();
    }
    if (audienceSeekUnlock && userScrollGuard) {
      userScrollGuard.unlockWithIdleRelock();
    }
    return;
  }

  if (reducedMotion || shouldResetScroll) {
    const snapTarget =
      shouldResetScroll || activeIndex !== prevActiveIndexRef.current
        ? target
        : (targetScrollTopRef.current ?? target);
    scrollSpring.jumpTo(snapTarget);
    writeScrollTop(snapTarget);
    targetScrollTopRef.current = snapTarget;
    prevActiveIndexRef.current = activeIndex;
    if (shouldResetScroll) {
      scrollControl.endUnlockSuppress();
    }
    if (audienceSeekUnlock && userScrollGuard) {
      userScrollGuard.unlockWithIdleRelock();
    }
    return;
  }

  if (activeIndex !== prevActiveIndexRef.current) {
    bindSpringToViewport(scrollSpring, container);
    prevActiveIndexRef.current = activeIndex;
    targetScrollTopRef.current = target;
    scrollSpring.setTarget(target);
  }

  if (!scrollSpring.isSettled()) {
    scrollSpring.update(dt);
  }

  writeScrollTop(scrollSpring.getPosition());

  if (audienceSeekUnlock && userScrollGuard) {
    userScrollGuard.unlockWithIdleRelock();
  }
}

export interface LyricsEngineFrameInput {
  container: HTMLElement | null;
  isPlainText: boolean;
  scrollState: LyricsEngineScrollState;
  userScrollGuard: UserScrollGuard | null;
  session: LyricsSession;
  lineRuntime: LyricsLineRuntime;
  reducedMotion: boolean;
  dt: number;
  positionMs: number;

  isSeek: boolean;
  hasSong: boolean;
  isPlaying: boolean;
  audienceMode?: boolean;
  focusStage?: boolean;
}

export function tickLyricsEngineFrame(input: LyricsEngineFrameInput): void {
  const {
    container,
    isPlainText,
    scrollState,
    userScrollGuard,
    session,
    lineRuntime,
    reducedMotion,
    dt,
    positionMs,
    isSeek,
    hasSong,
    isPlaying,
    audienceMode = false,
    focusStage = false,
  } = input;

  const adjustedMs = session.toAdjustedMs(positionMs);

  if (hasSong) {
    session.syncActiveLine(adjustedMs);
    session.syncActiveWord(adjustedMs);
  }

  const { activeLineIndex, lines } = session.getState();

  lineRuntime.tick({
    activeLineIndex,
    adjustedMs,
    isPlaying,
    dt,
    isPlainText,
    stage: focusStage ? "focus" : "list",
    viewportEl: container,
  });

  if (isPlainText || !container || lines.length === 0) {
    return;
  }

  tickLyricsEngineScroll({
    container,
    lines,
    adjustedMs,
    isSeek,
    scrollState,
    scrollControl: session.scroll,
    userScrollGuard,
    reducedMotion,
    dt,
    audienceMode,
    alignPosition: focusStage ? FOCUS_ALIGN_POSITION : undefined,
  });
}

export function shouldRunLyricsEngineLoop(playerState: {
  snapshot: { song_id: string | null } | null;
}): boolean {
  return Boolean(playerState.snapshot?.song_id);
}
