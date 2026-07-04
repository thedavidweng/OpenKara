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

// Duration (ms) to suppress auto-scroll after the user manually scrolls.
const USER_SCROLL_PAUSE_MS = 3000;

export { USER_SCROLL_PAUSE_MS };

/**
 * Attaches wheel and touchstart listeners to a container element and tracks
 * whether the user has recently scrolled manually.
 */
export function createUserScrollGuard(
  container: HTMLElement,
  pauseMs: number,
  timers: {
    setTimeout: typeof globalThis.setTimeout;
    clearTimeout: typeof globalThis.clearTimeout;
  } = { setTimeout, clearTimeout },
): { isActive: () => boolean; destroy: () => void } {
  let scrolling = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const onUserScroll = () => {
    scrolling = true;
    if (timer !== null) timers.clearTimeout(timer);
    timer = timers.setTimeout(() => {
      scrolling = false;
    }, pauseMs);
  };

  container.addEventListener("wheel", onUserScroll, { passive: true });
  container.addEventListener("touchstart", onUserScroll, { passive: true });

  return {
    isActive: () => scrolling,
    destroy: () => {
      container.removeEventListener("wheel", onUserScroll);
      container.removeEventListener("touchstart", onUserScroll);
      if (timer !== null) timers.clearTimeout(timer);
      scrolling = false;
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

export function syncLyricsActiveLine(prevIndexRef: { current: number }): void {
  const state = usePlayerStore.getState();
  const { snapshot } = state;
  const { lines, offsetMs, setActiveLineIndex } = useLyricsStore.getState();

  if (!snapshot?.song_id || lines.length === 0) {
    return;
  }

  const positionMs = selectCurrentPositionMs({
    snapshot: state.snapshot,
    positionMs: state.positionMs,
    playingSinceMs: state.playingSinceMs,
  });
  const adjustedMs = positionMs - offsetMs;
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

function applyAutoScrollTransform(
  scrollContent: HTMLElement,
  offset: number,
): void {
  scrollContent.style.transform = `translateY(${-offset}px)`;
}

function bakeUserScrollIntoTransform(
  container: HTMLElement,
  scrollSpring: Spring,
): number {
  const bakedOffset = scrollSpring.getPosition() + container.scrollTop;
  container.scrollTop = 0;
  scrollSpring.jumpTo(bakedOffset);
  return bakedOffset;
}

export interface LyricsEngineScrollState {
  scrollSpring: Spring;
  targetScrollTopRef: { current: number | null };
  prevActiveIndexRef: { current: number };
}

export function tickLyricsEngineScroll(input: {
  container: HTMLElement;
  scrollContent: HTMLElement;
  lines: { time_ms: number }[];
  adjustedMs: number;
  scrollState: LyricsEngineScrollState;
  userScrollGuard: { isActive: () => boolean } | null;
  reducedMotion: boolean;
  dt: number;
}): void {
  const {
    container,
    scrollContent,
    lines,
    adjustedMs,
    scrollState,
    userScrollGuard,
    reducedMotion,
    dt,
  } = input;
  const { scrollSpring, targetScrollTopRef, prevActiveIndexRef } = scrollState;

  if (userScrollGuard?.isActive()) {
    bakeUserScrollIntoTransform(container, scrollSpring);
    targetScrollTopRef.current = scrollSpring.getPosition();
    applyAutoScrollTransform(scrollContent, scrollSpring.getPosition());
    return;
  }

  const activeIndex = findActiveLyricLineIndex(lines, adjustedMs);
  const target = computeLineChangeLyricsScrollTop(container, lines, adjustedMs);

  if (target === null) {
    return;
  }

  if (reducedMotion) {
    scrollSpring.jumpTo(target);
    applyAutoScrollTransform(scrollContent, target);
    targetScrollTopRef.current = target;
    prevActiveIndexRef.current = activeIndex;
    return;
  }

  if (
    activeIndex !== prevActiveIndexRef.current ||
    target !== targetScrollTopRef.current
  ) {
    const currentOffset = bakeUserScrollIntoTransform(container, scrollSpring);
    if (currentOffset !== scrollSpring.getPosition()) {
      scrollSpring.jumpTo(currentOffset);
    }
    prevActiveIndexRef.current = activeIndex;
    targetScrollTopRef.current = target;
    scrollSpring.setTarget(target);
  }

  if (!scrollSpring.isSettled()) {
    scrollSpring.update(dt);
  }

  applyAutoScrollTransform(scrollContent, scrollSpring.getPosition());
}

export interface LyricsEngineFrameInput {
  container: HTMLElement | null;
  scrollContent: HTMLElement | null;
  isPlainText: boolean;
  scrollState: LyricsEngineScrollState;
  userScrollGuard: { isActive: () => boolean } | null;
  prevActiveLineRef: { current: number };
  prevActiveWordIndexRef: { current: number };
  lineRuntime: LyricsLineRuntime;
  reducedMotion: boolean;
  dt: number;
  nowMs: number;
}

export function tickLyricsEngineFrame(input: LyricsEngineFrameInput): void {
  const {
    container,
    scrollContent,
    isPlainText,
    scrollState,
    userScrollGuard,
    prevActiveLineRef,
    prevActiveWordIndexRef,
    lineRuntime,
    reducedMotion,
    dt,
    nowMs,
  } = input;

  const playerState = usePlayerStore.getState();
  const lyricsState = useLyricsStore.getState();
  const playbackClockMs =
    playerState.airPlayOutput.active &&
    playerState.airPlayOutput.displayedPositionMs !== null
      ? playerState.airPlayOutput.displayedPositionMs
      : selectCurrentPositionMs(
          {
            snapshot: playerState.snapshot,
            positionMs: playerState.positionMs,
            playingSinceMs: playerState.playingSinceMs,
          },
          () => nowMs,
        );
  const adjustedMs = playbackClockMs - lyricsState.offsetMs;

  if (playerState.snapshot?.song_id) {
    syncLyricsActiveLine(prevActiveLineRef);

    const activeLine = lyricsState.lines[lyricsState.activeLineIndex];
    if (activeLine?.words && activeLine.words.length > 0) {
      const activeWordIndex = findActiveWordIndex(activeLine.words, adjustedMs);
      if (activeWordIndex !== prevActiveWordIndexRef.current) {
        prevActiveWordIndexRef.current = activeWordIndex;
        lyricsState.setActiveWordIndex(activeWordIndex);
      }
    } else if (prevActiveWordIndexRef.current !== -1) {
      prevActiveWordIndexRef.current = -1;
      lyricsState.setActiveWordIndex(-1);
    }
  }

  lineRuntime.tick({
    activeLineIndex: lyricsState.activeLineIndex,
    adjustedMs,
    isPlaying: playerState.snapshot?.is_playing ?? false,
    dt,
    isPlainText,
  });

  if (
    isPlainText ||
    !container ||
    !scrollContent ||
    lyricsState.lines.length === 0
  ) {
    return;
  }

  tickLyricsEngineScroll({
    container,
    scrollContent,
    lines: lyricsState.lines,
    adjustedMs,
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
