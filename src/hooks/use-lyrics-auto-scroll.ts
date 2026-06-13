import { useEffect, useRef, type RefObject } from "react";
import {
  getScrollTopForLineIndex,
  interpolateScrollTop,
} from "@/components/Lyrics/lyrics-scroll";
import {
  findActiveLyricLineIndex,
  getLinePlaybackProgress,
} from "@/lib/lyrics-timing";
import { readLyricsAdjustedPlaybackMs } from "@/lib/lyrics-playback-clock";
import { Spring } from "@/lib/spring";
import { useLyricsStore } from "@/stores/lyrics-store";

// Duration (ms) to suppress auto-scroll after the user manually scrolls.
// Long enough to let users read ahead without being yanked back immediately.
const USER_SCROLL_PAUSE_MS = 3000;

const SCROLL_SPRING = { stiffness: 72, damping: 20, mass: 1.1 };

/**
 * Attaches wheel and touchstart listeners to a container element and tracks
 * whether the user has recently scrolled manually. Returns an object with an
 * `isActive()` predicate (true while the pause window is open) and a
 * `destroy()` cleanup that removes listeners and clears any pending timer.
 *
 * Exported so the suppression logic can be exercised in isolation without
 * needing a React renderer.
 *
 * Wheel and touchstart fire only on genuine user interaction — programmatic
 * scrollTop updates do not trigger them — so no extra flag is needed to tell
 * auto-scrolls apart from manual ones.
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

export function computeContinuousLyricsScrollTop(
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

  const currentTop = getScrollTopForLineIndex(container, activeIndex);
  if (currentTop === null) {
    return null;
  }

  const nextIndex = activeIndex + 1;
  if (nextIndex >= lines.length) {
    return currentTop;
  }

  const nextTop = getScrollTopForLineIndex(container, nextIndex);
  if (nextTop === null) {
    return currentTop;
  }

  const progress = getLinePlaybackProgress(lines, activeIndex, adjustedMs);
  return interpolateScrollTop(currentTop, nextTop, progress);
}

function readAdjustedPlaybackMs(): number {
  return readLyricsAdjustedPlaybackMs();
}

/**
 * Continuously scrolls the lyrics viewport so playback progress glides between
 * lines instead of snapping when the active index changes.
 */
export function useLyricsAutoScroll(
  containerRef: RefObject<HTMLDivElement | null>,
  isPlainText: boolean,
  lyricsFontStep: number,
  presentation: "standard" | "audience",
  songId: string | null | undefined,
  layoutVersion = "",
): void {
  const guardRef = useRef<ReturnType<typeof createUserScrollGuard> | null>(
    null,
  );
  const scrollSpringRef = useRef(new Spring(0, SCROLL_SPRING));

  useEffect(() => {
    if (isPlainText) return;
    const container = containerRef.current;
    if (!container) return;

    const guard = createUserScrollGuard(container, USER_SCROLL_PAUSE_MS);
    guardRef.current = guard;

    return () => {
      guard.destroy();
      guardRef.current = null;
    };
  }, [containerRef, isPlainText, songId]);

  useEffect(() => {
    if (isPlainText) return;

    const reducedMotion =
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const scrollSpring = scrollSpringRef.current;
    scrollSpring.jumpTo(containerRef.current?.scrollTop ?? 0);

    let rafId = 0;
    let lastTime = performance.now();

    const tick = (now: number) => {
      const container = containerRef.current;
      const lines = useLyricsStore.getState().lines;

      if (container && lines.length > 0 && !guardRef.current?.isActive()) {
        const target = computeContinuousLyricsScrollTop(
          container,
          lines,
          readAdjustedPlaybackMs(),
        );

        if (target !== null) {
          if (reducedMotion) {
            scrollSpring.jumpTo(target);
            container.scrollTop = target;
          } else {
            scrollSpring.setTarget(target);
            const dt = Math.min((now - lastTime) / 1000, 0.05);
            scrollSpring.update(dt);
            container.scrollTop = scrollSpring.getPosition();
          }
        }
      }

      lastTime = now;
      rafId = requestAnimationFrame(tick);
    };

    rafId = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafId);
  }, [
    containerRef,
    isPlainText,
    lyricsFontStep,
    presentation,
    songId,
    layoutVersion,
  ]);
}
