import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";
import { getScrollTopForLineIndex } from "@/components/Lyrics/lyrics-scroll";
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

// Coalesce continuous resize (drag-resizing the window, sidebar animation)
// into a single re-anchor snap once the viewport settles (#202).
const RESIZE_REANCHOR_DEBOUNCE_MS = 120;

export function useLyricsEngine(input: {
  containerRef: RefObject<HTMLDivElement | null>;
  isPlainText: boolean;
  lyricsFontStep: number;
  presentation: "standard" | "audience";
  songId: string | null | undefined;
  viewportActive: boolean;
  layoutVersion?: string;
  lineRuntime?: LyricsLineRuntime;
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
  const engineSongIdRef = useRef<string | null | undefined>(undefined);

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
      const positionMs = readLyricsPlaybackClockMs();
      setLyricsCurrentTime(positionMs);
      const adjustedMs = positionMs - useLyricsStore.getState().offsetMs;
      syncLyricsActiveLine(prevActiveLineRef, adjustedMs);
    };
    window.addEventListener("focus", syncNow);

    const reducedMotion =
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    const isSongChange = engineSongIdRef.current !== songId;
    engineSongIdRef.current = songId;

    const scrollState = scrollStateRef.current;
    const scrollSpring = scrollState.scrollSpring;
    scrollState.prevActiveIndexRef.current = -1;
    scrollState.prevAdjustedMsRef.current = null;
    lastSeekRevisionRef.current = usePlayerStore.getState().seekRevision;

    if (isSongChange) {
      scrollSpring.jumpTo(0);
      scrollState.targetScrollTopRef.current = null;
      scrollState.lastResumeGenerationRef.current =
        peekLyricsAutoScrollResumeGeneration();

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
    } else {
      requestLyricsAutoScrollResume();
    }

    let rafId = 0;
    let lastTime = performance.now();

    const tick = (now: number) => {
      const dt = Math.min((now - lastTime) / 1000, 0.05);
      lastTime = now;

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

  useLayoutEffect(() => {
    if (isPlainText || !viewportActive) {
      return;
    }
    const container = containerRef.current;
    if (!container || typeof ResizeObserver === "undefined") {
      return;
    }

    let lastWidth = container.clientWidth;
    let lastHeight = container.clientHeight;
    let debounceId: ReturnType<typeof setTimeout> | null = null;

    const reanchorToActiveLine = () => {
      debounceId = null;
      const currentContainer = containerRef.current;
      if (!currentContainer) {
        return;
      }
      if (guardRef.current?.isActive()) {
        return;
      }
      const { lines, activeLineIndex } = useLyricsStore.getState();
      if (lines.length === 0 || activeLineIndex < 0) {
        return;
      }
      const target = getScrollTopForLineIndex(
        currentContainer,
        activeLineIndex,
      );
      if (target === null) {
        return;
      }
      const scrollState = scrollStateRef.current;
      scrollState.scrollSpring.jumpTo(target);
      scrollState.targetScrollTopRef.current = target;
      const write = () => {
        currentContainer.scrollTop = target;
      };
      if (guardRef.current) {
        guardRef.current.withProgrammatic(write);
      } else {
        write();
      }
    };

    const observer = new ResizeObserver(() => {
      const currentContainer = containerRef.current;
      if (!currentContainer) {
        return;
      }
      const width = currentContainer.clientWidth;
      const height = currentContainer.clientHeight;
      if (width === lastWidth && height === lastHeight) {
        return;
      }
      lastWidth = width;
      lastHeight = height;
      if (debounceId !== null) {
        clearTimeout(debounceId);
      }
      debounceId = setTimeout(
        reanchorToActiveLine,
        RESIZE_REANCHOR_DEBOUNCE_MS,
      );
    });
    observer.observe(container);

    return () => {
      if (debounceId !== null) {
        clearTimeout(debounceId);
      }
      observer.disconnect();
    };
  }, [containerRef, isPlainText, viewportActive, songId]);
}
