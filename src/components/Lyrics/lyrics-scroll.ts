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
  // RATIONALE: getBoundingClientRect returns viewport-relative coordinates
  // that already account for the current scroll position. To get the line's
  // position relative to the container's content origin (independent of
  // scrollTop), subtract the container's top edge and add back scrollTop:
  //   offsetTop = (lineRect.top - containerRect.top) + container.scrollTop
  // The previous formula (lineRect.top - containerRect.top - scrollTop)
  // double-subtracted scrollTop, producing negative offsets when scrolled
  // and clamping getCenteredScrollTop to 0 — freezing auto-follow at the top.
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
