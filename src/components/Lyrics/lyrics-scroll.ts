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

  // RATIONALE: offsetTop is relative to offsetParent, which may be a nested
  // container with justify-center (audience mode). When the content is
  // vertically centered, lines above the center have negative offsetTop,
  // and getCenteredScrollTop clamps to 0 — freezing scroll at the top.
  // When the line's offsetParent is the scroll container itself (or null in
  // jsdom tests that mock offsetTop directly), use offsetTop directly.
  // When offsetParent is a different nested element, use getBoundingClientRect
  // to compute the position relative to the scroll container.
  if (lineEl.offsetParent === container || lineEl.offsetParent === null) {
    return {
      offsetTop: lineEl.offsetTop,
      height: lineEl.clientHeight,
    };
  }

  // offsetParent is a nested element (e.g. the justify-center inner div).
  // Compute the line's absolute position within the scroll container.
  const containerRect = container.getBoundingClientRect();
  const lineRect = lineEl.getBoundingClientRect();
  const containerContentTop = containerRect.top + container.scrollTop;
  const offsetTop = lineRect.top - containerContentTop;

  return {
    offsetTop,
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
