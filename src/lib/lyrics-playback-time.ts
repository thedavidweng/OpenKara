export interface LyricsTimeFrame {
  positionMs: number;
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
