export const LIST_ALIGN_POSITION = 0.5;
export const FOCUS_ALIGN_POSITION = 0.38;

interface CenteredScrollTopInput {
  viewportHeight: number;
  scrollHeight: number;
  lineOffsetTop: number;
  lineHeight: number;
  alignPosition?: number;
}

export function getCenteredScrollTop({
  viewportHeight,
  scrollHeight,
  lineOffsetTop,
  lineHeight,
  alignPosition = LIST_ALIGN_POSITION,
}: CenteredScrollTopInput): number {
  const centeredTop =
    lineOffsetTop + lineHeight / 2 - viewportHeight * alignPosition;
  const maxScrollTop = Math.max(0, scrollHeight - viewportHeight);

  return Math.max(0, Math.min(maxScrollTop, Math.round(centeredTop)));
}

interface LyricLineScrollMetrics {
  offsetTop: number;
  height: number;
}

function getLineScrollMetrics(
  container: HTMLElement,
  lineIndex: number,
): LyricLineScrollMetrics | null {
  const lineEl = container.querySelector<HTMLElement>(
    `[data-lyrics-line-index="${lineIndex}"]`,
  );
  if (!lineEl) {
    return null;
  }

  if (lineEl.offsetParent === container || lineEl.offsetParent === null) {
    return {
      offsetTop: lineEl.offsetTop,
      height: lineEl.clientHeight,
    };
  }

  const containerRect = container.getBoundingClientRect();
  const lineRect = lineEl.getBoundingClientRect();
  const offsetTop = lineRect.top - containerRect.top + container.scrollTop;

  return {
    offsetTop,
    height: lineEl.clientHeight,
  };
}

export function getScrollTopForLineIndex(
  container: HTMLElement,
  lineIndex: number,
  alignPosition: number = LIST_ALIGN_POSITION,
): number | null {
  const metrics = getLineScrollMetrics(container, lineIndex);
  if (!metrics) {
    return null;
  }

  return getCenteredScrollTop({
    viewportHeight: container.clientHeight,
    scrollHeight: container.scrollHeight,
    lineOffsetTop: metrics.offsetTop,
    lineHeight: metrics.height,
    alignPosition,
  });
}

export const SCROLL_DECELERATION_RATE = 0.998;

export function projectDecelerationOffset(
  velocityPxPerSec: number,
  decelerationRate = SCROLL_DECELERATION_RATE,
): number {
  if (
    velocityPxPerSec === 0 ||
    decelerationRate <= 0 ||
    decelerationRate >= 1
  ) {
    return 0;
  }
  return (
    ((velocityPxPerSec / 1000) * decelerationRate) / (1 - decelerationRate)
  );
}

export function collectLineSnapScrollTops(
  container: HTMLElement,
  lineCount: number,
  alignPosition: number = LIST_ALIGN_POSITION,
): number[] {
  const snaps: number[] = [];
  let previous = Number.NaN;
  for (let index = 0; index < lineCount; index += 1) {
    const top = getScrollTopForLineIndex(container, index, alignPosition);
    if (top === null || top === previous) {
      continue;
    }
    snaps.push(top);
    previous = top;
  }
  return snaps;
}

export function nearestSnapScrollTop(
  snaps: readonly number[],
  position: number,
): number | null {
  if (snaps.length === 0) {
    return null;
  }
  let best = snaps[0];
  let bestDist = Math.abs(position - best);
  for (let index = 1; index < snaps.length; index += 1) {
    const dist = Math.abs(position - snaps[index]);
    if (dist < bestDist) {
      best = snaps[index];
      bestDist = dist;
    }
  }
  return best;
}

export function resolveLyricScrollLanding(
  snaps: readonly number[],
  position: number,
  velocityPxPerSec: number,
): number | null {
  return nearestSnapScrollTop(
    snaps,
    position + projectDecelerationOffset(velocityPxPerSec),
  );
}

export function stepLineSnapScrollTop(
  snaps: readonly number[],
  position: number,
  direction: 1 | -1,
): number | null {
  if (snaps.length === 0) {
    return null;
  }
  if (direction > 0) {
    for (const snap of snaps) {
      if (snap > position + 0.5) {
        return snap;
      }
    }
    return snaps[snaps.length - 1];
  }
  for (let index = snaps.length - 1; index >= 0; index -= 1) {
    if (snaps[index] < position - 0.5) {
      return snaps[index];
    }
  }
  return snaps[0];
}
