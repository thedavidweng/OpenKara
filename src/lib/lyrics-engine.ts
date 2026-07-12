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

// Idle time after the last user scroll before auto-follow resumes (Spotify-style).
const USER_SCROLL_PAUSE_MS = 4000;

// Playback jumps larger than this (and larger than natural rAF advance) are
// treated as seeks — matching AMLL's setCurrentTime(..., isSeek) / resetScroll.
const SEEK_JUMP_MS = 400;

export { USER_SCROLL_PAUSE_MS, SEEK_JUMP_MS };

/**
 * AMLL-style resetScroll / Follow trigger. Line clicks and the Follow button
 * call this so auto-scroll re-locks to the playing line.
 */
let autoScrollResumeGeneration = 0;
/** While true, user-scroll signals are ignored (covers click seek gestures). */
let autoScrollUnlockSuppressed = false;

export function requestLyricsAutoScrollResume(): void {
  autoScrollResumeGeneration += 1;
  // Stay suppressed until the engine consumes the resume and writes scrollTop
  // through withProgrammatic — otherwise click scroll-into-view unlocks follow.
  autoScrollUnlockSuppressed = true;
}

export function peekLyricsAutoScrollResumeGeneration(): number {
  return autoScrollResumeGeneration;
}

export function endLyricsAutoScrollUnlockSuppress(): void {
  autoScrollUnlockSuppressed = false;
}

/** Test-only: reset module-level resume / suppress latches between cases. */
export function resetLyricsEngineScrollControlForTests(): void {
  autoScrollResumeGeneration = 0;
  autoScrollUnlockSuppressed = false;
}

export interface UserScrollGuard {
  /** True while the user has unlocked auto-follow (browsing lyrics freely). */
  isActive: () => boolean;
  /** Re-lock auto-follow immediately (Follow button / seek resetScroll). */
  clear: () => void;
  /**
   * Run a programmatic scrollTop write without treating it as user unlock.
   * Real browsers fire the resulting scroll event asynchronously, so the guard
   * also records the written scrollTop and ignores scroll events that land on
   * that exact position (see lastProgrammaticScrollTop).
   */
  withProgrammatic: (fn: () => void) => void;
  destroy: () => void;
}

/**
 * Spotify / Apple Music lyrics follow controller.
 *
 * Unlock from explicit wheel/touch movement or a pointer-owned native
 * scrollbar change — not touchstart/click/bare layout scroll events, which
 * can fire around line-click seek without user browsing intent.
 */
export function createUserScrollGuard(
  container: HTMLElement,
  pauseMs: number,
  options: {
    timers?: {
      setTimeout: typeof globalThis.setTimeout;
      clearTimeout: typeof globalThis.clearTimeout;
    };
    onActiveChange?: (active: boolean) => void;
    /**
     * Fired after idle re-lock. Defaults to {@link requestLyricsAutoScrollResume}
     * so the engine re-anchors scrollTop to the playing line — clearing
     * `unlocked` alone only hides the Follow button while the spring stays
     * parked at the user's browse offset until the next line change.
     */
    onIdleRelock?: () => void;
  } = {},
): UserScrollGuard {
  const timers = options.timers ?? { setTimeout, clearTimeout };
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
      // Was unlocked: re-lock UI and force a resume snap to the playing line.
      // Without the resume generation, the engine sees activeIndex unchanged
      // and leaves scrollTop at the user's browse position while the Follow
      // button already claims follow has resumed.
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
    // Ignore no-op / sub-pixel noise after programmatic writes.
    if (Math.abs(container.scrollTop - lastProgrammaticScrollTop) < 1) {
      return;
    }
    // RATIONALE: WKWebView can emit scroll events for scroll anchoring/layout
    // corrections after the active lyric changes. A bare scroll event does
    // not prove user intent and used to re-unlock follow immediately after a
    // seek. Wheel/touchmove are explicit; scroll is accepted only while the
    // user is holding a pointer (native scrollbar drag).
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
  const positionMs =
    playerState.airPlayOutput.active &&
    playerState.airPlayOutput.displayedPositionMs !== null
      ? playerState.airPlayOutput.displayedPositionMs
      : selectCurrentPositionMs(
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
  if (
    playerState.airPlayOutput.active &&
    playerState.airPlayOutput.displayedPositionMs !== null
  ) {
    return playerState.airPlayOutput.displayedPositionMs;
  }

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

/**
 * Detect a discontinuous playback jump (click-to-seek / scrub).
 * Mature lyric players (AMLL) take an explicit isSeek flag; we infer the same
 * from the clock so scroll can resetScroll without coupling LyricLine → engine.
 */
export function isLyricsPlaybackSeekJump(
  previousAdjustedMs: number | null,
  adjustedMs: number,
  dtSeconds: number,
): boolean {
  if (previousAdjustedMs === null) {
    return false;
  }
  const delta = Math.abs(adjustedMs - previousAdjustedMs);
  // Allow 2× realtime plus a small slack for timer jitter / IPC catch-up.
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

/**
 * Auto-scroll via native scrollTop (AMLL / umbra / pre-#56 OpenKara pattern).
 *
 * RATIONALE: Driving scroll with translateY on an overflow-y-auto container
 * fights click scroll-into-view and overflow anchoring. Mature lyric players
 * mutate scrollTop (or scrollIntoView) and call resetScroll on seek instead.
 */
export function tickLyricsEngineScroll(input: {
  container: HTMLElement;
  lines: { time_ms: number }[];
  adjustedMs: number;
  /** AMLL isSeek — explicit discontinuous jump from the host time feed. */
  isSeek?: boolean;
  scrollState: LyricsEngineScrollState;
  userScrollGuard: UserScrollGuard | null;
  reducedMotion: boolean;
  dt: number;
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
  } = input;
  const {
    scrollSpring,
    targetScrollTopRef,
    prevActiveIndexRef,
    prevAdjustedMsRef,
    lastResumeGenerationRef,
  } = scrollState;

  // Infer backend-driven position snaps that did not pass through the player
  // store. Normal UI seeks arrive through the host-owned explicit isSeek edge.
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

  if (shouldResetScroll) {
    // AMLL resetScroll on line-click seek: drop the user-scroll pause and
    // force the next target write so auto-scroll resumes immediately.
    userScrollGuard?.clear();
    prevActiveIndexRef.current = -1;
    targetScrollTopRef.current = null;
  }

  if (userScrollGuard?.isActive()) {
    // User is browsing freely (Spotify unlock). Track their viewport so the
    // next re-lock animates from where they left off — never write scrollTop.
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
    // Seek / explicit resume snaps (AMLL isSeek path); reduced-motion never springs.
    // Still only adopt a fresh layout measure on line change / reset — see
    // mid-line reflow rationale below.
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
    return;
  }

  // RATIONALE: Anchor scroll once per active line. Mid-line DOM reflow
  // (per-character emphasis swapping to inline-block, font-weight flips) makes
  // getScrollTopForLineIndex jitter by tens of pixels while the line index is
  // unchanged. Retargeting on every pixel delta kept the spring chasing a
  // moving target — lyrics looked "wrong" until pause froze the layout and
  // the next measure snapped back. Font/romanization changes remount the
  // engine via layoutVersion, so they still get a fresh anchor.
  if (activeIndex !== prevActiveIndexRef.current) {
    bindSpringToViewport(scrollSpring, container);
    prevActiveIndexRef.current = activeIndex;
    targetScrollTopRef.current = target;
    scrollSpring.setTarget(target);
  }

  if (!scrollSpring.isSettled()) {
    scrollSpring.update(dt);
  }

  // Re-assert scrollTop only while following. withProgrammatic prevents our
  // own writes from unlocking follow via the scroll listener.
  writeScrollTop(scrollSpring.getPosition());
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
  /**
   * Host playback clock sample (AMLL setCurrentTime). The engine does not
   * invent a second clock — it only applies lyrics offset and drives sync.
   */
  positionMs: number;
  /** AMLL isSeek for this frame. */
  isSeek: boolean;
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
  } = input;

  const playerState = usePlayerStore.getState();
  const lyricsState = useLyricsStore.getState();
  const adjustedMs = positionMs - lyricsState.offsetMs;

  if (playerState.snapshot?.song_id) {
    syncLyricsActiveLine(prevActiveLineRef, adjustedMs);

    // Re-read the store after syncLyricsActiveLine: setActiveLineIndex resets
    // activeWordIndex to -1 on a line change, and the stale `lyricsState`
    // captured above would still point at the previous line's words, causing
    // findActiveWordIndex to compute against the old line and overwrite the
    // just-reset -1 with a stale value for one rAF frame.
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
  });
}

export function shouldRunLyricsEngineLoop(
  playerState: ReturnType<typeof usePlayerStore.getState>,
): boolean {
  return Boolean(playerState.snapshot?.song_id);
}
