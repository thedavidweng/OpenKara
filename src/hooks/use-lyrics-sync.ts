import { useEffect, useRef } from "react";
import { findActiveLyricLineIndex } from "@/lib/lyrics-timing";
import { selectCurrentPositionMs, usePlayerStore } from "@/stores/player-store";
import { useLyricsStore } from "@/stores/lyrics-store";

const LYRICS_SYNC_INTERVAL_MS = 33;

export function syncLyricsToPlayback(prevIndexRef: { current: number }) {
  const state = usePlayerStore.getState();
  const { snapshot } = state;
  const { lines, offsetMs, setActiveLineIndex } = useLyricsStore.getState();

  // Allow sync when paused so seek-while-paused immediately updates the active line.
  // Guard only against no song loaded or no lines to sync.
  if (!snapshot?.song_id || lines.length === 0) {
    return;
  }

  // For determining the highlighted lyric line we extrapolate from the last
  // known authoritative position so we stay smooth even when IPC position
  // events are late or lost. For the audience-pacing display clock we still
  // consult selectSyncDisplayPositionMs.
  const positionMs = selectCurrentPositionMs(state);
  const adjustedMs = positionMs - offsetMs;
  const index = findActiveLyricLineIndex(lines, adjustedMs);

  if (index !== prevIndexRef.current) {
    prevIndexRef.current = index;
    setActiveLineIndex(index);
  }
}
export function startLyricsSyncLoop(
  tick: () => void,
  timers: Pick<typeof globalThis, "setInterval" | "clearInterval"> = globalThis,
): () => void {
  const timer = timers.setInterval(tick, LYRICS_SYNC_INTERVAL_MS);
  return () => timers.clearInterval(timer);
}

export function useLyricsSync(enabled = true): void {
  const prevIndexRef = useRef(-1);
  const hasSong = usePlayerStore((s) => !!s.snapshot?.song_id);

  useEffect(() => {
    if (!enabled || !hasSong) {
      return;
    }

    const stopLoop = startLyricsSyncLoop(() =>
      syncLyricsToPlayback(prevIndexRef),
    );

    // Force-sync lyrics when window regains focus so the current line snaps
    // into place immediately after backgrounding or monitor changes.
    const syncNow = () => {
      syncLyricsToPlayback(prevIndexRef);
    };
    window.addEventListener("focus", syncNow);

    return () => {
      stopLoop();
      window.removeEventListener("focus", syncNow);
    };
  }, [enabled, hasSong]);
}
