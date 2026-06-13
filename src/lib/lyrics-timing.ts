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

/** 0 at the current line timestamp, 1 just before the next line begins. */
export function getLinePlaybackProgress(
  lines: TimedLyricLine[],
  activeIndex: number,
  adjustedMs: number,
): number {
  if (activeIndex < 0 || activeIndex >= lines.length) {
    return 0;
  }

  const current = lines[activeIndex];
  const next = lines[activeIndex + 1];
  if (!next) {
    return 0;
  }

  const duration = next.time_ms - current.time_ms;
  if (duration <= 0) {
    return 0;
  }

  return Math.max(0, Math.min(1, (adjustedMs - current.time_ms) / duration));
}

/** Continuous playback position in "line space" for smooth scroll/highlight. */
export function getVirtualLineCenter(
  lines: TimedLyricLine[],
  adjustedMs: number,
): number {
  if (lines.length === 0) {
    return 0;
  }

  const activeIndex = findActiveLyricLineIndex(lines, adjustedMs);
  if (activeIndex < 0) {
    return 0;
  }

  return activeIndex + getLinePlaybackProgress(lines, activeIndex, adjustedMs);
}

export function smoothstep(value: number): number {
  const t = Math.max(0, Math.min(1, value));
  return t * t * (3 - 2 * t);
}
