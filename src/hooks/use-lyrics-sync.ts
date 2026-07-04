import { syncLyricsActiveLine } from "@/lib/lyrics-engine";

/** @deprecated Lyrics sync now runs inside `useLyricsEngine`. */
export const syncLyricsToPlayback = syncLyricsActiveLine;

/** @deprecated Interval sync replaced by the unified lyrics engine rAF loop. */
export function startLyricsSyncLoop(
  tick: () => void,
  timers: Pick<typeof globalThis, "setInterval" | "clearInterval"> = globalThis,
): () => void {
  const timer = timers.setInterval(tick, 33);
  return () => timers.clearInterval(timer);
}

/** @deprecated Use `useLyricsEngine` inside `LyricsPanel` instead. */
export function useLyricsSync(): void {}
