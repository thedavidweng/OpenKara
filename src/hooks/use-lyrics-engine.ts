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
  // Distinguishes a song change (reset to top + replay gather is correct) from
  // a live layout/font/romanize change (must re-anchor in place, never reset to
  // 0). Undefined sentinel forces the first real run to be treated as a song
  // change. See the engine effect below (#201).
  const engineSongIdRef = useRef<string | null | undefined>(undefined);

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

    // RATIONALE (#201): This effect re-runs on lyricsFontStep / layoutVersion /
    // presentation changes as well as songId. Resetting scrollTop to 0 and
    // replaying the entrance gather is only correct for a *song change*. A live
    // font-size / Romanize / layout change must keep the active line put and
    // re-anchor in place. So gate the reset-to-top on a genuine song change and
    // otherwise force the next frame to snap to the recomputed centered target
    // for the current active line (shouldResetScroll-style, no animate-from-0).
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
      // Same song, layout/font/romanize changed: leave the spring and scrollTop
      // where they are and request an explicit resume so the next frame snaps
      // (jumpTo, not spring-from-0) to the freshly measured centered target for
      // the current active line. lastResumeGenerationRef is intentionally left
      // stale so the next tick observes the bump as an explicit resume.
      requestLyricsAutoScrollResume();
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

  // RATIONALE (#202): The rAF loop only recomputes the scroll target when the
  // active line index changes; between line changes it re-asserts the settled
  // spring pixel position every frame. So resizing the window / maximizing /
  // toggling the sidebar while a line is held (slow ballads, long instrumental
  // holds) reflows the content but leaves the spring parked at a stale pixel
  // target — the active line drifts off-center and stays there until the next
  // line change. Mirror the audience paging hook and observe the scroll
  // container; on a real viewport resize, re-center the current active line and
  // snap the container to it. Skip while the user is browsing (guard active) so
  // a resize never yanks them, and debounce to coalesce continuous resizes.
  useLayoutEffect(() => {
    if (isPlainText || !viewportActive) {
      return;
    }
    const container = containerRef.current;
    if (!container || typeof ResizeObserver === "undefined") {
      return;
    }

    // Observe only the viewport box (not the content wrapper): mid-line
    // per-character emphasis / weight swaps reflow content height while the line
    // index is unchanged, and re-anchoring on that would reintroduce the exact
    // jitter tickLyricsEngineScroll deliberately avoids. Content-driven layout
    // changes (Romanize, font step) are already re-anchored by the engine
    // effect above via layoutVersion / lyricsFontStep.
    let lastWidth = container.clientWidth;
    let lastHeight = container.clientHeight;
    let debounceId: ReturnType<typeof setTimeout> | null = null;

    const reanchorToActiveLine = () => {
      debounceId = null;
      const currentContainer = containerRef.current;
      if (!currentContainer) {
        return;
      }
      // Never yank a user who has scrolled away to browse the lyrics.
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
      // ResizeObserver fires an initial callback on observe() and can fire for
      // sub-pixel/no-op churn; only re-anchor when the viewport box changed.
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
