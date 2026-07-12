/**
 * AMLL-shaped host→lyrics time feed.
 *
 * RATIONALE: Mature lyric players (AMLL) keep a hard boundary — the host owns
 * the playback clock and calls setCurrentTime(ms, { isSeek }) each frame; the
 * lyric engine only consumes that sample for line/word sync, karaoke fill, and
 * scroll. OpenKara mirrors that contract so word-level timing has one stable
 * clock source and seeks are explicit instead of only inferred from jumps.
 *
 * Pair {@link markLyricsSeekFlag} with `requestLyricsAutoScrollResume()` (or
 * use the wrapper `markLyricsSeek` in lyrics-engine) so seek also resetScrolls.
 */

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
 * Prefer {@link markLyricsSeekFlag} before async seeks; use `isSeek: true` when
 * the discontinuous time is known synchronously.
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

/** Latch isSeek for the next {@link sampleLyricsTimeFrame} (does not resetScroll). */
export function markLyricsSeekFlag(): void {
  pendingIsSeek = true;
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
