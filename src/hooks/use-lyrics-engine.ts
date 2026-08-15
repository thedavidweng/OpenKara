import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";
import {
  FOCUS_ALIGN_POSITION,
  LIST_ALIGN_POSITION,
  getScrollTopForLineIndex,
} from "@/components/Lyrics/lyrics-scroll";
import { findActiveLyricLineIndex } from "@/lib/lyrics-timing";
import {
  beginUserLineSnap,
  beginUserLineStep,
  createUserScrollGuard,
  shouldRunLyricsEngineLoop,
  tickLyricsEngineFrame,
  USER_SCROLL_PAUSE_MS,
  type LyricsEngineScrollState,
  type UserScrollGuard,
} from "@/lib/lyrics-engine";
import {
  lyricsLineRuntime,
  type LyricsLineRuntime,
} from "@/lib/lyrics-line-runtime";
import {
  sampleLyricsTimeFrame,
  setLyricsCurrentTime,
} from "@/lib/lyrics-playback-time";
import type { LyricsSession } from "@/lib/lyrics-session";
import { Spring } from "@/lib/spring";
import { lyricsSession as appLyricsSession } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";

const SCROLL_SPRING = { stiffness: 170, damping: 28, mass: 1 };

const RESIZE_REANCHOR_DEBOUNCE_MS = 120;

export function useLyricsEngine(input: {
  containerRef: RefObject<HTMLDivElement | null>;
  isPlainText: boolean;
  lyricsFontStep: number;
  presentation: "standard" | "audience";
  focusStage?: boolean;
  songId: string | null | undefined;
  viewportActive: boolean;
  layoutVersion?: string;
  lineRuntime?: LyricsLineRuntime;
  session?: LyricsSession;
  onUserScrollActiveChange?: (active: boolean) => void;
}): void {
  const {
    containerRef,
    isPlainText,
    lyricsFontStep,
    presentation,
    focusStage = false,
    songId,
    viewportActive,
    layoutVersion = "",
    lineRuntime = lyricsLineRuntime,
    session = appLyricsSession,
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
    userSnapTopRef: { current: null },
  });
  const focusStageRef = useRef(focusStage);
  focusStageRef.current = focusStage;
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

    const alignPosition = () =>
      focusStageRef.current ? FOCUS_ALIGN_POSITION : LIST_ALIGN_POSITION;
    const prefersReducedMotion = () =>
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    const guard = createUserScrollGuard(container, USER_SCROLL_PAUSE_MS, {
      scrollControl: session.scroll,
      onActiveChange: (active) => {
        onUserScrollActiveChangeRef.current?.(active);
      },
      onUserGesture: () => {
        const snap = scrollStateRef.current.userSnapTopRef;
        if (snap) {
          snap.current = null;
        }
      },
      onCoast: ({ scrollTop, velocityPxPerSec }) => {
        beginUserLineSnap({
          container,
          scrollState: scrollStateRef.current,
          lineCount: session.getState().lines.length,
          alignPosition: alignPosition(),
          position: scrollTop,
          velocityPxPerSec,
          reducedMotion: prefersReducedMotion(),
        });
      },
      onDiscreteStep: (direction) => {
        beginUserLineStep({
          container,
          scrollState: scrollStateRef.current,
          lineCount: session.getState().lines.length,
          alignPosition: alignPosition(),
          position: container.scrollTop,
          direction,
          reducedMotion: prefersReducedMotion(),
        });
      },
    });
    guardRef.current = guard;
    onUserScrollActiveChangeRef.current?.(false);

    return () => {
      guard?.destroy();
      guardRef.current = null;
      onUserScrollActiveChangeRef.current?.(false);
    };
  }, [containerRef, isPlainText, session, songId, viewportActive]);

  useEffect(() => {
    lineRuntime.clear();
  }, [lineRuntime, songId]);

  useEffect(() => {
    if (isPlainText || !songId || !viewportActive) {
      return;
    }

    if (!shouldRunLyricsEngineLoop(usePlayerStore.getState())) {
      return;
    }

    const syncNow = () => {
      const positionMs = session.readPositionMs();
      setLyricsCurrentTime(positionMs);
      if (!shouldRunLyricsEngineLoop(usePlayerStore.getState())) {
        return;
      }
      session.syncActiveLine(session.toAdjustedMs(positionMs));
    };
    window.addEventListener("focus", syncNow);

    const reducedMotion =
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    const previousSongId = engineSongIdRef.current;
    const isSongChange = previousSongId !== songId;
    engineSongIdRef.current = songId;

    const scrollState = scrollStateRef.current;
    const scrollSpring = scrollState.scrollSpring;
    scrollState.prevActiveIndexRef.current = -1;
    scrollState.prevAdjustedMsRef.current = null;
    lastSeekRevisionRef.current = usePlayerStore.getState().seekRevision;

    if (isSongChange) {
      const container = containerRef.current;
      const { lines } = session.getState();
      const adjustedMs = session.toAdjustedMs(session.readPositionMs());
      const activeIndex = findActiveLyricLineIndex(lines, adjustedMs);
      const measuredTarget =
        previousSongId != null && container && activeIndex >= 0
          ? getScrollTopForLineIndex(
              container,
              activeIndex,
              focusStage ? FOCUS_ALIGN_POSITION : undefined,
            )
          : null;
      const nextTop = measuredTarget ?? 0;
      scrollSpring.jumpTo(nextTop);
      scrollState.targetScrollTopRef.current =
        measuredTarget === null ? null : nextTop;
      scrollState.prevActiveIndexRef.current =
        measuredTarget === null ? -1 : activeIndex;
      scrollState.lastResumeGenerationRef.current =
        session.scroll.peekResumeGeneration();

      if (container) {
        const write = () => {
          container.scrollTop = nextTop;
        };
        if (guardRef.current) {
          guardRef.current.withProgrammatic(write);
        } else {
          write();
        }
      }
      if (previousSongId != null) {
        session.scroll.requestResume();
      }
    } else {
      session.scroll.requestResume();
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
        session.scroll.requestResume();
      }
      setLyricsCurrentTime(
        session.readPositionMs(() => now),
        { isSeek },
      );
      const frame = sampleLyricsTimeFrame();

      tickLyricsEngineFrame({
        container: containerRef.current,
        isPlainText,
        scrollState,
        userScrollGuard: guardRef.current,
        session,
        lineRuntime,
        reducedMotion,
        dt,
        positionMs: frame.positionMs,
        isSeek: frame.isSeek,
        hasSong: Boolean(playerState.snapshot?.song_id),
        isPlaying: playerState.snapshot?.is_playing ?? false,
        audienceMode: presentation === "audience",
        focusStage,
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
    focusStage,
    session,
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
      const { lines, activeLineIndex } = session.getState();
      if (lines.length === 0 || activeLineIndex < 0) {
        return;
      }
      const target = getScrollTopForLineIndex(
        currentContainer,
        activeLineIndex,
        focusStage ? FOCUS_ALIGN_POSITION : undefined,
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
  }, [containerRef, focusStage, isPlainText, session, viewportActive, songId]);
}
