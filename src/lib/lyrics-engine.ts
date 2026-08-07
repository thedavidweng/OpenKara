import { getScrollTopForLineIndex } from "@/components/Lyrics/lyrics-scroll";
import {
  findActiveLyricLineIndex,
  findActiveWordIndex,
} from "@/lib/lyrics-timing";
import type { LyricsLineRuntime } from "@/lib/lyrics-line-runtime";
import { selectCurrentPositionMs } from "@/stores/player-store";
import { useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";
import type { Spring } from "@/lib/spring";

const USER_SCROLL_PAUSE_MS = 4000;
const SEEK_JUMP_MS = 400;

export { USER_SCROLL_PAUSE_MS, SEEK_JUMP_MS };

let autoScrollResumeGeneration = 0;
let autoScrollUnlockSuppressed = false;

export function requestLyricsAutoScrollResume(): void {
  autoScrollResumeGeneration += 1;
  // Suppress unlock until withProgrammatic writes scrollTop (click scroll-into-view).
  autoScrollUnlockSuppressed = true;
}

export function peekLyricsAutoScrollResumeGeneration(): number {
  return autoScrollResumeGeneration;
}

export function endLyricsAutoScrollUnlockSuppress(): void {
  autoScrollUnlockSuppressed = false;
}

export function resetLyricsEngineScrollControlForTests(): void {
  autoScrollResumeGeneration = 0;
  autoScrollUnlockSuppressed = false;
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
    timers?: {
      setTimeout: typeof globalThis.setTimeout;
      clearTimeout: typeof globalThis.clearTimeout;
    };
    onActiveChange?: (active: boolean) => void;
    onIdleRelock?: () => void;
  } = {},
): UserScrollGuard {
  const timers = options.timers ?? {
    setTimeout: globalThis.setTimeout.bind(globalThis),
    clearTimeout: globalThis.clearTimeout.bind(globalThis),
  };
  const onActiveChange = options.onActiveChange;
  const onIdleRelock =
    options.onIdleRelock ?? (() => requestLyricsAutoScrollResume());

  let unlocked = false;
  let programmaticDepth = 0;
  let pointerDown = false;
  let lastProgrammaticScrollTop = container.scrollTop;
  let timer: ReturnType<typeof setTimeout> | null = null;

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
    if (autoScrollUnlockSuppressed) {
      return;
    }
    setUnlocked(true);
    armIdleRelock();
  };

  const onScroll = () => {
    if (programmaticDepth > 0) {
      return;
    }
    if (Math.abs(container.scrollTop - lastProgrammaticScrollTop) < 1) {
      return;
    }
    if (pointerDown) {
      unlockFromUser();
    }
  };

  const onWheel = (event: WheelEvent) => {
    if (event.deltaY === 0 && event.deltaX === 0) {
      return;
    }
    unlockFromUser();
  };

  const onTouchMove = () => {
    unlockFromUser();
  };

  const onPointerDown = () => {
    pointerDown = true;
  };

  const onPointerUp = () => {
    pointerDown = false;
  };

  container.addEventListener("scroll", onScroll, { passive: true });
  container.addEventListener("wheel", onWheel, { passive: true });
  container.addEventListener("touchmove", onTouchMove, { passive: true });
  container.addEventListener("pointerdown", onPointerDown, { passive: true });
  window.addEventListener("pointerup", onPointerUp, { passive: true });
  window.addEventListener("pointercancel", onPointerUp, { passive: true });

  return {
    isActive: () => unlocked,
    clear: () => {
      if (timer !== null) timers.clearTimeout(timer);
      timer = null;
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
      pointerDown = false;
      unlocked = false;
    },
  };
}

export function readLyricsAdjustedPlaybackMs(
  nowMs = () => performance.now(),
): number {
  const playerState = usePlayerStore.getState();
  const { offsetMs } = useLyricsStore.getState();
  const positionMs = selectCurrentPositionMs(
    {
      snapshot: playerState.snapshot,
      positionMs: playerState.positionMs,
      playingSinceMs: playerState.playingSinceMs,
    },
    nowMs,
  );
  return positionMs - offsetMs;
}

export function readLyricsPlaybackClockMs(
  nowMs = () => performance.now(),
): number {
  const playerState = usePlayerStore.getState();
  return selectCurrentPositionMs(
    {
      snapshot: playerState.snapshot,
      positionMs: playerState.positionMs,
      playingSinceMs: playerState.playingSinceMs,
    },
    nowMs,
  );
}

export function syncLyricsActiveLine(
  prevIndexRef: { current: number },
  adjustedMs: number,
): void {
  const state = usePlayerStore.getState();
  const { lines, setActiveLineIndex } = useLyricsStore.getState();

  if (!state.snapshot?.song_id || lines.length === 0) {
    return;
  }

  const index = findActiveLyricLineIndex(lines, adjustedMs);

  if (index !== prevIndexRef.current) {
    prevIndexRef.current = index;
    setActiveLineIndex(index);
  }
}

export function computeLineChangeLyricsScrollTop(
  container: HTMLElement,
  lines: { time_ms: number }[],
  adjustedMs: number,
): number | null {
  if (lines.length === 0) {
    return null;
  }

  const activeIndex = findActiveLyricLineIndex(lines, adjustedMs);
  if (activeIndex < 0) {
    return getScrollTopForLineIndex(container, 0);
  }

  return getScrollTopForLineIndex(container, activeIndex);
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
}

export function tickLyricsEngineScroll(input: {
  container: HTMLElement;
  lines: { time_ms: number }[];
  adjustedMs: number;
  isSeek?: boolean;
  scrollState: LyricsEngineScrollState;
  userScrollGuard: UserScrollGuard | null;
  reducedMotion: boolean;
  dt: number;
  audienceMode?: boolean;
}): void {
  const {
    container,
    lines,
    adjustedMs,
    isSeek = false,
    scrollState,
    userScrollGuard,
    reducedMotion,
    dt,
    audienceMode = false,
  } = input;
  const {
    scrollSpring,
    targetScrollTopRef,
    prevActiveIndexRef,
    prevAdjustedMsRef,
    lastResumeGenerationRef,
  } = scrollState;

  const seekJump = isLyricsPlaybackSeekJump(
    prevAdjustedMsRef.current,
    adjustedMs,
    dt,
  );
  prevAdjustedMsRef.current = adjustedMs;

  const resumeGeneration = peekLyricsAutoScrollResumeGeneration();
  const explicitResume = resumeGeneration !== lastResumeGenerationRef.current;
  if (explicitResume) {
    lastResumeGenerationRef.current = resumeGeneration;
  }

  const shouldResetScroll = isSeek || seekJump || explicitResume;

  const audienceSeekUnlock = isSeek && audienceMode && userScrollGuard !== null;

  if (shouldResetScroll) {
    if (!audienceSeekUnlock) {
      userScrollGuard?.clear();
    }
    prevActiveIndexRef.current = -1;
    targetScrollTopRef.current = null;
  }

  if (!audienceSeekUnlock && userScrollGuard?.isActive()) {
    bindSpringToViewport(scrollSpring, container);
    targetScrollTopRef.current = container.scrollTop;
    return;
  }

  const activeIndex = findActiveLyricLineIndex(lines, adjustedMs);
  const target = computeLineChangeLyricsScrollTop(container, lines, adjustedMs);

  if (target === null) {
    if (shouldResetScroll) {
      endLyricsAutoScrollUnlockSuppress();
    }
    if (audienceSeekUnlock && userScrollGuard) {
      userScrollGuard.unlockWithIdleRelock();
    }
    return;
  }

  const writeScrollTop = (value: number) => {
    if (userScrollGuard) {
      userScrollGuard.withProgrammatic(() => {
        container.scrollTop = value;
      });
    } else {
      container.scrollTop = value;
    }
  };

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
      endLyricsAutoScrollUnlockSuppress();
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
  prevActiveLineRef: { current: number };
  prevActiveWordIndexRef: { current: number };
  lineRuntime: LyricsLineRuntime;
  reducedMotion: boolean;
  dt: number;
  positionMs: number;

  isSeek: boolean;
  audienceMode?: boolean;
}

export function tickLyricsEngineFrame(input: LyricsEngineFrameInput): void {
  const {
    container,
    isPlainText,
    scrollState,
    userScrollGuard,
    prevActiveLineRef,
    prevActiveWordIndexRef,
    lineRuntime,
    reducedMotion,
    dt,
    positionMs,
    isSeek,
    audienceMode = false,
  } = input;

  const playerState = usePlayerStore.getState();
  const lyricsState = useLyricsStore.getState();
  const adjustedMs = positionMs - lyricsState.offsetMs;

  if (playerState.snapshot?.song_id) {
    syncLyricsActiveLine(prevActiveLineRef, adjustedMs);

    const syncedLyricsState = useLyricsStore.getState();
    const activeLine =
      syncedLyricsState.lines[syncedLyricsState.activeLineIndex];
    if (activeLine?.words && activeLine.words.length > 0) {
      const activeWordIndex = findActiveWordIndex(activeLine.words, adjustedMs);
      if (activeWordIndex !== prevActiveWordIndexRef.current) {
        prevActiveWordIndexRef.current = activeWordIndex;
        syncedLyricsState.setActiveWordIndex(activeWordIndex);
      }
    } else if (prevActiveWordIndexRef.current !== -1) {
      prevActiveWordIndexRef.current = -1;
      syncedLyricsState.setActiveWordIndex(-1);
    }
  }

  lineRuntime.tick({
    activeLineIndex: useLyricsStore.getState().activeLineIndex,
    adjustedMs,
    isPlaying: playerState.snapshot?.is_playing ?? false,
    dt,
    isPlainText,
  });

  if (isPlainText || !container || lyricsState.lines.length === 0) {
    return;
  }

  tickLyricsEngineScroll({
    container,
    lines: lyricsState.lines,
    adjustedMs,
    isSeek,
    scrollState,
    userScrollGuard,
    reducedMotion,
    dt,
    audienceMode,
  });
}

export function shouldRunLyricsEngineLoop(
  playerState: ReturnType<typeof usePlayerStore.getState>,
): boolean {
  return Boolean(playerState.snapshot?.song_id);
}
