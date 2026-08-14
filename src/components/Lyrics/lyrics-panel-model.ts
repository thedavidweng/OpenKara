import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import { useAudiencePlainTextPaging } from "@/hooks/use-audience-plain-text-paging";
import { useLyricsEngine } from "@/hooks/use-lyrics-engine";
import { buildAudiencePresentationSpec } from "@/lib/audience-presentation";
import type { LyricsAlignment } from "@/lib/lyrics-session";
import { lyricsLineRuntime } from "@/lib/lyrics-line-runtime";
import {
  resolvePlainTextRemoteTarget,
  stepPlainTextRemotePage,
  type PlainTextPageDirection,
} from "@/lib/plain-text-page-controls";
import { lyricsSession, useLyricsStore } from "@/stores/lyrics-store";
import { usePlayerStore } from "@/stores/player-store";
import { useSettingsStore } from "@/stores/settings-store";
import type { AudiencePresentationSpec, LyricLine } from "@/types/ipc";

export type LyricsPresentation = "standard" | "audience";

export type LyricLineState = "active" | "past" | "future" | "plain";

export interface RemotePageModel {
  visible: boolean;
  locked: boolean;
  pendingDirection: PlainTextPageDirection | null;
  step: (direction: PlainTextPageDirection) => void;
}

export interface LyricsPanelModel {
  presentation: LyricsPresentation;
  isAudience: boolean;
  spaciousStageLayout: boolean;
  audienceSpec: AudiencePresentationSpec;

  songId: string | null | undefined;
  isLoading: boolean;
  lines: LyricLine[];
  rawLrc: string;
  lyricsFontStep: number;
  lyricsAlignment: LyricsAlignment;
  showRomanized: boolean;
  isRomanizing: boolean;
  toggleRomanized: () => void;
  toggleLyricsAlignment: () => void;

  utilityControlsPinned: boolean;
  isPlainText: boolean;
  userScrollUnlocked: boolean;
  requestFollow: () => void;

  containerRef: RefObject<HTMLDivElement | null>;
  measurementRef: RefObject<HTMLDivElement | null>;
  paged: boolean;
  currentPageStart: number;
  visibleLines: LyricLine[];

  remotePage: RemotePageModel;

  lineState: (absoluteIndex: number) => LyricLineState;
  romanizedTextAt: (absoluteIndex: number) => string | undefined;
  activeWordIndexAt: (absoluteIndex: number) => number;
  registerLineWrapper: (
    lineIndex: number,
  ) => (node: HTMLDivElement | null) => (() => void) | void;
}

/**
 * Everything the lyrics panel needs that is not markup: which lyrics are on
 * screen, how they are paged for an audience display, who currently owns the
 * viewport, and where a remote page step should be sent. The panel below it
 * only decides how the result looks.
 */
export function useLyricsPanelModel(
  presentation: LyricsPresentation,
): LyricsPanelModel {
  const lines = useLyricsStore((s) => s.lines);
  const activeLineIndex = useLyricsStore((s) => s.activeLineIndex);
  const activeWordIndex = useLyricsStore((s) => s.activeWordIndex);
  const offsetMs = useLyricsStore((s) => s.offsetMs);
  const isLoading = useLyricsStore((s) => s.isLoading);
  const rawLrc = useLyricsStore((s) => s.rawLrc);
  const romanizedLines = useLyricsStore((s) => s.romanizedLines);
  const isRomanizing = useLyricsStore((s) => s.isRomanizing);
  const showRomanized = useLyricsStore((s) => s.showRomanized);
  const toggleRomanized = useLyricsStore((s) => s.toggleRomanized);
  const lyricsAlignment = useLyricsStore((s) => s.lyricsAlignment);
  const toggleLyricsAlignment = useLyricsStore((s) => s.toggleLyricsAlignment);

  const songId = usePlayerStore((s) => s.snapshot?.song_id);
  const airPlayOutput = usePlayerStore((s) => s.airPlayOutput);
  const localAudienceOutputActive = usePlayerStore(
    (s) => s.localAudienceOutputActive,
  );
  const airPlayPlainTextPagePending = usePlayerStore(
    (s) => s.airPlayPlainTextPagePending,
  );
  const airPlayPlainTextPagePendingDirection = usePlayerStore(
    (s) => s.airPlayPlainTextPagePendingDirection,
  );
  const lyricsFontStep = useSettingsStore((s) => s.lyricsFontStep);

  const [userScrollUnlocked, setUserScrollUnlocked] = useState(false);

  const isAudience = presentation === "audience";
  const spaciousStageLayout = !isAudience;
  const audienceSpec = buildAudiencePresentationSpec(lyricsFontStep);
  const utilityControlsPinned = offsetMs !== 0 || lyricsFontStep !== 0;
  const isPlainText =
    lines.length > 0 && lines.every((line) => line.time_ms === 0);

  const remoteTarget = resolvePlainTextRemoteTarget(
    airPlayOutput,
    localAudienceOutputActive,
  );
  const isAirPlayRemoteTarget = remoteTarget === "airplay";
  const remotePageLocked = isAirPlayRemoteTarget && airPlayPlainTextPagePending;
  const paged = isAudience && isPlainText;
  const pageIdentity = paged
    ? `${songId ?? ""}:${rawLrc}:${lyricsFontStep}`
    : "local";
  const layoutVersion = `${showRomanized}:${romanizedLines.join("\u0000")}`;

  const { containerRef, measurementRef, currentPageStart, visibleLines } =
    useAudiencePlainTextPaging({
      lines,
      shouldRender: paged,
      pageIdentity,
      audiencePresentationSpec: audienceSpec,
      layoutVersion,
    });

  useLyricsEngine({
    containerRef,
    isPlainText,
    lyricsFontStep,
    presentation,
    focusStage: lyricsAlignment === "center" && !isPlainText,
    songId,
    viewportActive: Boolean(songId) && !isLoading && lines.length > 0,
    layoutVersion,
    lineRuntime: lyricsLineRuntime,
    onUserScrollActiveChange: setUserScrollUnlocked,
  });

  const lastPendingSongIdRef = useRef<string | null>(null);
  useEffect(() => {
    if (!airPlayPlainTextPagePending) {
      lastPendingSongIdRef.current = songId ?? null;
      return;
    }

    const songChanged =
      lastPendingSongIdRef.current !== null &&
      lastPendingSongIdRef.current !== (songId ?? null);
    if (isAudience || songChanged || !isPlainText || !isAirPlayRemoteTarget) {
      usePlayerStore.getState().clearAirPlayPlainTextPagePending();
      return;
    }

    lastPendingSongIdRef.current = songId ?? null;
  }, [
    airPlayPlainTextPagePending,
    isAirPlayRemoteTarget,
    isAudience,
    isPlainText,
    songId,
  ]);

  const lineRefCallbacksRef = useRef(
    new Map<number, (node: HTMLDivElement | null) => (() => void) | void>(),
  );
  const prevSongIdForRefsRef = useRef(songId);
  if (prevSongIdForRefsRef.current !== songId) {
    lineRefCallbacksRef.current.clear();
    prevSongIdForRefsRef.current = songId;
  }
  const registerLineWrapper = useCallback((lineIndex: number) => {
    const cached = lineRefCallbacksRef.current.get(lineIndex);
    if (cached) {
      return cached;
    }
    const callback = (node: HTMLDivElement | null) => {
      if (!node) {
        return;
      }
      lyricsLineRuntime.registerWrapper(lineIndex, node);
      return () => {
        lyricsLineRuntime.unregisterWrapper(lineIndex);
      };
    };
    lineRefCallbacksRef.current.set(lineIndex, callback);
    return callback;
  }, []);

  return {
    presentation,
    isAudience,
    spaciousStageLayout,
    audienceSpec,

    songId,
    isLoading,
    lines,
    rawLrc,
    lyricsFontStep,
    lyricsAlignment,
    showRomanized,
    isRomanizing,
    toggleRomanized,
    toggleLyricsAlignment,

    utilityControlsPinned,
    isPlainText,
    userScrollUnlocked,
    requestFollow: () => lyricsSession.scroll.requestResume(),

    containerRef,
    measurementRef,
    paged,
    currentPageStart,
    visibleLines,

    remotePage: {
      visible: !isAudience && isPlainText && remoteTarget !== null,
      locked: remotePageLocked,
      pendingDirection: airPlayPlainTextPagePendingDirection,
      step: (direction) => {
        void stepPlainTextRemotePage(
          airPlayOutput,
          localAudienceOutputActive,
          direction,
        ).catch(() => {});
      },
    },

    lineState: (absoluteIndex) =>
      isPlainText
        ? "plain"
        : absoluteIndex === activeLineIndex
          ? "active"
          : absoluteIndex < activeLineIndex
            ? "past"
            : "future",
    romanizedTextAt: (absoluteIndex) =>
      showRomanized ? romanizedLines[absoluteIndex] : undefined,
    activeWordIndexAt: (absoluteIndex) =>
      absoluteIndex === activeLineIndex ? activeWordIndex : -1,
    registerLineWrapper,
  };
}
