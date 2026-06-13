interface CenteredScrollTopInput {
  viewportHeight: number;
  scrollHeight: number;
  lineOffsetTop: number;
  lineHeight: number;
}

export function getCenteredScrollTop({
  viewportHeight,
  scrollHeight,
  lineOffsetTop,
  lineHeight,
}: CenteredScrollTopInput): number {
  const centeredTop = lineOffsetTop + lineHeight / 2 - viewportHeight / 2;
  const maxScrollTop = Math.max(0, scrollHeight - viewportHeight);

  return Math.max(0, Math.min(maxScrollTop, Math.round(centeredTop)));
}

export interface LyricLineScrollMetrics {
  offsetTop: number;
  height: number;
}

export function getLineScrollMetrics(
  container: HTMLElement,
  lineIndex: number,
): LyricLineScrollMetrics | null {
  const lineEl = container.querySelector<HTMLElement>(
    `[data-lyrics-line-index="${lineIndex}"]`,
  );
  if (!lineEl) {
    return null;
  }

  return {
    offsetTop: lineEl.offsetTop,
    height: lineEl.clientHeight,
  };
}

export function getScrollTopForLineIndex(
  container: HTMLElement,
  lineIndex: number,
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
  });
}
