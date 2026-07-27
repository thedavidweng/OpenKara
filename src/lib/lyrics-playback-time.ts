export interface LyricsTimeFrame {
  /** Host playback position in ms (before lyrics offset). */
  positionMs: number;
  /**
   * True for the next sampled frame after a seek latch. Consumed once —
   * equivalent to AMLL's isSeek argument on setCurrentTime.
   */
  isSeek: boolean;
}

let currentPositionMs = 0;
let pendingIsSeek = false;

/**
 * Push the host clock into the lyrics feed (call once per animation frame).
 * Use `isSeek: true` when the discontinuous host time has been committed.
 */
export function setLyricsCurrentTime(
  positionMs: number,
  options: { isSeek?: boolean } = {},
): void {
  currentPositionMs = positionMs;
  if (options.isSeek) {
    pendingIsSeek = true;
  }
}

/** Take one frame sample for the lyrics engine; clears the isSeek latch. */
export function sampleLyricsTimeFrame(): LyricsTimeFrame {
  const isSeek = pendingIsSeek;
  pendingIsSeek = false;
  return { positionMs: currentPositionMs, isSeek };
}

/** Test-only: reset module latches between cases. */
export function resetLyricsPlaybackTimeForTests(): void {
  currentPositionMs = 0;
  pendingIsSeek = false;
}
