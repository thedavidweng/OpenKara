export interface TimedLyricLine {
  time_ms: number;
}

export interface TimedLyricWord {
  time_ms: number;
}

export function findActiveWordIndex(
  words: TimedLyricWord[],
  adjustedMs: number,
): number {
  let activeIndex = -1;

  for (let index = 0; index < words.length; index += 1) {
    if (words[index].time_ms > adjustedMs) {
      break;
    }
    activeIndex = index;
  }

  return activeIndex;
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
