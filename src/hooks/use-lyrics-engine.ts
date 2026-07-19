import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";
import { Spring } from "@/lib/spring";
import {
  createUserScrollGuard,
  peekLyricsAutoScrollResumeGeneration,
  readLyricsPlaybackClockMs,
  requestLyricsAutoScrollResume,
  syncLyricsActiveLine,
  shouldRunLyricsEngineLoop,
  tickLyricsEngineFrame,
  USER_SCROLL_PAUSE_MS,
  type LyricsEngineScrollState,
  type UserScrollGuard,
} from "@/lib/lyrics-engine";
import {
  setLyricsCurrentTime,
  sampleLyricsTimeFrame,
} from "@/lib/lyrics-playback-time";
import {
  lyricsLineRuntime,
  type LyricsLineRuntime,
} from "@/lib/lyrics-line-runtime";
import { useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";

const SCROLL_SPRING = { stiffness: 170, damping: 28, mass: 1 };

/**
 * Unified lyrics runtime (AMLL LyricPlayer shape):
 * each rAF the host pushes setCurrentTime from the playback clock, then the
 * engine updates line/word sync, karaoke fill, springs, and auto-scroll.
 */
export function useLyricsEngine(input: {
  containerRef: RefObject<HTMLDivElement | null>;
  isPlainText: boolean;
  lyricsFontStep: number;
  presentation: "standard" | "audience";
  songId: string | null | undefined;
  /** True when the scroll viewport is in the DOM (not loading/empty). */
  viewportActive: boolean;
  layoutVersion?: string;
  lineRuntime?: LyricsLineRuntime;
  /** Fires when the user unlocks/re-locks auto-follow (for the Follow button). */
  onUserScrollActiveChange?: (active: boolean) => void;
}): void {
  const {
    containerRef,
    isPlainText,
    lyricsFontStep,
    presentation,
    songId,
    viewportActive,
    layoutVersion = "",
    lineRuntime = lyricsLineRuntime,
    onUserScrollActiveChange,
  } = input;

  const guardRef = useRef<UserScrollGuard | null>(null);
  const onUserScrollActiveChangeRef = useRef(onUserScrollActiveChange);
  onUserScrollActiveChangeRef.current = onUserScrollActiveChange;

  const scrollSpringRef = useRef(new Spring(0, SCROLL_SPRING));
  const scrollStateRef = useRef<LyricsEngineScrollState>({
    scrollSpring: scrollSpringRef.current,
    targetScrollTopRef: { current: null },
    prevActiveIndexRef: { current: -1 },
    prevAdjustedMsRef: { current: null },
    lastResumeGenerationRef: { current: 0 },
  });
  const prevActiveLineRef = useRef(-1);
  const prevActiveWordIndexRef = useRef(-1);
  const lastSeekRevisionRef = useRef(usePlayerStore.getState().seekRevision);

  // RATIONALE: LyricsPanel early-returns a loading/empty state before the scroll
  // viewport mounts. An effect keyed only on songId would run while
  // containerRef.current is null, skip guard setup, then never retry — leaving
  // auto-follow writing scrollTop every frame with no way for the user to unlock.
  useLayoutEffect(() => {
    if (isPlainText || !viewportActive) {
      return;
    }
    const container = containerRef.current;
    if (!container) {
      return;
    }

    const guard = createUserScrollGuard(container, USER_SCROLL_PAUSE_MS, {
      onActiveChange: (active) => {
        onUserScrollActiveChangeRef.current?.(active);
      },
    });
    guardRef.current = guard;
    onUserScrollActiveChangeRef.current?.(false);

    return () => {
      // Null-safe: createUserScrollGuard always returns a guard in production,
      // but tests may mock it to null to exercise the no-guard engine path.
      guard?.destroy();
      guardRef.current = null;
      onUserScrollActiveChangeRef.current?.(false);
    };
  }, [containerRef, isPlainText, songId, viewportActive]);

  useEffect(() => {
    lineRuntime.clear();
    prevActiveLineRef.current = -1;
    prevActiveWordIndexRef.current = -1;
  }, [lineRuntime, songId]);

  useEffect(() => {
    if (isPlainText || !songId || !viewportActive) {
      return;
    }

    const playerState = usePlayerStore.getState();
    if (!shouldRunLyricsEngineLoop(playerState)) {
      return;
    }

    const syncNow = () => {
      // Focus resync must not sample/consume the isSeek latch — only the rAF
      // tick should take the frame (AMLL host still owns discontinuous seeks).
      const positionMs = readLyricsPlaybackClockMs();
      setLyricsCurrentTime(positionMs);
      const adjustedMs = positionMs - useLyricsStore.getState().offsetMs;
      syncLyricsActiveLine(prevActiveLineRef, adjustedMs);
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
    scrollState.prevAdjustedMsRef.current = null;
    scrollState.lastResumeGenerationRef.current =
      peekLyricsAutoScrollResumeGeneration();
    lastSeekRevisionRef.current = usePlayerStore.getState().seekRevision;

    const container = containerRef.current;
    if (container) {
      const write = () => {
        container.scrollTop = 0;
      };
      if (guardRef.current) {
        guardRef.current.withProgrammatic(write);
      } else {
        write();
      }
    }

    let rafId = 0;
    let lastTime = performance.now();

    const tick = (now: number) => {
      const dt = Math.min((now - lastTime) / 1000, 0.05);
      lastTime = now;

      // Host clock → AMLL setCurrentTime → engine sample. The player store
      // publishes seekRevision only after Tauri's authoritative target
      // snapshot is applied, so resetScroll cannot be consumed against the
      // pre-seek playhead while the async command is still in flight.
      const playerState = usePlayerStore.getState();
      const isSeek = playerState.seekRevision !== lastSeekRevisionRef.current;
      if (isSeek) {
        lastSeekRevisionRef.current = playerState.seekRevision;
        requestLyricsAutoScrollResume();
      }
      setLyricsCurrentTime(
        readLyricsPlaybackClockMs(() => now),
        { isSeek },
      );
      const frame = sampleLyricsTimeFrame();

      tickLyricsEngineFrame({
        container: containerRef.current,
        isPlainText,
        scrollState,
        userScrollGuard: guardRef.current,
        prevActiveLineRef,
        prevActiveWordIndexRef,
        lineRuntime,
        reducedMotion,
        dt,
        positionMs: frame.positionMs,
        isSeek: frame.isSeek,
        audienceMode: presentation === "audience",
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
    isPlainText,
    lyricsFontStep,
    presentation,
    songId,
    viewportActive,
    layoutVersion,
    lineRuntime,
  ]);
}
