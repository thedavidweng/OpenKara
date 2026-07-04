import { useEffect, useRef, type RefObject } from "react";
import { Spring } from "@/lib/spring";
import {
  createUserScrollGuard,
  readLyricsPlaybackClockMs,
  syncLyricsActiveLine,
  shouldRunLyricsEngineLoop,
  tickLyricsEngineFrame,
  USER_SCROLL_PAUSE_MS,
  type LyricsEngineScrollState,
} from "@/lib/lyrics-engine";
import { usePlayerStore } from "@/stores/player-store";

const SCROLL_SPRING = { stiffness: 170, damping: 28, mass: 1 };

export {
  computeLineChangeLyricsScrollTop,
  createUserScrollGuard,
  readLyricsAdjustedPlaybackMs,
  syncLyricsActiveLine,
  USER_SCROLL_PAUSE_MS,
} from "@/lib/lyrics-engine";

function readStableDisplayPositionMs(): number {
  return readLyricsPlaybackClockMs();
}

/**
 * Unified lyrics runtime: one requestAnimationFrame loop drives playback clock,
 * active-line sync, and auto-scroll so the three paths cannot drift apart.
 */
export function useLyricsEngine(input: {
  containerRef: RefObject<HTMLDivElement | null>;
  scrollContentRef: RefObject<HTMLDivElement | null>;
  isPlainText: boolean;
  lyricsFontStep: number;
  presentation: "standard" | "audience";
  songId: string | null | undefined;
  layoutVersion?: string;
  onPlaybackClockMs: (ms: number) => void;
}): void {
  const {
    containerRef,
    scrollContentRef,
    isPlainText,
    lyricsFontStep,
    presentation,
    songId,
    layoutVersion = "",
    onPlaybackClockMs,
  } = input;

  const guardRef = useRef<ReturnType<typeof createUserScrollGuard> | null>(
    null,
  );
  const scrollSpringRef = useRef(new Spring(0, SCROLL_SPRING));
  const scrollStateRef = useRef<LyricsEngineScrollState>({
    scrollSpring: scrollSpringRef.current,
    targetScrollTopRef: { current: null },
    prevActiveIndexRef: { current: -1 },
  });
  const prevActiveLineRef = useRef(-1);
  const onPlaybackClockMsRef = useRef(onPlaybackClockMs);
  onPlaybackClockMsRef.current = onPlaybackClockMs;

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
    if (isPlainText || !songId) {
      onPlaybackClockMsRef.current(readStableDisplayPositionMs());
      return;
    }

    onPlaybackClockMsRef.current(readStableDisplayPositionMs());

    const playerState = usePlayerStore.getState();
    if (!shouldRunLyricsEngineLoop(playerState)) {
      return;
    }

    const syncNow = () => {
      onPlaybackClockMsRef.current(readStableDisplayPositionMs());
      syncLyricsActiveLine(prevActiveLineRef);
    };
    window.addEventListener("focus", syncNow);

    const reducedMotion =
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    const scrollState = scrollStateRef.current;
    const scrollSpring = scrollState.scrollSpring;
    scrollSpring.jumpTo(0);
    scrollState.targetScrollTopRef.current = null;
    scrollState.prevActiveIndexRef.current = -1;
    prevActiveLineRef.current = -1;

    const container = containerRef.current;
    const scrollContent = scrollContentRef.current;
    if (container) {
      container.scrollTop = 0;
    }
    if (scrollContent) {
      scrollContent.style.transform = "translateY(0px)";
    }

    let rafId = 0;
    let lastTime = performance.now();

    const tick = (now: number) => {
      const dt = Math.min((now - lastTime) / 1000, 0.05);
      lastTime = now;

      tickLyricsEngineFrame({
        container: containerRef.current,
        scrollContent: scrollContentRef.current,
        isPlainText,
        scrollState,
        userScrollGuard: guardRef.current,
        prevActiveLineRef,
        reducedMotion,
        dt,
        nowMs: now,
        onPlaybackClockMs: (ms) => onPlaybackClockMsRef.current(ms),
      });

      rafId = requestAnimationFrame(tick);
    };

    rafId = requestAnimationFrame(tick);
    return () => {
      window.removeEventListener("focus", syncNow);
      cancelAnimationFrame(rafId);
    };
  }, [
    containerRef,
    scrollContentRef,
    isPlainText,
    lyricsFontStep,
    presentation,
    songId,
    layoutVersion,
  ]);

  const isPlaying = usePlayerStore((s) => s.snapshot?.is_playing ?? false);
  const playingSinceMs = usePlayerStore((s) => s.playingSinceMs);
  const airPlayDisplayedPositionMs = usePlayerStore(
    (s) => s.airPlayOutput.displayedPositionMs,
  );
  const airPlayActive = usePlayerStore((s) => s.airPlayOutput.active);

  useEffect(() => {
    onPlaybackClockMsRef.current(readStableDisplayPositionMs());
  }, [
    isPlaying,
    playingSinceMs,
    airPlayActive,
    airPlayDisplayedPositionMs,
    songId,
  ]);
}
