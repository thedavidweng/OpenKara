import { useEffect, useRef, type RefObject } from "react";
import { Spring } from "@/lib/spring";
import {
  createUserScrollGuard,
  syncLyricsActiveLine,
  shouldRunLyricsEngineLoop,
  tickLyricsEngineFrame,
  USER_SCROLL_PAUSE_MS,
  type LyricsEngineScrollState,
} from "@/lib/lyrics-engine";
import {
  lyricsLineRuntime,
  type LyricsLineRuntime,
} from "@/lib/lyrics-line-runtime";
import { usePlayerStore } from "@/stores/player-store";

const SCROLL_SPRING = { stiffness: 170, damping: 28, mass: 1 };

/**
 * Unified lyrics runtime: one requestAnimationFrame loop drives active-line
 * sync, karaoke fill, line springs, and auto-scroll.
 */
export function useLyricsEngine(input: {
  containerRef: RefObject<HTMLDivElement | null>;
  scrollContentRef: RefObject<HTMLDivElement | null>;
  isPlainText: boolean;
  lyricsFontStep: number;
  presentation: "standard" | "audience";
  songId: string | null | undefined;
  layoutVersion?: string;
  lineRuntime?: LyricsLineRuntime;
}): void {
  const {
    containerRef,
    scrollContentRef,
    isPlainText,
    lyricsFontStep,
    presentation,
    songId,
    layoutVersion = "",
    lineRuntime = lyricsLineRuntime,
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
  const prevActiveWordIndexRef = useRef(-1);

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
    lineRuntime.clear();
    prevActiveLineRef.current = -1;
    prevActiveWordIndexRef.current = -1;
  }, [lineRuntime, songId]);

  useEffect(() => {
    if (isPlainText || !songId) {
      return;
    }

    const playerState = usePlayerStore.getState();
    if (!shouldRunLyricsEngineLoop(playerState)) {
      return;
    }

    const syncNow = () => {
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
        prevActiveWordIndexRef,
        lineRuntime,
        reducedMotion,
        dt,
        nowMs: now,
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
    lineRuntime,
  ]);
}
