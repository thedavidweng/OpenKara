export interface LyricsTimeFrame {
  positionMs: number;
  /**
   * True for the next sampled frame after a seek latch. Consumed once —
   * equivalent to AMLL's isSeek argument on setCurrentTime.
   */
  isSeek: boolean;
}

let currentPositionMs = 0;
let pendingIsSeek = false;

export function setLyricsCurrentTime(
  positionMs: number,
  options: { isSeek?: boolean } = {},
): void {
  currentPositionMs = positionMs;
  if (options.isSeek) {
    pendingIsSeek = true;
  }
}

export function sampleLyricsTimeFrame(): LyricsTimeFrame {
  const isSeek = pendingIsSeek;
  pendingIsSeek = false;
  return { positionMs: currentPositionMs, isSeek };
}

export function resetLyricsPlaybackTimeForTests(): void {
  currentPositionMs = 0;
  pendingIsSeek = false;
}
