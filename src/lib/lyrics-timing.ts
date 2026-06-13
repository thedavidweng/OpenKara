export interface TimedLyricLine {
  time_ms: number;
}

export function findActiveLyricLineIndex(
  lines: TimedLyricLine[],
  adjustedMs: number,
): number {
  let lo = 0;
  let hi = lines.length - 1;
  let result = -1;

  while (lo <= hi) {
    const mid = (lo + hi) >>> 1;
    if (lines[mid].time_ms <= adjustedMs) {
      result = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }

  return result;
}
