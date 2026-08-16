export const LIST_ALIGN_POSITION = 0.5;
export const FOCUS_ALIGN_POSITION = 0.45;

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
